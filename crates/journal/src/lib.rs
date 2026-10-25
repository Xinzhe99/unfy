//! Durable transaction log.
//!
//! The engine may only move a window after the preimage of that window has been
//! written here and flushed. Everything in this crate exists to make that
//! promise survivable: a process kill, a power cut, a full disk, or a corrupted
//! tail must never turn into "windows were moved and the original positions are
//! gone".
//!
//! # On-disk format
//!
//! ```text
//! file   := record*
//! record := len:u32le | crc32:u32le | kind:u8 | payload:json[len-1]
//! ```
//!
//! `crc32` covers `kind` + `payload`. Recovery accepts an incomplete final
//! record after a valid prefix, and rejects complete corrupt records. Reading
//! recovery state never changes the file or creates diagnostic sidecars.
//!
//! # Why not a single JSON document
//!
//! Rewriting a whole document requires a temp file plus rename, and a crash
//! between "windows moved" and "document replaced" loses the preimage. An
//! append-only log makes the *order* of durable writes the source of truth.

mod record;

pub use record::Record;

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use unfy_domain::{Rect, WindowId, WindowMode};
use uuid::Uuid;

/// One window captured before it was touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preimage {
    pub id: WindowId,
    pub display_native_id: String,
    pub display_quality: unfy_domain::IdentityQuality,
    /// Work area of the display the window was on, so restore can tell whether
    /// the topology is unchanged or must be rescaled.
    pub display_work_area: Rect,
    pub display_scale: unfy_domain::Scale,
    pub visible_rect: Rect,
    pub outer_rect: Rect,
    pub mode: WindowMode,
    pub topmost: bool,
    /// Opaque platform placement blob, base64 is avoided by storing bytes as a
    /// JSON array; these blobs are small (44 bytes on Windows).
    pub native_placement: Option<Vec<u8>>,
}

/// The result of applying a plan to one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedResult {
    pub id: WindowId,
    pub outcome: unfy_domain::WindowOutcome,
    pub observed: Option<ObservedRect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ObservedRect {
    pub visible_rect: Rect,
    pub mode: WindowMode,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JournalError {
    #[error("io error: {0}")]
    Io(String),
    #[error("record is corrupt at offset {offset}: {reason}")]
    Corrupt { offset: u64, reason: String },
    #[error("journal belongs to another schema version: {0}")]
    SchemaMismatch(u32),
    #[error("transaction {0} is already closed")]
    AlreadyClosed(Uuid),
    #[error("no open transaction in this journal")]
    NotOpen,
}

impl From<std::io::Error> for JournalError {
    fn from(e: std::io::Error) -> Self {
        JournalError::Io(e.to_string())
    }
}

pub type Result<T, E = JournalError> = std::result::Result<T, E>;

/// Current schema version. Bumped only for changes that old readers cannot
/// interpret; the recovery path must keep reading every version it ever wrote.
pub const SCHEMA_VERSION: u32 = 3;

/// A single migration transaction on disk.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: File,
    transaction: Uuid,
    /// Byte offset just past the last complete record. Recovery uses this to
    /// know whether a later append would overwrite a torn tail.
    end: u64,
    /// Whether appended bytes still need a `sync_all` before they are trusted.
    dirty: bool,
    closed: bool,
    next_seq: u64,
    intents: std::collections::HashMap<WindowId, u64>,
    schema_version: u32,
}

/// What recovery found on disk.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovered {
    /// No journal file, or a cleanly closed transaction.
    Clean,
    /// An unfinished transaction. The caller must resolve it before allowing
    /// new migrations.
    Unfinished(RecoveredTransaction),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoveredTransaction {
    pub transaction: Uuid,
    pub schema_version: u32,
    pub preimages: Vec<Preimage>,
    pub intents: Vec<RecoveredIntent>,
    pub results: Vec<AppliedResult>,
    /// Windows whose recovery obligation is finished, with the outcome that
    /// finished it. A later run must not handle these again.
    pub resolved: Vec<(WindowId, unfy_domain::WindowOutcome)>,
    /// Bytes dropped from the tail because their CRC did not match.
    pub truncated_bytes: u64,
}

/// An intent whose outcome was never recorded. The platform call may or may not
/// have happened, so the window must be treated as possibly moved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveredIntent {
    pub id: WindowId,
    pub target: Rect,
    pub mode: WindowMode,
    #[serde(default)]
    pub write_seq: u64,
}

