//! Actual binary tests: compatible configuration -> provider -> tools -> answer.
//! Only synthetic files and loopback endpoints; no Pi runtime or real auth.

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn cli(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nahida"));
    command
        .env_clear()
        .env("HOME", root)
        .env("XDG_DATA_HOME", root)
        .env("ANTHROPIC_API_KEY", "unrelated-secret")
        .arg("--root")
        .arg(root.join("workspace"))
        .arg("--config-dir")
        .arg(root.join("config"))
        .args([
            "--provider",
            "local",
            "--model",
            "test-model",
            "--no-session",
            "--max-retries",
            "0",
        ])
        .stdin(Stdio::null());
    command
}

fn setup(base_url: &str, key: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("workspace")).unwrap();
    std::fs::create_dir(root.path().join("config")).unwrap();
    let config = json!({"providers":{"local": {
        "api":"openai-completions", "baseUrl":base_url, "apiKey":key,
        "models":[{"id":"test-model","maxTokens":1024}]
    }}});
    std::fs::write(root.path().join("config/models.json"), config.to_string()).unwrap();
    root
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn request(listener: &TcpListener) -> (TcpStream, String, Value) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "Nahida did not contact the fake provider");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept: {error}"),
        }
    };
    // Darwin can inherit O_NONBLOCK from the accept listener. The per-socket
    // timeout below only bounds a blocking read; do not race request arrival.
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut bytes = Vec::new();
    let (headers, length, body_start) = loop {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "request ended before headers");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() <= 1024 * 1024, "oversized test request");
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            assert!(length <= 1024 * 1024);
            break (headers, length, end + 4);
        }
    };
    while bytes.len() < body_start + length {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "request ended before body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
    (stream, headers, body)
}

fn reply(mut stream: TcpStream, status: &str, content_type: &str, body: &str) {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}

fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}

#[test]
fn configuration_drives_an_actual_tool_turn_and_final_answer() {
    let listener = listener();
    let root = setup(&format!("http://{}/v1", listener.local_addr().unwrap()), "config-secret");
    std::fs::write(
        root.path().join("config/auth.json"),
        json!({
            "local":{"type":"api_key","key":"stored-secret"},
            "unrelated":{"type":"oauth","access":"other-secret"}
        })
        .to_string(),
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        let (stream, headers, first) = request(&listener);
        assert!(headers.starts_with("POST /v1/chat/completions "));
        assert!(headers.to_ascii_lowercase().contains("authorization: bearer stored-secret"));
        assert!(!headers.contains("unrelated-secret"));
        assert_eq!(first["model"], "test-model");
        assert_eq!(first["max_tokens"], 1024);
        assert!(
            first["tools"].as_array().unwrap().iter().any(|t| t["function"]["name"] == "write")
        );
        let call = json!({"id":"turn-1","model":"test-model","choices":[{
            "delta":{"tool_calls":[{"index":0,"id":"call-write","function":{
                "name":"write","arguments":json!({"path":"answer.txt","content":"native works"}).to_string()
            }}]},"finish_reason":"tool_calls"
        }]});
        reply(stream, "200 OK", "text/event-stream", &format!("data: {call}\n\ndata: [DONE]\n\n"));
        let (stream, _, second) = request(&listener);
        assert!(
            second["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "tool" && m["tool_call_id"] == "call-write")
        );
        let answer = json!({"id":"turn-2","model":"test-model","choices":[{
            "delta":{"content":"Native config works."},"finish_reason":"stop"
        }]});
        reply(
            stream,
            "200 OK",
            "text/event-stream",
            &format!("data: {answer}\n\ndata: [DONE]\n\n"),
        );
    });
    let output = cli(root.path()).args(["--json", "write the answer"]).output().unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("Native config works."));
    assert_eq!(
        std::fs::read_to_string(root.path().join("workspace/answer.txt")).unwrap(),
        "native works"
    );
    for secret in ["config-secret", "stored-secret", "other-secret", "unrelated-secret"] {
        assert!(!text(&output).contains(secret), "credential leaked");
    }
}

#[test]
fn anthropic_configuration_uses_its_native_wire_and_key_header() {
    let listener = listener();
    let root = setup(&format!("http://{}", listener.local_addr().unwrap()), "anthropic-secret");
    let config_path = root.path().join("config/models.json");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["providers"]["local"]["api"] = json!("anthropic-messages");
    std::fs::write(config_path, config.to_string()).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, headers, body) = request(&listener);
        assert!(headers.starts_with("POST /v1/messages "));
        assert!(headers.to_ascii_lowercase().contains("x-api-key: anthropic-secret"));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        assert_eq!(body["model"], "test-model");
        let events = [
            json!({"type":"message_start","message":{"id":"a","model":"test-model"}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Anthropic native works."}}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"}),
        ];
        let mut frames = String::new();
        for event in events {
            write!(frames, "data: {event}\n\n").unwrap();
        }
        reply(stream, "200 OK", "text/event-stream", &frames);
    });
    let output = cli(root.path()).arg("hello").output().unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("Anthropic native works."));
    assert!(!text(&output).contains("anthropic-secret"));
}

