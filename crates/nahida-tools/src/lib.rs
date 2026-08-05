//! `nahida-tools` — the starting tool set.
//!
//! Three tools, chosen because together they close the loop: the agent can look
//! at the workspace, change it, and check its work. Everything else (grep, glob,
//! a staleness-checked edit, a gated `git push`) is an addition to a working
//! thing rather than a prerequisite.
//!
//! Every path goes through [`Sandbox`], because a path from the model is
//! untrusted input.

pub mod bash;
pub mod read;
pub mod sandbox;
pub mod write;

use std::sync::Arc;

use nahida_agent::Tool;

pub use bash::Bash;
pub use read::Read;
pub use sandbox::Sandbox;
pub use write::Write;

/// The default set, rooted at `sandbox`.
pub fn default_set(sandbox: &Sandbox) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Read::new(sandbox.clone())),
        Arc::new(Write::new(sandbox.clone())),
        Arc::new(Bash::new(sandbox.clone())),
    ]
}

/// Trim tool output to a character budget, keeping the head and the tail.
///
/// The tail matters: a failing test run puts the summary at the end, and a naive
/// head-only truncation throws away the one line worth reading.
pub fn clamp_output(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head_len = max * 2 / 3;
    let tail_len = max - head_len;

    let head: String = s.chars().take(head_len).collect();
    let tail: String = {
        let chars: Vec<char> = s.chars().collect();
        chars[chars.len() - tail_len..].iter().collect()
    };
    let dropped = s.chars().count() - max;

    format!("{head}\n\n[… {dropped} characters omitted …]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_is_untouched() {
        assert_eq!(clamp_output("hello", 100), "hello");
    }

    #[test]
    fn long_output_keeps_head_and_tail() {
        let s = format!("{}{}", "a".repeat(500), "ZTAIL");
        let out = clamp_output(&s, 100);
        assert!(out.starts_with("aaa"));
        assert!(out.ends_with("ZTAIL"), "tail was dropped: {out}");
        assert!(out.contains("characters omitted"));
    }
}
