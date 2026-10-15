//! Actual-binary exercise of the selected Pi-shaped GLM model metadata.
//! Uses synthetic config/credentials and loopback HTTP only.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}

fn fixture(level_map: &str, compat: &str) -> (tempfile::TempDir, TcpListener) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("workspace")).unwrap();
    std::fs::create_dir(root.path().join("config")).unwrap();
    let listener = listener();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let models = format!(
        r#"{{"providers":{{"zai-coding-cn":{{"api":"openai-completions","baseUrl":"{endpoint}","apiKey":"synthetic-key","models":[{{"id":"glm-5.3-flash","reasoning":true,"thinkingLevelMap":{level_map},"compat":{compat}}}]}}}}}}"#
    );
    std::fs::write(root.path().join("config/models.json"), models).unwrap();
    (root, listener)
}

fn selected_map() -> &'static str {
    r#"{"off":null,"minimal":null,"low":"low","medium":null,"high":"high","xhigh":null,"max":"max"}"#
}

fn compat() -> &'static str {
    r#"{"supportsStore":false,"supportsDeveloperRole":false,"supportsReasoningEffort":true,"maxTokensField":"max_tokens","thinkingFormat":"zai","supportsStrictMode":true,"zaiToolStream":true}"#
}

fn command(root: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nahida"));
    command
        .env_clear()
        .env("HOME", root)
        .env("XDG_DATA_HOME", root)
        .arg("--root")
        .arg(root.join("workspace"))
        .arg("--config-dir")
        .arg(root.join("config"))
        .args([
            "--provider",
            "zai-coding-cn",
            "--model",
            "glm-5.3-flash",
            "--no-session",
            "--max-retries",
            "0",
            "--json",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn output(root: &std::path::Path, args: &[&str]) -> Output {
    let mut child = command(root, args).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let result = child.wait_with_output().unwrap();
            panic!(
                "binary timeout: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "binary did not contact loopback fixture");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("loopback accept: {error}"),
        }
    }
}

fn request(listener: &TcpListener) -> (TcpStream, Value) {
    let mut stream = accept(listener);
    let mut bytes = Vec::new();
    let (body_start, length) = loop {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "request ended before headers");
        bytes.extend_from_slice(&chunk[..count]);
        let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else { continue };
        let headers = String::from_utf8_lossy(&bytes[..end]);
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        break (end + 4, length);
    };
    while bytes.len() < body_start + length {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "request ended before body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
    (stream, body)
}

fn reply(mut stream: TcpStream, events: &str) {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", events.len(), events).unwrap();
    stream.flush().unwrap();
}

fn readable(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_nullable_property(schema: &Value, name: &str, expected_type: &str) {
    let variants = schema["properties"][name]["anyOf"].as_array().unwrap();
    assert!(variants.iter().any(|variant| variant["type"] == expected_type));
    assert!(variants.iter().any(|variant| variant["type"] == "null"));
}

#[test]
fn actual_binary_strict_read_schema_keeps_optional_null_and_omitted_inputs_compatible() {
    for arguments in
        [json!({"path":"fixture.txt","offset":null,"limit":null}), json!({"path":"fixture.txt"})]
    {
        let (root, listener) = fixture(selected_map(), compat());
        std::fs::write(root.path().join("workspace/fixture.txt"), "first line\nsecond line\n")
            .unwrap();
        let server = std::thread::spawn(move || {
            let (stream, first) = request(&listener);
            let tools = first["tools"].as_array().unwrap();
            let read = &tools.iter().find(|tool| tool["function"]["name"] == "read").unwrap()["function"]
                ["parameters"];
            assert_eq!(read["properties"]["path"]["type"], "string");
            assert_eq!(read["required"].as_array().unwrap().len(), 3);
            assert!(read["required"].as_array().unwrap().contains(&json!("path")));
            assert!(read["required"].as_array().unwrap().contains(&json!("offset")));
            assert!(read["required"].as_array().unwrap().contains(&json!("limit")));
            assert_nullable_property(read, "offset", "integer");
            assert_nullable_property(read, "limit", "integer");

            let grep = &tools.iter().find(|tool| tool["function"]["name"] == "grep").unwrap()["function"]
                ["parameters"];
            assert_eq!(grep["properties"]["pattern"]["type"], "string");
            assert!(grep["required"].as_array().unwrap().contains(&json!("pattern")));
            for (name, kind) in [
                ("path", "string"),
                ("glob", "string"),
                ("ignore_case", "boolean"),
                ("literal", "boolean"),
                ("context", "integer"),
                ("limit", "integer"),
            ] {
                assert_nullable_property(grep, name, kind);
                assert!(grep["required"].as_array().unwrap().contains(&json!(name)));
            }

            let tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-read","function":{"name":"read","arguments":arguments.to_string()}}]},"finish_reason":"tool_calls"}]});
            reply(stream, &format!("data: {tool}\n\ndata: [DONE]\n\n"));
            let (stream, next) = request(&listener);
            let tool_result = next["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "tool")
                .unwrap();
            assert_eq!(tool_result["tool_call_id"], "call-read");
            assert!(tool_result["content"].as_str().unwrap().contains("first line"));
            assert!(tool_result["content"].as_str().unwrap().contains("second line"));
            let answer = json!({"choices":[{"delta":{"content":"Read completed with defaults."},"finish_reason":"stop"}]});
            reply(stream, &format!("data: {answer}\n\ndata: [DONE]\n\n"));
        });
        let result = output(root.path(), &["read fixture.txt"]);
        server.join().unwrap();
        assert!(result.status.success(), "{}", readable(&result));
        assert!(readable(&result).contains("Read completed with defaults."));
        let events: Vec<Value> = String::from_utf8_lossy(&result.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let tool_result = events
            .iter()
            .find(|event| event["type"] == "tool_result" && event["tool_use_id"] == "call-read")
            .unwrap();
        assert_eq!(tool_result["is_error"], false);
        assert!(tool_result["content"].as_str().unwrap().contains("first line"));
        assert!(tool_result["content"].as_str().unwrap().contains("second line"));
    }
}

#[test]
fn actual_binary_streams_tool_writes_exact_bytes_replays_reasoning_and_settles_final_answer() {
    let (root, listener) = fixture(selected_map(), compat());
    let server = std::thread::spawn(move || {
        let (stream, first) = request(&listener);
        assert_eq!(first["model"], "glm-5.3-flash");
        assert_eq!(first["max_tokens"], 32_000);
        assert!(first.get("max_completion_tokens").is_none());
        assert!(first.get("store").is_none());
        assert!(first.get("tool_stream").is_some());
        assert_eq!(first["thinking"], json!({"type":"enabled","clear_thinking":false}));
        assert_eq!(first["reasoning_effort"], "high");
        assert!(first["messages"].as_array().unwrap().iter().any(|m| m["role"] == "system"));
        let schema = first["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["function"]["name"] == "write")
            .unwrap();
        assert_eq!(schema["function"]["strict"], true);
        assert_eq!(schema["function"]["parameters"]["additionalProperties"], false);
        let required = schema["function"]["parameters"]["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&json!("path")));
        assert!(required.contains(&json!("content")));
        let tool = json!({"choices":[{"delta":{"reasoning_content":"planning ","tool_calls":[{"index":0,"id":"call-write","function":{"name":"write","arguments":"{\"path\":\"artifact.bin\",\"content\":\"Pi → Nahida\\n\"}"}}]},"finish_reason":null}]});
        let settled = json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]});
        reply(stream, &format!("data: {tool}\n\ndata: {settled}\n\ndata: [DONE]\n\n"));

        let (stream, second) = request(&listener);
        let assistant = second["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["reasoning_content"], "planning ");
        assert_eq!(assistant["tool_calls"][0]["id"], "call-write");
        assert!(
            second["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "tool" && m["tool_call_id"] == "call-write")
        );
        let answer =
            json!({"choices":[{"delta":{"content":"Artifact written."},"finish_reason":"stop"}]});
        reply(stream, &format!("data: {answer}\n\ndata: [DONE]\n\n"));
    });
    let result = output(root.path(), &["--effort", "high", "write artifact"]);
    server.join().unwrap();
    assert!(result.status.success(), "{}", readable(&result));
    assert!(readable(&result).contains("Artifact written."));
    assert_eq!(
        std::fs::read(root.path().join("workspace/artifact.bin")).unwrap(),
        "Pi → Nahida\n".as_bytes()
    );
}

#[test]
fn actual_binary_omits_effort_but_disables_zai_thinking_without_requesting_it() {
    let (root, listener) = fixture(selected_map(), compat());
    let server = std::thread::spawn(move || {
        let (stream, body) = request(&listener);
        assert_eq!(body["thinking"], json!({"type":"disabled"}));
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["tool_stream"], true);
        assert_ne!(body["tools"].as_array().unwrap().len(), 0);
        let answer = json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]});
        reply(stream, &format!("data: {answer}\n\ndata: [DONE]\n\n"));
    });
    let result = output(root.path(), &["hello"]);
    server.join().unwrap();
    assert!(result.status.success(), "{}", readable(&result));
}

#[test]
fn actual_binary_accepts_each_supported_mapped_effort() {
    for (level, mapped) in [("low", "low"), ("high", "high"), ("max", "max")] {
        let (root, listener) = fixture(selected_map(), compat());
        let server = std::thread::spawn(move || {
            let (stream, body) = request(&listener);
            assert_eq!(body["reasoning_effort"], mapped);
            let answer = json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]});
            reply(stream, &format!("data: {answer}\n\ndata: [DONE]\n\n"));
        });
        let result = output(root.path(), &["--effort", level, "hello"]);
        server.join().unwrap();
        assert!(result.status.success(), "{level}: {}", readable(&result));
    }
}

