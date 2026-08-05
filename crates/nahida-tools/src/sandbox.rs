//! Path confinement.
//!
//! Every path a tool receives is model output, which means it is untrusted the
//! same way user input is. A file tool that opens whatever string it is handed
//! will happily read `~/.ssh/id_ed25519` the first time the model guesses that
//! path — so resolution goes through here, and `..` is rejected outright rather
//! than normalised.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// `root` must already exist; it is canonicalised once so later comparisons
    /// are against a real path rather than a symlink alias.
    pub fn new(root: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self { root: root.as_ref().canonicalize()? })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a model-supplied path, or explain why it was refused.
    ///
    /// The target need not exist — `write` creates files — so we canonicalise
    /// the deepest existing ancestor and check *that*, then re-attach the tail.
    /// Canonicalising only the parent would miss a symlinked grandparent.
    pub fn resolve(&self, candidate: &str) -> Result<PathBuf, String> {
        if candidate.trim().is_empty() {
            return Err("path must not be empty".to_string());
        }

        let raw = Path::new(candidate);

        // Reject rather than normalise: `a/../../etc/passwd` collapses to
        // something outside the root, and a symlink in the middle makes textual
        // normalisation unsound anyway.
        if raw.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(format!(
                "path `{candidate}` contains `..`; use a path relative to {}",
                self.root.display()
            ));
        }

        let joined = if raw.is_absolute() { raw.to_path_buf() } else { self.root.join(raw) };

        let mut existing: &Path = &joined;
        loop {
            if existing.exists() {
                break;
            }
            match existing.parent() {
                Some(parent) => existing = parent,
                None => return Err(format!("path `{candidate}` has no reachable ancestor")),
            }
        }

        let real = existing
            .canonicalize()
            .map_err(|e| format!("cannot resolve `{}`: {e}", existing.display()))?;

        if !real.starts_with(&self.root) {
            return Err(format!(
                "path `{candidate}` resolves outside the workspace root {}",
                self.root.display()
            ));
        }

        let tail = joined.strip_prefix(existing).unwrap_or(Path::new(""));
        Ok(real.join(tail))
    }

    /// A path as it should appear in a message back to the model: relative to the
    /// root, so the transcript does not leak the machine's directory layout.
    pub fn display(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).display().to_string()
    }
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
    fn accepts_a_relative_path_inside_the_root() {
        let (_dir, sb) = sandbox();
        let p = sb.resolve("src/main.rs").expect("resolves");
        assert!(p.starts_with(sb.root()));
        assert_eq!(sb.display(&p), "src/main.rs");
    }

    #[test]
    fn rejects_parent_traversal() {
        let (_dir, sb) = sandbox();
        let err = sb.resolve("../../etc/passwd").expect_err("must refuse");
        assert!(err.contains(".."), "got {err}");
    }

    #[test]
    fn rejects_an_absolute_path_outside_the_root() {
        let (_dir, sb) = sandbox();
        let err = sb.resolve("/etc/hosts").expect_err("must refuse");
        assert!(err.contains("outside the workspace root"), "got {err}");
    }

    #[test]
    fn rejects_empty_path() {
        let (_dir, sb) = sandbox();
        assert!(sb.resolve("   ").is_err());
    }
}