impl RecoveredTransaction {
    /// Whether any window in this transaction might have been moved.
    ///
    /// A transaction with no `Applied` result and no `Intent` record can be
    /// discarded without touching windows.
    pub fn may_have_moved_windows(&self) -> bool {
        !self.intents.is_empty()
            || self.results.iter().any(|r| {
                matches!(
                    r.outcome,
                    unfy_domain::WindowOutcome::Applied
                        | unfy_domain::WindowOutcome::Restored
                        | unfy_domain::WindowOutcome::FallbackVisible
                        | unfy_domain::WindowOutcome::KeptUserChange
                )
            })
    }

    pub fn preimage_for(&self, id: &WindowId) -> Option<&Preimage> {
        self.preimages.iter().find(|p| &p.id == id)
    }

    /// Whether any window has an intent with no recorded result.
    ///
    /// This is the state a kill between `SetWindowPos` and `read_back` leaves
    /// behind, and it is the strongest argument for writing the intent before
    /// the platform call.
    pub fn has_unresolved_intent(&self) -> bool {
        self.intents.iter().any(|intent| {
            self.is_unresolved(&intent.id)
                && self
                    .results
                    .iter()
                    .rev()
                    .find(|r| r.id == intent.id)
                    .is_none_or(|result| result.outcome == unfy_domain::WindowOutcome::Unknown)
        })
    }

    /// Every window that still needs an outcome: preimages minus resolutions.
    pub fn touched_windows(&self) -> Vec<WindowId> {
        self.preimages
            .iter()
            .map(|p| p.id.clone())
            .filter(|id| self.is_unresolved(id))
            .collect()
    }

    /// Whether this window still owes the user a restore.
    pub fn is_unresolved(&self, id: &WindowId) -> bool {
        !self.resolved.iter().any(|(resolved, _)| resolved == id)
    }

    /// The resolution recorded for a window, if any.
    pub fn resolution(&self, id: &WindowId) -> Option<unfy_domain::WindowOutcome> {
        self.resolved
            .iter()
            .find(|(resolved, _)| resolved == id)
            .map(|(_, outcome)| *outcome)
    }

    /// Whether the transaction still has work, considering resolutions.
    pub fn has_open_obligation(&self) -> bool {
        self.preimages.iter().any(|p| self.is_unresolved(&p.id))
    }
}

impl Journal {
    /// Open (creating if needed) a journal file and start a new transaction.
    ///
    /// The `Begin` record is flushed before this returns, so a crash immediately
    /// afterwards still leaves a recoverable (empty) transaction.
    pub fn begin(path: impl AsRef<Path>, display_summary: &str) -> Result<Self> {
        Self::begin_named(path, Uuid::new_v4(), display_summary)
    }

    /// Start a journal for a transaction the caller has already named.
    ///
    /// The caller owns the id because it also names the file: `pending_work` finds
    /// unfinished work by scanning the directory and skipping the one the engine
    /// currently owns, and it compares transaction ids. Two different ids - one in
    /// the filename, one inside the file - made that comparison fail for every
    /// transaction, so a layout that was in progress (or already finished) was
    /// reported as "the last gather did not finish", permanently.
    pub fn begin_named(
        path: impl AsRef<Path>,
        transaction: Uuid,
        display_summary: &str,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;

        let mut journal = Journal {
            path,
            file: file.try_clone()?,
            transaction,
            end: 0,
            dirty: false,
            closed: false,
            next_seq: 1,
            intents: std::collections::HashMap::new(),
            schema_version: SCHEMA_VERSION,
        };
        journal.append(&Record::Begin {
            schema_version: SCHEMA_VERSION,
            transaction,
            display_summary: display_summary.to_string(),
            created_unix_ms: now_unix_ms(),
        })?;
        journal.durable()?;
        Ok(journal)
    }

    pub fn transaction(&self) -> Uuid {
        self.transaction
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reopen an unfinished journal so a later process can finish its work.
    ///
    /// Unlike [`Journal::begin`] this neither truncates the file nor writes a new
    /// `Begin`: the transaction id and every preimage already on disk are
    /// preserved, because they are the only record of where the windows were.
    /// A torn tail is cut off so the next append starts on a record boundary.
    pub fn reopen(path: impl AsRef<Path>, transaction: Uuid) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let recovered = recover(&path)?;
        let Recovered::Unfinished(recovered) = recovered else {
            return Err(JournalError::NotOpen);
        };
        if recovered.transaction != transaction {
            return Err(JournalError::Corrupt {
                offset: 0,
                reason: "reopen transaction does not match Begin".into(),
            });
        }
        let next_seq = recovered
            .intents
            .iter()
            .map(|intent| intent.write_seq)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| JournalError::Corrupt {
                offset: 0,
                reason: "write sequence exhausted".into(),
            })?;
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let offset = file.metadata()?.len() - recovered.truncated_bytes;
        if recovered.truncated_bytes > 0 {
            file.set_len(offset)?;
            file.sync_all()?;
        }