#[test]
fn actual_binary_rejects_explicit_null_effort_map_before_http() {
    let (root, listener) = fixture(selected_map(), compat());
    let result = output(root.path(), &["--effort", "medium", "hello"]);
    assert!(!result.status.success(), "{}", readable(&result));
    assert!(readable(&result).contains("unsupported by this model"));
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn actual_binary_provider_error_stop_does_not_write_or_report_success() {
    let (root, listener) = fixture(selected_map(), compat());
    let server = std::thread::spawn(move || {
        let (stream, _) = request(&listener);
        let arguments = json!({"path":"error.txt","content":"must not appear"}).to_string();
        let tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-write","function":{"name":"write","arguments":arguments}}]},"finish_reason":"tool_calls"}]});
        let error = json!({"error":{"message":"synthetic failure"}});
        reply(stream, &format!("data: {tool}\n\ndata: {error}\n\ndata: [DONE]\n\n"));
    });
    let result = output(root.path(), &["write error artifact"]);
    server.join().unwrap();
    assert!(!result.status.success(), "{}", readable(&result));
    assert!(!root.path().join("workspace/error.txt").exists());
    assert!(!readable(&result).contains("Done"));
}

#[test]
fn actual_binary_malformed_tool_json_does_not_write_or_report_success() {
    let (root, listener) = fixture(selected_map(), compat());
    let server = std::thread::spawn(move || {
        let (stream, _) = request(&listener);
        let tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-write","function":{"name":"write","arguments":"{not-json"}}]},"finish_reason":"tool_calls"}]});
        reply(stream, &format!("data: {tool}\n\ndata: [DONE]\n\n"));
    });
    let result = output(root.path(), &["write malformed artifact"]);
    server.join().unwrap();
    assert!(!result.status.success(), "{}", readable(&result));
    assert!(!root.path().join("workspace/malformed.txt").exists());
    assert!(!readable(&result).contains("Done"));
}

#[cfg(unix)]
#[test]
fn actual_binary_sigint_during_partial_tool_stream_never_dispatches() {
    use std::os::unix::process::ExitStatusExt as _;
    use std::sync::mpsc;

    let (root, listener) = fixture(selected_map(), compat());
    let (started_tx, started_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = request(&listener);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n").unwrap();
        let arguments = r#"{"path":"cancelled.txt","content":"must not"#;
        let partial = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-write","function":{"name":"write","arguments":arguments}}]}}]});
        let frame = format!("data: {partial}\n\n");
        write!(stream, "{:X}\r\n{}\r\n", frame.len(), frame).unwrap();
        stream.flush().unwrap();
        started_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let _ = stream.shutdown(std::net::Shutdown::Both);
    });

    let mut child = command(root.path(), &["write while cancelling"]).spawn().unwrap();
    started_rx
        .recv_timeout(Duration::from_secs(6))
        .expect("server did not stream partial tool arguments");
    let signal = Command::new("kill").args(["-INT", &child.id().to_string()]).status().unwrap();
    assert!(signal.success(), "SIGINT delivery failed: {signal}");

    let deadline = Instant::now() + Duration::from_secs(6);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            server.join().unwrap();
            panic!("binary did not exit after SIGINT within 6 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let result = child.wait_with_output().unwrap();
    server.join().unwrap();
    assert!(status.signal().is_none_or(|signal| signal != 9));
    let output = readable(&result);
    assert!(output.contains("interrupting"), "SIGINT handler did not run: {output}");
    assert!(!root.path().join("workspace/cancelled.txt").exists());
    assert!(!output.contains("\"type\":\"done\""), "cancelled turn reported success: {output}");
}

#[test]
fn actual_binary_truncated_tool_stream_does_not_write_or_report_success() {
    let (root, listener) = fixture(selected_map(), compat());
    let server = std::thread::spawn(move || {
        let (stream, _) = request(&listener);
        let tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-write","function":{"name":"write","arguments":"{\"path\":\"partial.txt\",\"content\":\"must not appear\"}"}}]},"finish_reason":"tool_calls"}]});
        // Intentionally omit [DONE]: transport settlement must fail before dispatch.
        reply(stream, &format!("data: {tool}\n\n"));
    });
    let result = output(root.path(), &["write partial artifact"]);
    server.join().unwrap();
    assert!(!result.status.success(), "{}", readable(&result));
    assert!(!root.path().join("workspace/partial.txt").exists());
    assert!(!readable(&result).contains("Done"));
}
