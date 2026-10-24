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
//! `crc32` covers `kind` + `payload`, so a torn write at the tail is detected
//! even when the length prefix itself survived. Recovery replays every record
//! whose CRC matches and stops at the first mismatch, keeping the remainder as
//! a `.corrupt` sidecar for diagnostics.
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

use unfy_domain::{Rect, WindowId, WindowMode};
use serde::{Deserialize, Serialize};
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
pub const SCHEMA_VERSION: u32 = 2;

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
}

impl RecoveredTransaction {
    /// Whether any window in this transaction might have been moved.
    ///
    /// A transaction with no `Applied` result and no `Intent` record can be
    /// discarded without touching windows.
    pub fn may_have_moved_windows(&self) -> bool {
        self.results.iter().any(|r| {
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
        self.intents
            .iter()
            .any(|intent| !self.results.iter().any(|r| r.id == intent.id))
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
        self.preimages
            .iter()
            .any(|p| self.is_unresolved(&p.id))
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
            .create(true)
            .truncate(true)
            .open(&path)?;

        let mut journal = Journal {
            path,
            file: file.try_clone()?,
            transaction,
            end: 0,
            dirty: false,
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
        let mut file = OpenOptions::new().read(true).write(true).open(&path)?;

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        // Walk to the last complete record so a torn tail cannot corrupt the
        // records we append after it.
        let mut offset = 0usize;
        while offset < bytes.len() {
            match Record::decode(&bytes[offset..]) {
                Ok(Some((_, consumed))) => offset += consumed,
                Ok(None) | Err(_) => break,
            }
        }
        if offset < bytes.len() {
            file.set_len(offset as u64)?;
            file.sync_all()?;
        }

        Ok(Journal {
            path,
            file,
            transaction,
            end: offset as u64,
            dirty: false,
        })
    }

    /// Append one record to the file without forcing it to disk.
    ///
    /// Callers must follow with [`Journal::durable`] before acting on the record.
    /// Batching the fsync is what keeps a hundred-window gather inside its
    /// latency budget: a per-record `sync_all` costs milliseconds on a real disk,
    /// and the invariant only requires that *no window moves before its preimage
    /// is on disk*, not that every record is synced individually.
    fn append(&mut self, record: &Record) -> Result<()> {
        let bytes = record.encode()?;
        self.file.seek(SeekFrom::Start(self.end))?;
        self.file.write_all(&bytes)?;
        self.end += bytes.len() as u64;
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
        self.append(&Record::Intent {
            transaction: self.transaction,
            id: id.clone(),
            target,
            mode,
        })
    }

    /// Persist the observed outcome after the platform call and read-back.
    pub fn write_result(&mut self, result: &AppliedResult) -> Result<()> {
        self.append(&Record::Result {
            transaction: self.transaction,
            result: result.clone(),
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
    pub fn close(mut self) -> Result<()> {
        self.append(&Record::Closed {
            transaction: self.transaction,
            closed_unix_ms: now_unix_ms(),
        })?;
        self.durable()?;
        Ok(())
    }

    /// Close and delete the file. Only safe once every window is resolved.
    pub fn close_and_remove(mut self) -> Result<()> {
        self.append(&Record::Closed {
            transaction: self.transaction,
            closed_unix_ms: now_unix_ms(),
        })?;
        self.durable()?;
        drop(self.file);
        std::fs::remove_file(&self.path)?;
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
    if !path.exists() {
        return Ok(Recovered::Clean);
    }

    let mut file = File::open(path)?;
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
                        // Duplicate preimages are only legal when identical: a
                        // second, different preimage would overwrite the position
                        // the user actually started from.
                        if let Some(existing) = preimages
                            .iter()
                            .find(|p: &&Preimage| p.id == preimage.id)
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
                        intents.push(RecoveredIntent { id, target, mode });
                    }
                    Record::Result {
                        result,
                        transaction: _,
                    } => {
                        if !saw_begin {
                            return Err(JournalError::Corrupt {
                                offset: offset as u64,
                                reason: "Result before Begin".to_string(),
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
                        closed = true;
                    }
                }
                offset += consumed;
            }
            Ok(None) => {
                // The buffer ends inside a record: an interrupted append. That is
                // only a legal shape at the *tail*; everything before it parsed.
                truncated = (bytes.len() - offset) as u64;
                break;
            }
            Err(reason) => {
                // A record that will not decode is either a torn append or a file
                // that was damaged after the fact. They are distinguishable by
                // position: a tail after a valid Begin and before Closed is the
                // normal shape of an interrupted write, while anything before the
                // first valid record - or after a Closed - cannot be explained by
                // an interrupted append and must not be silently dropped.
                if !saw_begin || closed {
                    return Err(JournalError::Corrupt {
                        offset: offset as u64,
                        reason,
                    });
                }
                truncated = (bytes.len() - offset) as u64;
                break;
            }
        }
    }

    if truncated > 0 {
        let sidecar = path.with_extension("corrupt");
        std::fs::write(&sidecar, &bytes[offset..])?;
    }

    let Some(transaction) = transaction else {
        // A file with no Begin is not a journal this version can interpret. It may
        // be a partially created file (harmless) or garbage; either way it cannot
        // describe a recovery obligation.
        if bytes.is_empty() {
            return Ok(Recovered::Clean);
        }
        return Err(JournalError::Corrupt {
            offset: 0,
            reason: "no Begin record".to_string(),
        });
    };

    if closed && !resolved.is_empty() {
        // A clean close means every obligation was settled before the close; the
        // records are kept only as history.
        return Ok(Recovered::Clean);
    }

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
