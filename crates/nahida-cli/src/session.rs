//! JSONL session persistence: `--continue` / `--resume`.
//!
//! One writer, one linear transcript — unlike a multi-session-writer log,
//! there is no concurrent access to guard against, so this is a plain
//! append-only file: a header line, then one line per [`Message`] as it's
//! added to the transcript. `Agent::run` already mutates `transcript` in
//! place so an interrupted run stays resumable in memory; this just mirrors
//! that same growing list to disk.

use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use nahida_llm::Message;

/// The `Line::Header` shape below. Bumping this is a breaking change to every
/// session already on disk — `nahida --describe` reports it precisely so that
/// is visible before it bites someone resuming an old session.
pub const SESSION_FORMAT_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Line {
    Header { version: u32, id: String, created_at: u64, cwd: String },
    Message { message: Message },
}

pub struct SessionStore {
    file: std::fs::File,
    pub id: String,
}

impl SessionStore {
    /// `$XDG_DATA_HOME/nahida/sessions`, falling back to `~/.local/share`.
    /// Dependency-free rather than pulling in a `dirs` crate for one lookup.
    pub fn sessions_dir() -> Result<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        });
        let base = base.context("cannot find a data directory: set $HOME or $XDG_DATA_HOME")?;
        let dir = base.join("nahida/sessions");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create sessions directory `{}`", dir.display()))?;
        Ok(dir)
    }

    /// Start a fresh session log rooted at `cwd`.
    pub fn create(sessions_dir: &Path, cwd: &Path) -> Result<Self> {
        let id = now_millis().to_string();
        let path = sessions_dir.join(format!("{id}.jsonl"));
        // `create_new` so a fresh file always starts clean; a millisecond-id
        // collision from one user's own process is not worth guarding harder.
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("cannot create session `{}`", path.display()))?;

        let header = Line::Header {
            version: SESSION_FORMAT_VERSION,
            id: id.clone(),
            created_at: now_millis(),
            cwd: display(cwd),
        };
        writeln!(file, "{}", serde_json::to_string(&header)?)?;

        Ok(Self { file, id })
    }

    /// Resume a specific session (`id` given) or the most recent one whose
    /// header `cwd` matches `cwd` (`id` omitted). Returns the replayed
    /// transcript alongside the still-open, append-ready store.
    pub fn resume(
        sessions_dir: &Path,
        cwd: &Path,
        id: Option<&str>,
    ) -> Result<(Self, Vec<Message>)> {
        let path = match id {
            Some(id) => sessions_dir.join(format!("{id}.jsonl")),
            None => most_recent_for(sessions_dir, cwd)?.with_context(|| {
                format!("no session found for `{}` — omit --continue to start one", display(cwd))
            })?,
        };

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read session `{}`", path.display()))?;
        let mut lines = content.lines();

        let header_line =
            lines.next().with_context(|| format!("session `{}` is empty", path.display()))?;
        let Line::Header { id: session_id, .. } = serde_json::from_str(header_line)
            .with_context(|| format!("session `{}` has a corrupt header", path.display()))?
        else {
            anyhow::bail!("session `{}`'s first line is not a header", path.display());
        };

        let remaining: Vec<&str> = lines.collect();
        let mut transcript = Vec::new();
        for (i, line) in remaining.iter().enumerate() {
            match serde_json::from_str::<Line>(line) {
                Ok(Line::Message { message }) => transcript.push(message),
                Ok(Line::Header { .. }) => {}
                // A crash mid-write can leave a torn final line; drop it
                // rather than refusing to resume an otherwise-good session.
                Err(e) if i == remaining.len() - 1 => {
                    eprintln!(
                        "warning: dropping a truncated last line in session `{session_id}`: {e}"
                    );
                }
                Err(e) => {
                    anyhow::bail!("session `{}` is corrupt at line {}: {e}", path.display(), i + 2);
                }
            }
        }

        // Reads happened through `read_to_string` above; this handle is only
        // for appending from here on, which is why `O_APPEND` (not the read
        // cursor) is what matters.
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .with_context(|| format!("cannot reopen session `{}` for appending", path.display()))?;

        Ok((Self { file, id: session_id }, transcript))
    }

    pub fn append(&mut self, message: &Message) -> Result<()> {
        let line = Line::Message { message: message.clone() };
        writeln!(self.file, "{}", serde_json::to_string(&line)?)?;
        self.file.flush()?;
        Ok(())
    }
}

fn most_recent_for(sessions_dir: &Path, cwd: &Path) -> Result<Option<PathBuf>> {
    let cwd = display(cwd);
    let mut best: Option<(u64, PathBuf)> = None;

    for entry in std::fs::read_dir(sessions_dir)
        .with_context(|| format!("cannot list sessions directory `{}`", sessions_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else { continue };
        let Some(Ok(first_line)) = std::io::BufReader::new(file).lines().next() else { continue };
        let Ok(Line::Header { created_at, cwd: header_cwd, .. }) =
            serde_json::from_str(&first_line)
        else {
            continue;
        };
        if header_cwd != cwd {
            continue;
        }
        if best.as_ref().is_none_or(|(t, _)| created_at > *t) {
            best = Some((created_at, path));
        }
    }

    Ok(best.map(|(_, p)| p))
}

fn now_millis() -> u64 {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn display(p: &Path) -> String {
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/workspace")
    }

    #[test]
    fn round_trips_create_append_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = SessionStore::create(dir.path(), &cwd()).expect("create");
        let id = store.id.clone();

        let user = Message::user(vec![nahida_llm::ContentBlock::text("hi")]);
        let reply = Message::assistant(vec![nahida_llm::ContentBlock::text("hello")]);
        store.append(&user).expect("append user");
        store.append(&reply).expect("append reply");
        drop(store);

        let (resumed, transcript) =
            SessionStore::resume(dir.path(), &cwd(), Some(&id)).expect("resume");

        assert_eq!(resumed.id, id);
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].content.len(), 1);
    }

    #[test]
    fn continue_picks_the_most_recent_session_for_the_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        let other_cwd = PathBuf::from("/elsewhere");

        let mut older = SessionStore::create(dir.path(), &other_cwd).expect("create older");
        older.append(&Message::user(vec![])).expect("append");
        std::thread::sleep(std::time::Duration::from_millis(2));

        let mut newer = SessionStore::create(dir.path(), &cwd()).expect("create newer");
        newer.append(&Message::user(vec![])).expect("append");
        let newer_id = newer.id.clone();

        let (resumed, _) = SessionStore::resume(dir.path(), &cwd(), None).expect("resume");

        assert_eq!(resumed.id, newer_id);
    }

    #[test]
    fn tolerates_a_truncated_last_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = SessionStore::create(dir.path(), &cwd()).expect("create");
        let id = store.id.clone();
        store.append(&Message::user(vec![nahida_llm::ContentBlock::text("hi")])).expect("append");
        drop(store);

        // Simulate a crash mid-write: append a partial JSON line with no
        // trailing newline.
        let path = dir.path().join(format!("{id}.jsonl"));
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).expect("reopen");
        write!(file, "{{\"kind\":\"message\",\"mess").expect("write partial");

        let (_, transcript) = SessionStore::resume(dir.path(), &cwd(), Some(&id)).expect("resume");

        assert_eq!(transcript.len(), 1);
    }
}
