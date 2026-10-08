//! Exercise provider selection in fresh processes, never by mutating the test
//! runner's environment. All credentials are fake; normal runs see EOF on stdin
//! and --describe must not construct a client, contact an endpoint or log a session.

use std::process::{Command, Output, Stdio};

fn run(args: &[&str], env: &[(&str, &str)]) -> Output {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nahida"))
        .env_clear()
        .env("HOME", root.path())
        .env("XDG_DATA_HOME", root.path())
        .envs(env.iter().copied())
        .arg("--root")
        .arg(root.path())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0, "created session/auth files");
    output
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn explicit_cn_wins_in_description_and_actual_resolution() {
    let env = [
        ("ANTHROPIC_API_KEY", "fake-anthropic-secret"),
        ("ZAI_API_KEY", "fake-global-secret"),
        ("ZAI_CODING_CN_API_KEY", "fake-cn-secret"),
    ];
    for mode in ["--describe", "--verbose"] {
        let output = run(&["--provider", "zai-coding-cn", "--no-session", mode], &env);
        let text = text(&output);
        assert!(output.status.success(), "{text}");
        assert!(text.contains("zai-coding-cn"), "{text}");
        assert!(text.contains("glm-5.3"), "{text}");
        for (_, secret) in env {
            assert!(!text.contains(secret), "{text}");
        }
    }
}

#[test]
fn omission_keeps_auto_precedence_and_ignores_empty_keys() {
    for (env, expected) in [
        (vec![("ANTHROPIC_API_KEY", "fake"), ("ZAI_CODING_CN_API_KEY", "fake")], "anthropic"),
        (vec![("ANTHROPIC_AUTH_TOKEN", "fake"), ("ZAI_API_KEY", "fake")], "anthropic"),
        (vec![("ZAI_API_KEY", "fake"), ("ZAI_CODING_CN_API_KEY", "fake")], "zai ("),
        (
            vec![("ANTHROPIC_API_KEY", ""), ("ZAI_API_KEY", ""), ("ZAI_CODING_CN_API_KEY", "fake")],
            "zai-coding-cn",
        ),
    ] {
        let output = run(&["--describe", "--no-session"], &env);
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains(expected), "{}", text(&output));
    }
}

#[test]
fn missing_explicit_key_does_not_fall_back() {
    let output = run(
        &["--provider", "zai-coding-cn", "--no-session"],
        &[("ANTHROPIC_API_KEY", "fake-secret")],
    );
    let text = text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ZAI_CODING_CN_API_KEY"), "{text}");
    assert!(text.contains("no fallback"), "{text}");
    assert!(!text.contains("fake-secret"), "{text}");
}

#[test]
fn named_description_works_without_credentials() {
    for (name, model) in
        [("anthropic", "claude-opus-5"), ("zai", "glm-5.1"), ("zai-coding-cn", "glm-5.3")]
    {
        let output = run(&["--provider", name, "--describe"], &[]);
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains(name), "{}", text(&output));
        assert!(text(&output).contains(model), "{}", text(&output));
    }
    let output = run(&["--describe"], &[]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("provider      (unresolved:"), "{}", text(&output));
}

#[test]
fn description_never_connects_or_echoes_endpoint_secrets() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint =
        format!("http://127.0.0.1:{}/endpoint-secret", listener.local_addr().unwrap().port());
    let output = run(
        &["--provider", "anthropic", "--describe"],
        &[("ANTHROPIC_BASE_URL", &endpoint), ("ANTHROPIC_AUTH_TOKEN", "fake-token-secret")],
    );
    let text = text(&output);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("endpoint-secret"), "{text}");
    assert!(!text.contains("fake-token-secret"), "{text}");
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn chatgpt_is_unavailable_even_with_other_provider_keys() {
    let env = [("ANTHROPIC_API_KEY", "fake-secret"), ("OPENAI_API_KEY", "fake-openai-secret")];
    for describe in [false, true] {
        let args = if describe {
            vec!["--provider", "chatgpt", "--describe"]
        } else {
            vec!["--provider", "chatgpt", "--no-session"]
        };
        let output = run(&args, &env);
        let text = text(&output);
        assert_eq!(output.status.success(), describe, "{text}");
        assert!(text.contains("official sign-in support is not implemented"), "{text}");
        assert!(!text.contains("fake-secret"), "{text}");
        assert!(!text.contains("fake-openai-secret"), "{text}");
    }
}

#[test]
fn normal_eof_run_still_creates_its_session_directory_and_log() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nahida"))
        .env_clear()
        .env("HOME", root.path())
        .env("XDG_DATA_HOME", root.path())
        .env("ANTHROPIC_API_KEY", "fake-key")
        .args(["--provider", "anthropic", "--root"])
        .arg(root.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let logs: Vec<_> = std::fs::read_dir(root.path().join("nahida/sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].extension().unwrap(), "jsonl");
}

#[test]
fn unknown_provider_is_rejected_and_json_description_stays_on_stderr() {
    let bad = run(&["--provider", "typo", "--describe"], &[]);
    assert!(!bad.status.success(), "{}", text(&bad));
    let output = run(&["--provider", "zai-coding-cn", "--describe", "--json"], &[]);
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert!(String::from_utf8_lossy(&output.stderr).contains("zai-coding-cn"));
}