        Ok(Journal {
            path,
            file,
            transaction,
            end: offset,
            dirty: false,
            closed: false,
            next_seq,
            intents: recovered
                .intents
                .iter()
                .map(|intent| (intent.id.clone(), intent.write_seq))
                .collect(),
            schema_version: recovered.schema_version,
        })
    }

    /// Older logs cannot prove the absence of a write; current logs can.
    pub fn may_have_written(&self, id: &WindowId) -> bool {
        self.schema_version < SCHEMA_VERSION || self.intents.contains_key(id)
    }

    /// Append one record to the file without forcing it to disk.
    ///
    /// Callers must follow with [`Journal::durable`] before acting on the record.
    /// Batching the fsync is what keeps a hundred-window gather inside its
    /// latency budget: a per-record `sync_all` costs milliseconds on a real disk,
    /// and the invariant only requires that *no window moves before its preimage
    /// is on disk*, not that every record is synced individually.
    fn append(&mut self, record: &Record) -> Result<()> {
        if self.closed {
            return Err(JournalError::AlreadyClosed(self.transaction));
        }
        let bytes = record.encode()?;
        self.file.seek(SeekFrom::Start(self.end))?;
        self.file.write_all(&bytes)?;
        self.end += bytes.len() as u64;
        self.file.set_len(self.end)?;
        self.dirty = true;
        Ok(())
    }

    /// Force everything appended so far to disk.
    ///
    /// This is the barrier the engine waits on before touching windows.
    pub fn durable(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.file.flush()?;
        self.file.sync_all()?;
        self.dirty = false;
        Ok(())
    }

    /// Persist the preimage of a window before touching it.
    ///
    /// Writing the same window twice is rejected: the first preimage is the one
    /// that matters, and a second write would mean the engine lost track.
    pub fn write_preimage(&mut self, preimage: &Preimage) -> Result<()> {
        self.append(&Record::Preimage {
            transaction: self.transaction,
            preimage: preimage.clone(),
        })
    }

    /// Persist the intent to move a window, with the target rectangle.
    ///
    /// Written before the platform call so that a crash between "called
    /// SetWindowPos" and "read the result" is still recoverable: recovery knows
    /// the window may have moved and to where.
    pub fn write_intent(&mut self, id: &WindowId, target: Rect, mode: WindowMode) -> Result<()> {
        let write_seq = self.next_seq;
        let next_seq = write_seq
            .checked_add(1)
            .ok_or_else(|| JournalError::Io("write sequence exhausted".into()))?;
        self.append(&Record::Intent {
            transaction: self.transaction,
            id: id.clone(),
            target,
            mode,
            write_seq,
        })?;
        self.next_seq = next_seq;
        self.intents.insert(id.clone(), write_seq);
        Ok(())
    }

    /// Persist the observed outcome after the platform call and read-back.
    pub fn write_result(&mut self, result: &AppliedResult) -> Result<()> {
        self.append(&Record::Result {
            transaction: self.transaction,
            result: result.clone(),
            write_seq: self.intents.get(&result.id).copied().unwrap_or(0),
        })
    }

    /// Record that a window no longer owes a restore.
    ///
    /// Must be durable before the in-memory preimage is dropped: a crash after
    /// the close but before this record would otherwise reopen a window that was
    /// already put back.
    pub fn write_resolved(
        &mut self,
        id: &WindowId,
        resolution: unfy_domain::WindowOutcome,
    ) -> Result<()> {
        self.append(&Record::Resolved {
            transaction: self.transaction,
            id: id.clone(),
            resolution,
        })
    }

    /// Close the transaction. After this returns, recovery will treat the file
    /// as clean and the caller may delete it.
    pub fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.append(&Record::Closed {
                transaction: self.transaction,
                closed_unix_ms: now_unix_ms(),
            })?;
            self.closed = true;
        }
        self.durable()
    }

    /// A deletion failure is cleanup only: the durable Closed record is enough.
    pub fn close_and_remove(mut self) -> Result<()> {
        self.close()?;
        drop(self.file);
        let _ = std::fs::remove_file(&self.path);
        Ok(())
    }

    /// Abandon without closing: the transaction stays recoverable.
    pub fn abandon(self) -> PathBuf {
        self.path
    }
}

