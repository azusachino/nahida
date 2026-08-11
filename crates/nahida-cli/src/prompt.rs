//! `@file` expansion in prompts.
//!
//! A prompt can reference a file directly with `@path/to/file`; the token is
//! replaced with the file's contents before the prompt reaches the model.
//! A token that doesn't resolve to a real file under the workspace root is
//! left untouched, so an `@` in ordinary prose (an email address, a mention)
//! is not disturbed.

use nahida_tools::Sandbox;

pub fn expand_file_refs(prompt: &str, sandbox: &Sandbox) -> String {
    prompt
        .split_whitespace()
        .map(|token| expand_token(token, sandbox))
        .collect::<Vec<_>>()
        .join(" ")
}

fn expand_token(token: &str, sandbox: &Sandbox) -> String {
    let Some(candidate) = token.strip_prefix('@') else {
        return token.to_string();
    };
    let Ok(resolved) = sandbox.resolve(candidate) else {
        return token.to_string();
    };
    std::fs::read_to_string(&resolved).unwrap_or_else(|_| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sb = Sandbox::new(dir.path()).expect("sandbox");
        (dir, sb)
    }

    #[test]
    fn expands_a_resolving_file_reference() {
        let (dir, sb) = sandbox();
        std::fs::write(dir.path().join("notes.md"), "some notes").expect("seed");

        let out = expand_file_refs("summarize @notes.md please", &sb);

        assert_eq!(out, "summarize some notes please");
    }

    #[test]
    fn leaves_a_non_resolving_reference_untouched() {
        let (_dir, sb) = sandbox();

        let out = expand_file_refs("email me at user@example.com", &sb);

        assert_eq!(out, "email me at user@example.com");
    }

    #[test]
    fn leaves_a_missing_file_untouched() {
        let (_dir, sb) = sandbox();

        let out = expand_file_refs("read @nope.txt", &sb);

        assert_eq!(out, "read @nope.txt");
    }
}
