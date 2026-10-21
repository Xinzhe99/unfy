//! Activity log.
//!
//! # Why a product needs this
//!
//! This app moves other applications' windows. When something ends up in the
//! wrong place, the user's first question is "what did it do, and when?" — and
//! without an answer the app is untrustworthy no matter how correct it is. The
//! log also explains the difference between "the app did nothing" and "the app
//! tried and an application refused", which are indistinguishable from a
//! screenshot.
//!
//! # Storage
//!
//! Append-only JSONL, capped by rewriting the file when it grows past
//! [`MAX_ENTRIES`]. A single small file per user, written after the action has
//! already completed: losing the last line to a crash costs nothing, because the
//! journal is the data that actually matters.
//!
//! Entries never contain window titles or file paths. Application names appear
//! only as the count of refusals, and only executable base names.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many entries are kept. Two hundred is several weeks of normal use and
/// keeps the file under ~40 kB.
pub const MAX_ENTRIES: usize = 200;

/// What happened. Kept as a small closed set so the UI can label each row
/// without parsing free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Gather,
    Restore,
    Rescue,
    /// An automatic gather triggered by the detector.
    AutoGather,
    /// An automatic restore triggered by the detector.
    AutoRestore,
    /// The user paused or resumed automation.
    Pause,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Gather => "gather",
            Kind::Restore => "restore",
            Kind::Rescue => "rescue",
            Kind::AutoGather => "auto_gather",
            Kind::AutoRestore => "auto_restore",
            Kind::Pause => "pause",
        }
    }
}

/// One completed action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub at: u64,
    pub kind: Kind,
    /// Windows actually moved (gather) or put back (restore).
    pub affected: usize,
    /// Windows that ended up unhandled: refused by the app, still pending.
    pub unsettled: usize,
    /// Why, when something went wrong. Empty on success.
    #[serde(default)]
    pub note: String,
    /// Which trigger caused it: "hotkey", "tray", "window", "auto".
    #[serde(default)]
    pub trigger: String,
}

/// In-memory view plus the file to append to.
#[derive(Debug)]
pub struct Activity {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Activity {
    /// Load existing entries. A missing or unreadable file yields an empty log
    /// rather than an error: history is a convenience, never a prerequisite.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let entries = read_entries(&path);
        Self { path, entries }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Append one entry, newest last, and persist.
    pub fn record(&mut self, entry: Entry) {
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
            // Rewrite rather than append: the cap has to be enforced on disk too,
            // otherwise the file grows without bound while the UI shows 200 rows.
            if let Err(e) = self.rewrite() {
                tracing::warn!(error = %e, "could not compact the activity log");
            }
            return;
        }
        if let Err(e) = self.append(self.entries.last().expect("just pushed")) {
            tracing::warn!(error = %e, "could not append to the activity log");
        }
    }

    fn append(&self, entry: &Entry) -> Result<(), std::io::Error> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(entry)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }

    fn rewrite(&self) -> Result<(), std::io::Error> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("jsonl.tmp");
        let mut text = String::new();
        for entry in &self.entries {
            text.push_str(&serde_json::to_string(entry)?);
            text.push('\n');
        }
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        drop(file);
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

/// Parse the file, skipping lines that do not decode.
///
/// A half-written final line is normal after a crash; discarding it is correct,
/// and discarding the whole file over one bad line would lose real history.
fn read_entries(path: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if entries.len() > MAX_ENTRIES {
        let excess = entries.len() - MAX_ENTRIES;
        entries.drain(..excess);
    }
    entries
}

/// Current time in seconds since the epoch, or 0 if the clock is before 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "conflux-activity-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("activity.jsonl")
    }

    fn entry(affected: usize) -> Entry {
        Entry {
            at: 1,
            kind: Kind::Gather,
            affected,
            unsettled: 0,
            note: String::new(),
            trigger: "window".into(),
        }
    }

    #[test]
    fn entries_survive_a_reload() {
        let path = temp_path("reload");
        let mut activity = Activity::load(&path);
        activity.record(entry(3));
        activity.record(Entry {
            kind: Kind::Restore,
            ..entry(3)
        });

        let reloaded = Activity::load(&path);
        assert_eq!(reloaded.entries().len(), 2);
        assert_eq!(reloaded.entries()[0].kind, Kind::Gather);
        assert_eq!(reloaded.entries()[1].kind, Kind::Restore);
    }

    #[test]
    fn missing_file_is_empty_not_an_error() {
        let activity = Activity::load(temp_path("missing"));
        assert!(activity.entries().is_empty());
    }

    #[test]
    fn a_truncated_line_is_skipped_without_losing_the_rest() {
        let path = temp_path("truncated");
        let mut activity = Activity::load(&path);
        activity.record(entry(1));
        activity.record(entry(2));

        // Simulate a crash mid-write: append a partial JSON object.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{\"at\":3,\"kind\":\"ga");
        std::fs::write(&path, text).unwrap();

        let reloaded = Activity::load(&path);
        assert_eq!(reloaded.entries().len(), 2);
    }

    #[test]
    fn the_file_is_capped_at_the_entry_limit() {
        let path = temp_path("cap");
        let mut activity = Activity::load(&path);
        for i in 0..(MAX_ENTRIES + 5) {
            activity.record(entry(i));
        }

        assert_eq!(activity.entries().len(), MAX_ENTRIES);
        let reloaded = Activity::load(&path);
        assert_eq!(
            reloaded.entries().len(),
            MAX_ENTRIES,
            "the cap must be enforced on disk, not only in memory"
        );
        // The oldest entries are the ones dropped.
        assert_eq!(reloaded.entries()[0].affected, 5);
    }

    #[test]
    fn entries_carry_no_window_titles() {
        // The struct has no field for one; this test documents the guarantee so a
        // future field addition has to think about it.
        let json = serde_json::to_string(&entry(2)).unwrap();
        assert!(!json.contains("title"));
    }
}