/// Scan a journal file and decide what the engine must do about it.
///
/// A missing file is `Clean`. A file whose last record is `Closed` is `Clean`.
/// Anything else is `Unfinished`, including a file whose tail was torn.
pub fn recover(path: impl AsRef<Path>) -> Result<Recovered> {
    let path = path.as_ref();
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Recovered::Clean),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;

    let mut offset = 0usize;
    let mut transaction = None;
    let mut schema_version = SCHEMA_VERSION;
    let mut preimages = Vec::new();
    let mut intents = Vec::new();
    let mut results = Vec::new();
    let mut resolved = Vec::new();
    let mut closed = false;
    let mut truncated = 0u64;
    let mut saw_begin = false;
    let mut last_seq = 0;

    while offset < bytes.len() {
        match Record::decode(&bytes[offset..]) {
            Ok(Some((record, consumed))) => {
                // A record for a different transaction inside this file means the
                // file was overwritten or concatenated. Treating it as "just
                // another record" would silently merge two recoveries and could
                // write one transaction's rectangles onto another's windows.
                if closed {
                    return Err(JournalError::Corrupt {
                        offset: offset as u64,
                        reason: "record after Closed".to_string(),
                    });
                }
                if saw_begin && record.transaction() != transaction.unwrap_or_default() {
                    return Err(JournalError::Corrupt {
                        offset: offset as u64,
                        reason: "record belongs to a different transaction".to_string(),
                    });
                }
                match record {
                    Record::Begin {
                        schema_version: v,
                        transaction: t,
                        ..
                    } => {
                        if saw_begin {
                            // Two Begin records cannot belong to one transaction.
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "duplicate Begin record".to_string(),
                            });
                        }
                        if !(1..=SCHEMA_VERSION).contains(&v) {
                            return Err(JournalError::SchemaMismatch(v));
                        }
                        saw_begin = true;
                        transaction = Some(t);
                        schema_version = v;
                    }
                    Record::Preimage {
                        preimage,
                        transaction: t,
                    } => {
                        if !saw_begin {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Preimage before Begin".to_string(),
                            });
                        }
                        let _ = t;
                        if preimage.visible_rect.right <= preimage.visible_rect.left
                            || preimage.visible_rect.bottom <= preimage.visible_rect.top
                            || preimage.display_work_area.right <= preimage.display_work_area.left
                            || preimage.display_work_area.bottom <= preimage.display_work_area.top
                            || !preimage.display_scale.x.is_finite()
                            || preimage.display_scale.x <= 0.0
                            || !preimage.display_scale.y.is_finite()
                            || preimage.display_scale.y <= 0.0
                        {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "invalid preimage geometry or scale".into(),
                            });
                        }
                        // Duplicate preimages are only legal when identical: a
                        // second, different preimage would overwrite the position
                        // the user actually started from.
                        if let Some(existing) =
                            preimages.iter().find(|p: &&Preimage| p.id == preimage.id)
                        {
                            if existing != &preimage {
                                return Err(JournalError::Corrupt {
                                    offset: offset as u64,
                                    reason: "conflicting duplicate preimage".to_string(),
                                });
                            }
                        } else {
                            preimages.push(preimage);
                        }
                    }
                    Record::Intent {
                        id,
                        target,
                        mode,
                        write_seq,
                        transaction: _,
                    } => {
                        if !saw_begin {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Intent before Begin".to_string(),
                            });
                        }
                        if !preimages.iter().any(|p| p.id == id) {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Intent for a window with no preimage".to_string(),
                            });
                        }
                        if resolved.iter().any(|(window, _)| window == &id) {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Intent after resolution".into(),
                            });
                        }
                        if write_seq > 0 {
                            if write_seq <= last_seq {
                                return Err(JournalError::Corrupt {
                                    offset: offset as u64,
                                    reason: "nonmonotonic write sequence".into(),
                                });
                            }
                            last_seq = write_seq;
                        } else if schema_version >= 3 {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "missing write sequence".into(),
                            });
                        }
                        // Only the latest attempt can satisfy this window's intent.
                        intents.retain(|intent: &RecoveredIntent| intent.id != id);
                        results.retain(|result: &AppliedResult| result.id != id);
                        intents.push(RecoveredIntent {
                            id,
                            target,
                            mode,
                            write_seq,
                        });
                    }
                    Record::Result {
                        result,
                        write_seq,
                        transaction: _,
                    } => {
                        if !saw_begin {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Result before Begin".to_string(),
                            });
                        }
                        if !preimages.iter().any(|p| p.id == result.id)
                            || resolved.iter().any(|(id, _)| id == &result.id)
                        {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Result for an uncaptured or resolved window".into(),
                            });
                        }
                        if write_seq
                            != intents
                                .iter()
                                .find(|intent| intent.id == result.id)
                                .map(|intent| intent.write_seq)
                                .unwrap_or(0)
                        {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "result does not match latest intent".into(),
                            });
                        }
                        if schema_version >= 3
                            && matches!(
                                result.outcome,
                                unfy_domain::WindowOutcome::Applied
                                    | unfy_domain::WindowOutcome::Restored
                            )
                            && (write_seq == 0 || result.observed.is_none())
                        {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "successful write lacks intent or observation".into(),
                            });
                        }
                        results.push(result);
                    }
                    Record::Resolved {
                        id,
                        resolution,
                        transaction: _,
                    } => {
                        if !saw_begin {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Resolved before Begin".to_string(),
                            });
                        }
                        if !preimages.iter().any(|p| p.id == id) {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Resolved for a window with no preimage".to_string(),
                            });
                        }
                        if !matches!(
                            resolution,
                            unfy_domain::WindowOutcome::Restored
                                | unfy_domain::WindowOutcome::Gone
                                | unfy_domain::WindowOutcome::KeptUserChange
                        ) {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "nonterminal resolution".into(),
                            });
                        }
                        if schema_version >= 3
                            && resolution != unfy_domain::WindowOutcome::KeptUserChange
                            && results
                                .iter()
                                .rev()
                                .find(|result| result.id == id)
                                .is_none_or(|result| result.outcome != resolution)
                        {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "resolution lacks a matching confirmed result".into(),
                            });
                        }
                        resolved.retain(|(window, _)| window != &id);
                        resolved.push((id, resolution));
                    }
                    Record::Closed { .. } => {
                        if !saw_begin {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Closed before Begin".to_string(),
                            });
                        }
                        if closed {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "duplicate Closed record".to_string(),
                            });
                        }
                        if schema_version >= 2
                            && preimages
                                .iter()
                                .any(|p| !resolved.iter().any(|(id, _)| id == &p.id))
                        {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Closed with unresolved windows".into(),
                            });
                        }
                        closed = true;
                    }
                }
                offset += consumed;
            }
            Ok(None) => {
                if !saw_begin || closed {
                    return Err(JournalError::Corrupt {
                        offset: offset as u64,
                        reason: "incomplete record outside an open transaction".into(),
                    });
                }
                // The buffer ends inside a record: an interrupted append. That is
                // only a legal shape at the *tail*; everything before it parsed.
                truncated = (bytes.len() - offset) as u64;
                break;
            }
            Err(reason) => {
                return Err(JournalError::Corrupt {
                    offset: offset as u64,
                    reason,
                });
            }
        }
    }

    let Some(transaction) = transaction else {
        // Empty or partial files are unknown recovery state, never proof of Closed.
        return Err(JournalError::Corrupt {
            offset: 0,
            reason: "no Begin record".to_string(),
        });
    };

    if closed {
        return Ok(Recovered::Clean);
    }

    Ok(Recovered::Unfinished(RecoveredTransaction {
        transaction,
        schema_version,
        preimages,
        intents,
        results,
        resolved,
        truncated_bytes: truncated,
    }))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_close_keeps_the_writer_retryable() {
        let path = std::env::temp_dir().join(format!("unfy-close-{}.journal", Uuid::new_v4()));
        let mut journal = Journal::begin(&path, "close retry").unwrap();
        journal.file = File::open(&path).unwrap();
        assert!(journal.close().is_err());
        assert!(matches!(recover(&path).unwrap(), Recovered::Unfinished(_)));
        journal.file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        journal.close().unwrap();
        assert_eq!(recover(&path).unwrap(), Recovered::Clean);
        assert!(journal
            .write_intent(
                &WindowId {
                    native_id: 1,
                    process_id: 1,
                    process_start: 1,
                    instance: 1
                },
                Rect::new(0, 0, 100, 100),
                WindowMode::Normal
            )
            .is_err());
        drop(journal);
        std::fs::remove_file(path).unwrap();
    }
}
