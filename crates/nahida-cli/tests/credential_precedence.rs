//! Actual-binary credential precedence regressions using synthetic config and
//! loopback HTTP only. No real auth store or provider endpoint is accessed.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};

use serde_json::json;

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("config")).unwrap();
    std::fs::create_dir(root.path().join("workspace")).unwrap();
    root
}

fn configure_cn(root: &std::path::Path, endpoint: &str) {
    std::fs::write(
        root.join("config/models.json"),
        json!({"providers":{"zai-coding-cn":{
            "api":"openai-completions",
            "baseUrl":endpoint,
            "models":[{"id":"glm-5.3"}]
        }}})
        .to_string(),
    )
    .unwrap();
}

fn base_command(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nahida"));
    command
        .env_clear()
        .env("HOME", root)
        .env("XDG_DATA_HOME", root)
        .arg("--root")
        .arg(root.join("workspace"))
        .arg("--config-dir")
        .arg(root.join("config"))
        .args(["--no-session", "--max-retries", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run_with_timeout(mut command: Command) -> Output {
    let mut child = command.spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("binary exceeded 10-second fixture timeout: {}", text(&output));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn accept(listener: &TcpListener) -> std::io::Result<(TcpStream, std::net::SocketAddr)> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok(accepted) => return Ok(accepted),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}

fn read_headers(stream: &mut TcpStream) -> String {
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let mut chunk = [0; 2048];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "client closed before HTTP headers");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 64 * 1024, "oversized HTTP headers");
    }
    let end = bytes.windows(4).position(|window| window == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while bytes.len() < end + 4 + length {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "client closed before request body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    headers
}

fn answer_anthropic(mut stream: TcpStream) {
    let body = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\",\"model\":\"claude-opus-5\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    stream.flush().unwrap();
}

fn answer(mut stream: TcpStream) {
    let body = r#"data: {"id":"fixture","model":"glm-5.3","choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}

data: [DONE]

"#;
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    stream.flush().unwrap();
}

#[test]
fn automatic_selection_uses_saved_api_key_before_ambient_key() {
    let listener = listener();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let root = root();
    configure_cn(root.path(), &endpoint);
    std::fs::write(
        root.path().join("config/auth.json"),
        r#"{"zai-coding-cn":{"type":"api_key","key":"stored-test-key"}}"#,
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = accept(&listener).unwrap();
        let headers = read_headers(&mut stream);
        assert!(headers.starts_with("POST /v1/chat/completions "), "{headers}");
        assert!(
            headers.to_ascii_lowercase().contains("authorization: bearer stored-test-key"),
            "{headers}"
        );
        assert!(!headers.contains("ambient-test-key"), "{headers}");
        answer(stream);
    });
    // Only the configured CN provider is selectable in this fixture; do not
    // inject a higher-priority Anthropic key with a non-fixture endpoint.
    let mut command = base_command(root.path());
    command.env("ZAI_CODING_CN_API_KEY", "ambient-test-key").arg("hello");
    let output = run_with_timeout(command);
    assert!(output.status.success(), "{}", text(&output));
    server.join().unwrap();
    assert!(text(&output).contains("ok"));
    assert!(!text(&output).contains("stored-test-key"));
}

#[test]
fn legacy_precedence_selects_loopback_anthropic_and_uses_stored_key() {
    let listener = listener();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = root();
    std::fs::write(
        root.path().join("config/auth.json"),
        r#"{"anthropic":{"type":"api_key","key":"stored-anthropic-key"}}"#,
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = accept(&listener).unwrap();
        let headers = read_headers(&mut stream);
        assert!(headers.starts_with("POST /v1/messages "), "{headers}");
        assert!(
            headers.to_ascii_lowercase().contains("x-api-key: stored-anthropic-key"),
            "{headers}"
        );
        assert!(!headers.to_ascii_lowercase().contains("ambient-anthropic-key"), "{headers}");
        answer_anthropic(stream);
    });
    let mut command = base_command(root.path());
    command
        .env("ANTHROPIC_API_KEY", "ambient-anthropic-key")
        .env("ANTHROPIC_BASE_URL", endpoint)
        .env("ZAI_CODING_CN_API_KEY", "lower-priority-cn-key")
        .arg("hello");
    let output = run_with_timeout(command);
    assert!(output.status.success(), "{}", text(&output));
    server.join().unwrap();
    assert!(text(&output).contains("ok"));
    assert!(!text(&output).contains("stored-anthropic-key"));
}

#[cfg(unix)]
#[test]
fn automatic_description_does_not_open_auth_or_connect() {
    use std::os::unix::fs::symlink;

    let listener = listener();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let root = root();
    configure_cn(root.path(), &endpoint);
    symlink("auth.json", root.path().join("config/auth.json")).unwrap();
    let mut command = base_command(root.path());
    command.env("ZAI_CODING_CN_API_KEY", "synthetic-key").arg("--describe");
    let output = run_with_timeout(command);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("zai-coding-cn"));
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn stored_oauth_blocks_ambient_fallback_for_automatic_explicit_and_settings_selection() {
    for selection in ["automatic", "explicit", "settings"] {
        let root = root();
        let listener = listener();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        configure_cn(root.path(), &endpoint);
        std::fs::write(
            root.path().join("config/auth.json"),
            r#"{"zai-coding-cn":{"type":"oauth","access":"synthetic-oauth","refresh":"synthetic-refresh"}}"#,
        )
        .unwrap();
        let path = root.path().join("config/auth.json");
        let auth_before = std::fs::read(&path).unwrap();
        let mut command = base_command(root.path());
        command.env("ZAI_CODING_CN_API_KEY", "ambient-test-key");
        match selection {
            "explicit" => {
                command.args(["--provider", "zai-coding-cn", "--model", "glm-5.3"]);
            }
            "settings" => {
                std::fs::write(
                    root.path().join("config/settings.json"),
                    json!({"defaultProvider":"zai-coding-cn","defaultModel":"glm-5.3"}).to_string(),
                )
                .unwrap();
            }
            _ => {}
        }
        command.arg("hello");
        let output = run_with_timeout(command);
        let message = text(&output);
        assert!(!output.status.success(), "{selection}: {message}");
        assert!(
            message.contains("stored OAuth refresh is not implemented"),
            "{selection}: {message}"
        );
        assert!(message.contains("no fallback attempted"), "{selection}: {message}");
        assert!(!message.contains("synthetic-oauth"), "{selection}: credential leaked");
        assert!(!message.contains("ambient-test-key"), "{selection}: credential leaked");
        assert_eq!(std::fs::read(&path).unwrap(), auth_before, "{selection}: auth mutated");
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "{selection}: made HTTP request"
        );
    }
}