#[test]
fn describe_does_not_read_auth_or_connect() {
    let listener = listener();
    let root = setup(
        &format!("http://{}/endpoint-secret", listener.local_addr().unwrap()),
        "literal-secret",
    );
    // A directory cannot be decoded as auth JSON, even if tests run as root.
    std::fs::create_dir(root.path().join("config/auth.json")).unwrap();
    let output = cli(root.path()).arg("--describe").output().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("test-model"));
    assert!(!text(&output).contains("literal-secret"));
    assert!(!text(&output).contains("endpoint-secret"));
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[cfg(unix)]
#[test]
fn built_in_description_never_even_stats_the_auth_store() {
    let root = setup("http://127.0.0.1:1/v1", "fake-key");
    std::os::unix::fs::symlink("auth.json", root.path().join("config/auth.json")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nahida"))
        .env_clear()
        .env("HOME", root.path())
        .arg("--root")
        .arg(root.path().join("workspace"))
        .arg("--config-dir")
        .arg(root.path().join("config"))
        .args(["--provider", "anthropic", "--describe"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("provider      anthropic"));
    assert!(text(&output).contains("claude-opus-5"));
}

#[test]
fn credential_commands_fail_without_execution_or_connection() {
    let listener = listener();
    let root = setup(&format!("http://{}/v1", listener.local_addr().unwrap()), "!touch marker");
    let output = cli(root.path()).arg("hello").output().unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("no command executed"));
    assert!(!root.path().join("workspace/marker").exists());
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn built_in_saved_key_preserves_bearer_auth_with_a_custom_endpoint() {
    let listener = listener();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = setup("http://127.0.0.1:1/v1", "unused-key");
    std::fs::write(
        root.path().join("config/auth.json"),
        r#"{"zai":{"type":"api_key","key":"saved-global-secret"}}"#,
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        let (stream, headers, body) = request(&listener);
        assert!(headers.starts_with("POST /v1/messages "));
        assert!(headers.to_ascii_lowercase().contains("authorization: bearer saved-global-secret"));
        assert!(!headers.to_ascii_lowercase().contains("x-api-key:"));
        assert_eq!(body["model"], "test-model");
        reply(
            stream,
            "200 OK",
            "text/event-stream",
            concat!(
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"a\",\"model\":\"test-model\"}}\n\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Saved key works.\"}}\n\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
                "data: {\"type\":\"message_stop\"}\n\n"
            ),
        );
    });
    let output = Command::new(env!("CARGO_BIN_EXE_nahida"))
        .env_clear()
        .env("HOME", root.path())
        .env("ANTHROPIC_BASE_URL", endpoint)
        .arg("--root")
        .arg(root.path().join("workspace"))
        .arg("--config-dir")
        .arg(root.path().join("config"))
        .args([
            "--provider",
            "zai",
            "--model",
            "test-model",
            "--no-session",
            "--max-retries",
            "0",
            "hello",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("Saved key works."));
    assert!(!text(&output).contains("saved-global-secret"));
}

#[test]
fn stored_oauth_never_falls_back_or_changes_the_auth_file() {
    let listener = listener();
    let root = setup(&format!("http://{}/v1", listener.local_addr().unwrap()), "config-secret");
    let path = root.path().join("config/auth.json");
    let auth =
        r#"{"local":{"type":"oauth","access":"fake-oauth-secret","refresh":"fake-refresh"}}"#;
    std::fs::write(&path, auth).unwrap();
    let output = cli(root.path()).arg("hello").output().unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("no fallback attempted"));
    assert!(!text(&output).contains("fake-oauth-secret"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), auth);
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn default_nahida_home_settings_and_environment_template_drive_the_binary() {
    let listener = listener();
    let root = setup(&format!("http://{}/v1", listener.local_addr().unwrap()), "${FIXTURE_KEY}");
    std::fs::write(
        root.path().join("config/settings.json"),
        r#"{"defaultProvider":"local","defaultModel":"test-model"}"#,
    )
    .unwrap();
    std::fs::rename(root.path().join("config"), root.path().join("nahida")).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, headers, body) = request(&listener);
        assert!(headers.to_ascii_lowercase().contains("authorization: bearer template-secret"));
        assert_eq!(body["model"], "test-model");
        let answer = json!({"id":"defaults","model":"test-model","choices":[{
            "delta":{"content":"Nahida defaults work."},"finish_reason":"stop"
        }]});
        reply(
            stream,
            "200 OK",
            "text/event-stream",
            &format!("data: {answer}\n\ndata: [DONE]\n\n"),
        );
    });
    let output = Command::new(env!("CARGO_BIN_EXE_nahida"))
        .env_clear()
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path())
        .env("FIXTURE_KEY", "template-secret")
        .env("ANTHROPIC_API_KEY", "unrelated-secret")
        .arg("--root")
        .arg(root.path().join("workspace"))
        .args(["--no-session", "--max-retries", "0", "hello"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("Nahida defaults work."));
    assert!(!text(&output).contains("template-secret"));
}

#[test]
fn selection_is_ordinary_nahida_flags_not_a_pi_mode() {
    let root = setup("http://127.0.0.1:1/v1", "fake-key");
    let output = cli(root.path()).arg("--help").output().unwrap();
    assert!(output.status.success());
    for flag in ["--provider", "--model", "--config-dir"] {
        assert!(text(&output).contains(flag));
    }
    for flag in ["--pi-model", "--pi-agent-dir"] {
        assert!(!text(&output).contains(flag));
        let output = cli(root.path()).arg(flag).arg("anything").output().unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn provider_errors_cannot_echo_the_configured_key() {
    let listener = listener();
    let root = setup(&format!("http://{}/v1", listener.local_addr().unwrap()), "literal-secret");
    let server = std::thread::spawn(move || {
        let (stream, _, _) = request(&listener);
        reply(
            stream,
            "401 Unauthorized",
            "application/json",
            r#"{"error":{"type":"secret","message":"literal-secret"}}"#,
        );
    });
    let output = cli(root.path()).arg("hello").output().unwrap();
    server.join().unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("401"));
    assert!(!text(&output).contains("literal-secret"));
}
