//! The single-writer engine.
//!
//! Every window mutation in the product flows through [`Engine`]. It owns the
//! state machine, the journal, and the read-back retry policy, and it is the only
//! place that decides "it is now safe to move this window".
//!
//! # Ordering rules that are not negotiable
//!
//! 1. Preimage on disk **before** the first write for that window.
//! 2. Intent on disk **before** the platform call, so a crash between the call
//!    and the read-back is still recoverable.
//! 3. Result on disk after the read-back, whether it succeeded or not.
//! 4. Restore before any new migration, before updating, before exit.
//!
//! # Why the engine is generic
//!
//! `Engine<B: Backend>` takes the platform behind a trait, so the entire state
//! machine runs against the fake backend in tests. The Windows and macOS
//! backends only supply enumeration, apply and read-back.

use std::collections::HashMap;
use std::path::PathBuf;

pub use conflux_domain::EngineState;

use conflux_domain::{
    rescale_between, DisplayKey, DisplaySnapshot, EngineError, IdentityQuality, Rect, SkipReason,
    WindowId, WindowMode, WindowOutcome, WindowSnapshot,
};
use conflux_journal::{
    AppliedResult, Journal, ObservedRect, Preimage, Recovered, RecoveredTransaction,
};
use conflux_layout::{Candidate, LayoutConfig, Plan, Unplaced};
use conflux_platform::{
    Capabilities, DisplayProvider, ObservedWindow, PlatformError, WindowBackend, WindowPlan,
};

/// What the engine can ask of a platform.
///
/// Combining the two traits keeps call sites short and makes the fake a single
/// type in tests.
pub trait Backend: DisplayProvider + WindowBackend {}

impl<T> Backend for T where T: DisplayProvider + WindowBackend {}

/// A window that passed filtering, with the facts the planner needs.
#[derive(Debug, Clone, PartialEq)]
pub struct EligibleWindow {
    pub snapshot: WindowSnapshot,
    pub display: DisplaySnapshot,
    pub min_size: Option<conflux_domain::Size>,
    pub focused: bool,
}

/// A window the engine refused to touch, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedWindow {
    pub id: WindowId,
    /// Application name for display purposes; empty when unknown.
    pub app_name: String,
    pub reason: SkipReason,
}

/// The result of a dry run. Never touches a window.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub target: DisplayKey,
    pub plan: Plan,
    pub eligible: Vec<WindowId>,
    pub skipped: Vec<SkippedWindow>,
}

/// Per-window progress reported to the UI while a migration runs.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationReport {
    pub transaction: uuid::Uuid,
    pub applied: Vec<WindowId>,
    pub outcomes: HashMap<WindowId, WindowOutcome>,
    pub skipped: Vec<SkippedWindow>,
    pub unplaced: Vec<WindowId>,
    pub cascaded: bool,
    /// Windows the application refused to move, with the process to name.
    pub refused: Vec<RefusedWindow>,
}

/// Result of a retarget: how many windows went home first, plus the gather.
#[derive(Debug, Clone, PartialEq)]
pub struct RetargetReport {
    pub restored: usize,
    pub migration: MigrationReport,
}

/// A window that stayed where it was despite the request.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedWindow {
    pub id: WindowId,
    /// Why it was refused, for the UI's explanation.
    pub outcome: WindowOutcome,
}

impl MigrationReport {
    pub fn applied_count(&self) -> usize {
        self.outcomes
            .values()
            .filter(|o| **o == WindowOutcome::Applied)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.outcomes
            .values()
            .filter(|o| matches!(o, WindowOutcome::Rejected | WindowOutcome::Unknown))
            .count()
    }
}

/// Summary of a restore pass.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreReport {
    pub transaction: Option<uuid::Uuid>,
    pub outcomes: HashMap<WindowId, WindowOutcome>,
    pub pending: Vec<WindowId>,
}

impl RestoreReport {
    pub fn is_complete(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn count(&self, outcome: WindowOutcome) -> usize {
        self.outcomes.values().filter(|o| **o == outcome).count()
    }
}

/// Configuration the engine needs from settings.
#[derive(Clone)]
pub struct EngineConfig {
    pub layout: LayoutConfig,
    /// How far a window may drift from the requested rectangle before the engine
    /// retries. Four physical pixels matches the product spec's tolerance.
    pub tolerance_px: i32,
    /// Attempts per window before it is reported as `Rejected`.
    ///
    /// Two is deliberate: the first write and one retry distinguish "the message
    /// pump was slow" from "this application does not accept the move". A third
    /// attempt costs another settle delay and has never been observed to succeed
    /// in the verification harness.
    pub max_attempts: u8,
    /// Pause before re-requesting a window whose position did not stick.
    ///
    /// Only paid on the retry pass, so a batch of N windows costs at most one
    /// settle wait per diverging window rather than one per window.
    pub retry_delay_ms: u64,
    /// Where journal files live. One file per transaction.
    pub journal_dir: PathBuf,
    /// A window narrower than this is not considered "reachable".
    pub min_reachable: conflux_domain::Size,
    /// Seconds to wait at the start of `restore()`.
    /// Advisory: the desktop layer waits this long before calling `restore`.
    /// Kept in the config so diagnostics can report it, but the engine never
    /// sleeps on it — see the note in `restore`.
    pub restore_delay_secs: u32,
    /// Called after each window has been submitted, with (done, total).
    ///
    /// The engine has no UI thread and must not grow one; this is the smallest
    /// hook that lets a caller report progress. A batch of a hundred windows
    /// completes in ~200 ms on an idle desktop, but one application that is
    /// busy pumping its message queue can hold a single window for seconds - and
    /// an indeterminate spinner for that long reads as a hang.
    pub on_progress: Option<std::sync::Arc<dyn Fn(usize, usize) + Send + Sync>>,
    /// Executable names that are never moved.
    ///
    /// Matched case-insensitively against `WindowSnapshot::app_name`. This is the
    /// escape hatch for applications the user does not want rearranged: media
    /// players, capture tools, games, a full-screen dashboard.
    pub excluded_apps: Vec<String>,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The progress callback is a closure and cannot be printed; everything
        // else is plain data. Printing `on_progress: Some(..)` keeps the shape of
        // the struct visible in logs without requiring a Debug bound on it.
        f.debug_struct("EngineConfig")
            .field("layout", &self.layout)
            .field("tolerance_px", &self.tolerance_px)
            .field("max_attempts", &self.max_attempts)
            .field("retry_delay_ms", &self.retry_delay_ms)
            .field("journal_dir", &self.journal_dir)
            .field("min_reachable", &self.min_reachable)
            .field("restore_delay_secs", &self.restore_delay_secs)
            .field(
                "on_progress",
                &self.on_progress.as_ref().map(|_| "<callback>"),
            )
            .field("excluded_apps", &self.excluded_apps)
            .finish()
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            on_progress: None,
            layout: LayoutConfig::default(),
            tolerance_px: 4,
            max_attempts: 2,
            retry_delay_ms: 60,
            journal_dir: std::env::temp_dir().join("conflux-journal"),
            min_reachable: conflux_domain::Size::new(64, 32),
            restore_delay_secs: 0,
            excluded_apps: Vec::new(),
        }
    }
}

/// The single writer.
pub struct Engine<B: Backend> {
    backend: B,
    config: EngineConfig,
    state: EngineState,
    /// Live transaction, if any.
    transaction: Option<Transaction>,
    /// Outcomes accumulated for the current transaction.
    outcomes: HashMap<WindowId, WindowOutcome>,
    /// Preimages captured for the current transaction, keyed by window.
    preimages: HashMap<WindowId, Preimage>,
    /// Set by `pause()`; blocks new gathers without discarding the layout.
    paused: bool,
}

struct Transaction {
    id: uuid::Uuid,
    journal: Journal,
    /// The rectangle each window was last placed at by us, used to tell a user
    /// move apart from our own write.
    last_applied: HashMap<WindowId, ObservedWindow>,
}

impl<B: Backend> Engine<B> {
    pub fn new(backend: B, config: EngineConfig) -> Self {
        Self {
            backend,
            config,
            state: EngineState::Idle,
            transaction: None,
            outcomes: HashMap::new(),
            preimages: HashMap::new(),
            paused: false,
        }
    }

    pub fn state(&self) -> EngineState {
        self.state
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Apply changed layout or timing settings.
    ///
    /// Refused while a transaction is open: the plan for the current gather was
    /// computed with the old settings, and swapping them mid-flight would make
    /// the journal disagree with what the user saw in the preview. The caller
    /// must either wait for the next gather or restore first.
    pub fn update_config(&mut self, config: EngineConfig) -> Result<(), EngineError> {
        if self.state.may_write() {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        self.config = config;
        Ok(())
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Resolve the configured target against the live topology.
    ///
    /// Returns the configured display when it is still present, otherwise the
    /// primary display. Never guesses silently: callers that need to distinguish
    /// "configured target gone" from "no displays" should use
    /// `preview()`/`migrate()` and handle `TargetDisplayMissing`.
    pub fn resolve_target(&self, configured_native_id: Option<&str>) -> Option<DisplayKey> {
        let displays = self.backend.topology().ok()?;
        if let Some(native_id) = configured_native_id {
            if let Some(found) = displays.iter().find(|d| d.key.native_id == native_id) {
                return Some(found.key.clone());
            }
        }
        displays
            .iter()
            .find(|d| d.primary)
            .or_else(|| displays.first())
            .map(|d| d.key.clone())
    }

    /// How many windows the current transaction has captured.
    pub fn captured_window_count(&self) -> usize {
        self.preimages.len()
    }

    /// The transaction this engine currently owns, if any.
    ///
    /// The UI uses this to avoid reporting the live journal as "work left over
    /// from a previous run".
    pub fn current_transaction(&self) -> Option<uuid::Uuid> {
        self.transaction.as_ref().map(|t| t.id)
    }

    /// A cheap fingerprint of the current display topology.
    ///
    /// Used to detect a hot-plug: when this changes, any plan computed against
    /// the old topology is stale and windows may now sit outside every work
    /// area. Comparing display identity plus geometry catches an unplug, a
    /// resolution change, and a rearrangement, and nothing else.
    pub fn topology_signature(&self) -> Option<String> {
        let displays = self.backend.topology().ok()?;
        let mut parts: Vec<String> = displays
            .iter()
            .map(|d| {
                format!(
                    "{}|{}x{}@{},{}|{:.3}",
                    d.key.native_id,
                    d.work_area.width(),
                    d.work_area.height(),
                    d.work_area.left,
                    d.work_area.top,
                    d.scale.x
                )
            })
            .collect();
        parts.sort();
        Some(parts.join(";"))
    }

    /// React to a display being added, removed or reconfigured.
    ///
    /// Returns the windows that had to be moved back into a visible area.
    ///
    /// The dangerous case is a monitor disappearing while windows sit on it:
    /// those windows are now unreachable, and the user may not even be able to
    /// see that they exist. Rescue is deliberately allowed here even while a
    /// transaction is open, because leaving a window off-screen is worse than
    /// acting outside the normal plan. The snapshot keeps the original
    /// coordinates, so a later restore can still put the window back if the
    /// display returns.
    pub fn on_topology_changed(&mut self) -> Result<Vec<(WindowId, Rect)>, EngineError> {
        tracing::warn!(state = ?self.state, "display topology changed; rescuing unreachable windows");
        let rescued = self.rescue_offscreen()?;
        if !rescued.is_empty() {
            tracing::warn!(
                count = rescued.len(),
                "moved windows back into a visible area"
            );
        }
        Ok(rescued)
    }

    /// Stop acting on new gather requests while keeping captured windows.
    ///
    /// Pausing is not the same as restoring: the layout stays, the journal stays
    /// open, and `resume()` puts the engine back to `Active` without touching a
    /// window. This is what the user needs when they briefly work locally on the
    /// host and do not want the next hotkey press to move anything.
    pub fn pause(&mut self) -> Result<(), EngineError> {
        if self.transaction.is_none() {
            return Ok(());
        }
        if self.state == EngineState::Migrating || self.state == EngineState::Restoring {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        self.paused = true;
        tracing::info!("engine paused; layout and journal retained");
        Ok(())
    }

    /// Resume acting on gather requests.
    pub fn resume(&mut self) -> Result<(), EngineError> {
        if !self.paused {
            return Ok(());
        }
        self.paused = false;
        if self.transaction.is_some() {
            self.state = EngineState::Active;
        }
        tracing::info!("engine resumed");
        Ok(())
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Adopt a journal left behind by a previous run.
    ///
    /// Must be called before the engine is allowed to migrate, and before the UI
    /// offers "restore". Adopting means the engine takes ownership of the
    /// transaction: reopening the file for appends and loading the preimages into
    /// memory, so `restore()` can put the windows back instead of merely telling
    /// the user that something is wrong.
    ///
    /// A transaction with no preimages is discarded immediately; there is
    /// nothing to restore and keeping it would block every future migration.
    pub fn recover(&mut self, path: impl AsRef<std::path::Path>) -> Result<Recovered, EngineError> {
        let path = path.as_ref();
        let recovered = conflux_journal::recover(path)
            .map_err(|e| EngineError::JournalCorrupt(e.to_string()))?;

        let Recovered::Unfinished(tx) = &recovered else {
            return Ok(recovered);
        };

        if tx.preimages.is_empty() {
            // Nothing was captured, so nothing can have moved. A leftover
            // "Begin" record must not block the next migration forever.
            std::fs::remove_file(path).ok();
            self.state = EngineState::Idle;
            return Ok(Recovered::Clean);
        }

        let journal = Journal::reopen(path, tx.transaction)
            .map_err(|e| EngineError::JournalCorrupt(e.to_string()))?;

        self.preimages = tx
            .preimages
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        self.transaction = Some(Transaction {
            id: tx.transaction,
            journal,
            last_applied: HashMap::new(),
        });
        self.state = EngineState::Degraded;

        tracing::warn!(
            transaction = %tx.transaction,
            preimages = tx.preimages.len(),
            may_have_moved = tx.may_have_moved_windows(),
            "adopted unfinished transaction"
        );
        Ok(recovered)
    }

    /// Read the newest journal in `dir` and adopt it, if any.
    ///
    /// Called once at startup. Returns the adopted transaction's id when the
    /// engine took over work from a previous run.
    pub fn recover_latest(
        &mut self,
        dir: impl AsRef<std::path::Path>,
    ) -> Result<Option<uuid::Uuid>, EngineError> {
        let dir = dir.as_ref();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(None);
        };

        // Only the newest journal can belong to the interrupted run; every
        // earlier file is from a session that already ended or was superseded.
        // Those are deleted here rather than accumulated: a user whose app
        // crashed a few times would otherwise carry a growing pile of files that
        // no code path ever reads again, and each startup would scan them.
        let mut journals: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "journal") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            journals.push((modified, path));
        }
        journals.sort_by_key(|(time, _)| std::cmp::Reverse(*time));

        let newest = journals.first().cloned().map(|(_, path)| path);
        for (_, stale) in journals.iter().skip(1) {
            match std::fs::remove_file(stale) {
                Ok(()) => tracing::info!(path = %stale.display(), "removed superseded journal"),
                Err(e) => tracing::warn!(
                    path = %stale.display(),
                    error = %e,
                    "could not remove superseded journal"
                ),
            }
        }

        let Some(path) = newest else {
            return Ok(None);
        };

        match self.recover(&path)? {
            Recovered::Unfinished(tx) => Ok(Some(tx.transaction)),
            Recovered::Clean => Ok(None),
        }
    }

    /// Enumerate, filter and plan without touching a window.
    pub fn preview(&mut self, target: &DisplayKey) -> Result<Preview, EngineError> {
        if self.state.may_write() {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        self.state = EngineState::Planning;

        let preview = self.build_preview(target)?;
        self.state = EngineState::Idle;
        Ok(preview)
    }

    fn build_preview(&self, target: &DisplayKey) -> Result<Preview, EngineError> {
        let displays = self
            .backend
            .topology()
            .map_err(|e| EngineError::Platform(e.to_string()))?;
        let target_display = displays
            .iter()
            .find(|d| &d.key == target)
            .cloned()
            .ok_or(EngineError::TargetDisplayMissing)?;

        let windows = self
            .backend
            .enumerate()
            .map_err(|e| EngineError::Platform(e.to_string()))?;

        let mut eligible = Vec::new();
        let mut skipped = Vec::new();
        for window in windows {
            match self.classify(&window, &displays) {
                Ok(Some((display, min_size))) => eligible.push(EligibleWindow {
                    snapshot: window,
                    display,
                    min_size,
                    focused: false,
                }),
                Ok(None) => {}
                Err(reason) => skipped.push(SkippedWindow {
                    id: window.id.clone(),
                    app_name: window.app_name.clone(),
                    reason,
                }),
            }
        }

        let candidates: Vec<Candidate> = eligible
            .iter()
            .map(|e| Candidate {
                id: e.snapshot.id.clone(),
                current: e.snapshot.visible_rect,
                min_size: e.min_size,
                focused: e.focused,
            })
            .collect();

        let plan = conflux_layout::plan(&candidates, target_display.work_area, &self.config.layout);

        Ok(Preview {
            target: target.clone(),
            plan,
            eligible: eligible.iter().map(|e| e.snapshot.id.clone()).collect(),
            skipped,
        })
    }

    /// Decide whether a window may be moved, and find its display.
    ///
    /// `Ok(None)` means "not a candidate but not a problem" (for example, the
    /// window is on another virtual desktop and is invisible by design).
    fn classify(
        &self,
        window: &WindowSnapshot,
        displays: &[DisplaySnapshot],
    ) -> Result<Option<(DisplaySnapshot, Option<conflux_domain::Size>)>, SkipReason> {
        if self.is_excluded(window) {
            return Err(SkipReason::ExcludedApp);
        }
        if !window.is_migratable() {
            return Err(match window.mode {
                WindowMode::Fullscreen => SkipReason::Fullscreen,
                WindowMode::Minimized => SkipReason::Minimized,
                _ => SkipReason::Invisible,
            });
        }

        let capabilities = self.backend.capabilities(&window.id);
        if !capabilities.can_move {
            return Err(SkipReason::PermissionDenied);
        }

        let display = displays
            .iter()
            .find(|d| d.key == window.display)
            .or_else(|| {
                displays
                    .iter()
                    .max_by_key(|d| d.bounds.intersection_area(&window.visible_rect))
            })
            .ok_or(SkipReason::Invisible)?;

        Ok(Some((display.clone(), None)))
    }

    /// Move every eligible window onto `target`.
    ///
    /// Returns an error only when the transaction could not be started or had to
    /// be aborted. Per-window failures are reported in the result.
    pub fn migrate(&mut self, target: &DisplayKey) -> Result<MigrationReport, EngineError> {
        if self.state.may_write() {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        if self.state == EngineState::Error {
            return Err(EngineError::InvalidTransition { from: self.state });
        }

        // A second gather while a transaction is open must not capture new
        // preimages: the windows have already moved, so the "original" position
        // would be recorded as the position this engine just produced. The user
        // asked to change the target, not to redefine home. Restore first.
        // Pause is checked first: when both apply, "automation is paused" is the
        // actionable message, and telling the user to restore would be wrong —
        // resuming is what they actually need to do.
        if self.paused {
            return Err(EngineError::Paused);
        }
        if self.transaction.is_some() {
            return Err(EngineError::TransactionAlreadyOpen);
        }

        let t0 = std::time::Instant::now();
        let preview = self.build_preview(target)?;
        let t_preview = t0.elapsed().as_millis();
        if preview.plan.is_empty() {
            self.state = EngineState::Idle;
            return Ok(MigrationReport {
                transaction: uuid::Uuid::nil(),
                applied: Vec::new(),
                outcomes: HashMap::new(),
                skipped: preview.skipped,
                unplaced: Vec::new(),
                cascaded: false,
                refused: Vec::new(),
            });
        }

        self.state = EngineState::Migrating;

        let t1 = std::time::Instant::now();
        // 1. Durable preimages for every window we are about to touch.
        let journal_path = self
            .config
            .journal_dir
            .join(format!("{}.journal", uuid::Uuid::new_v4()));
        let mut journal = Journal::begin(
            &journal_path,
            &format!(
                "target={} displays={}",
                preview.target.native_id,
                preview.plan.placements.len()
            ),
        )
        .map_err(|e| EngineError::JournalWrite(e.to_string()))?;

        let transaction_id = journal.transaction();

        // Outcomes are collected while capturing preimages too (a window can
        // vanish before we snapshot it), so clear before the loop, not after.
        self.outcomes.clear();

        let mut preimages = HashMap::new();
        for placement in &preview.plan.placements {
            let Some(eligible) = preview.eligible.iter().find(|id| **id == placement.id) else {
                continue;
            };
            let _ = eligible;

            // Re-read the window so the preimage matches what is on screen now,
            // not what enumeration reported a moment ago.
            let observed = match self.backend.read_back(&placement.id) {
                Ok(observed) => observed,
                Err(PlatformError::WindowGone) => {
                    // The window disappeared between enumeration and now. There
                    // is nothing to snapshot and nothing to move, but the user
                    // still deserves to see it in the report.
                    self.outcomes
                        .insert(placement.id.clone(), WindowOutcome::Gone);
                    continue;
                }
                Err(_) => continue,
            };

            let snapshot = self
                .backend
                .enumerate()
                .map_err(|e| EngineError::Platform(e.to_string()))?
                .into_iter()
                .find(|w| w.id == placement.id);

            let Some(snapshot) = snapshot else {
                continue;
            };
            let display = self.display_of(&snapshot)?;

            let preimage = Preimage {
                id: snapshot.id.clone(),
                display_native_id: display.key.native_id.clone(),
                display_quality: display.key.quality,
                display_work_area: display.work_area,
                display_scale: display.scale,
                visible_rect: observed.visible_rect,
                outer_rect: snapshot.outer_rect,
                mode: observed.mode,
                topmost: snapshot.topmost,
                native_placement: snapshot.native_placement.clone(),
            };
            journal
                .write_preimage(&preimage)
                .map_err(|e| EngineError::JournalWrite(e.to_string()))?;
            preimages.insert(placement.id.clone(), preimage);
        }

        let t_preimage = t1.elapsed().as_millis();

        // The invariant: every preimage is on disk before the first window moves.
        journal
            .durable()
            .map_err(|e| EngineError::JournalWrite(e.to_string()))?;

        if preimages.is_empty() {
            journal
                .close_and_remove()
                .map_err(|e| EngineError::JournalWrite(e.to_string()))?;
            self.state = EngineState::Idle;
            return Ok(MigrationReport {
                transaction: transaction_id,
                applied: Vec::new(),
                // Windows that vanished during preimage capture are still
                // reported; silently dropping them would look like success.
                outcomes: self.outcomes.clone(),
                skipped: preview.skipped,
                unplaced: Vec::new(),
                cascaded: preview.plan.cascaded,
                refused: Vec::new(),
            });
        }

        self.transaction = Some(Transaction {
            id: transaction_id,
            journal,
            last_applied: HashMap::new(),
        });
        self.preimages = preimages;

        // 2. Apply, one window at a time, with the intent written first.
        //    Windows that do not settle are collected here and retried once the
        //    whole plan has been submitted.
        let mut settling: Vec<(WindowId, WindowPlan)> = Vec::new();
        let mut applied = Vec::new();
        let mut unplaced = Vec::new();
        for id in preview.plan.unplaced.iter().map(|(id, _)| id.clone()) {
            unplaced.push(id);
        }

        // Intents are written in batches: every intent in a batch is durable
        // before any window in it is touched, so a crash can only leave windows
        // whose intent is already on disk. Batching keeps the fsync count
        // proportional to batches rather than windows.
        const BATCH: usize = 16;
        let placements = &preview.plan.placements;
        let mut next = 0usize;

        while next < placements.len() {
            let end = (next + BATCH).min(placements.len());
            let Some(transaction) = self.transaction.as_mut() else {
                break;
            };
            let mut wrote_all = true;
            for placement in &placements[next..end] {
                if transaction
                    .journal
                    .write_intent(&placement.id, placement.rect, WindowMode::Normal)
                    .is_err()
                {
                    wrote_all = false;
                    break;
                }
            }
            let durable = transaction.journal.durable().is_ok();
            if !wrote_all || !durable {
                break;
            }

            for placement in &placements[next..end] {
                let plan = WindowPlan {
                    visible_rect: placement.rect,
                    mode: WindowMode::Normal,
                };

                // First pass: submit and read back, never sleep. Diverging windows are
                // retried after the whole plan has been submitted, so one slow
                // application cannot serialise a hundred windows behind it.
                let outcome = self.apply_once(&placement.id, &plan);
                if outcome == WindowOutcome::Unknown {
                    settling.push((placement.id.clone(), plan));
                }

                let observed = self.backend.read_back(&placement.id).ok();

                let record = AppliedResult {
                    id: placement.id.clone(),
                    outcome,
                    observed: observed.map(|o| ObservedRect {
                        visible_rect: o.visible_rect,
                        mode: o.mode,
                    }),
                };
                if let Some(transaction) = self.transaction.as_mut() {
                    let _ = transaction.journal.write_result(&record);
                    if let Some(observed) = observed {
                        transaction
                            .last_applied
                            .insert(placement.id.clone(), observed);
                    }
                }
                self.outcomes.insert(placement.id.clone(), outcome);
                if let Some(progress) = self.config.on_progress.as_ref() {
                    progress(self.outcomes.len(), placements.len());
                }
                if outcome == WindowOutcome::Applied {
                    applied.push(placement.id.clone());
                }
            }

            next = end;
        }

        let t_apply = t1.elapsed().as_millis().saturating_sub(t_preimage);

        if let Some(transaction) = self.transaction.as_mut() {
            if transaction.journal.durable().is_err() {
                tracing::warn!("could not flush first-pass results to disk");
            }
        }

        tracing::debug!(
            enumerate_and_plan_ms = t_preview,
            preimage_ms = t_preimage,
            apply_ms = t_apply,
            "migration phase timings"
        );

        // 3. Retry pass. Only diverging windows pay the settle delay, and the
        //    first retry is immediate: an application that simply had not pumped
        //    its queue yet usually lands correctly on the second request, so
        //    sleeping before it would waste the budget for no benefit.
        for (id, plan) in settling {
            let outcome = self.apply_with_retry(&id, &plan);
            let observed = self.backend.read_back(&id).ok();
            let record = AppliedResult {
                id: id.clone(),
                outcome,
                observed: observed.map(|o| ObservedRect {
                    visible_rect: o.visible_rect,
                    mode: o.mode,
                }),
            };
            if let Some(transaction) = self.transaction.as_mut() {
                let _ = transaction.journal.write_result(&record);
                if let Some(observed) = observed {
                    transaction.last_applied.insert(id.clone(), observed);
                }
            }
            if outcome == WindowOutcome::Applied && !applied.contains(&id) {
                applied.push(id.clone());
            }
            self.outcomes.insert(id, outcome);
        }

        if let Some(transaction) = self.transaction.as_mut() {
            if transaction.journal.durable().is_err() {
                tracing::warn!("could not flush retry results to disk");
            }
        }

        self.state = if applied.is_empty() {
            EngineState::Degraded
        } else {
            EngineState::Active
        };
        tracing::info!(
            transaction = %transaction_id,
            applied = applied.len(),
            refused = self
                .outcomes
                .values()
                .filter(|o| matches!(o, WindowOutcome::Rejected | WindowOutcome::Unknown))
                .count(),
            skipped = preview.skipped.len(),
            cascaded = preview.plan.cascaded,
            state = ?self.state,
            "migration finished"
        );

        let refused = self
            .outcomes
            .iter()
            .filter(|(_, outcome)| {
                matches!(outcome, WindowOutcome::Rejected | WindowOutcome::Unknown)
            })
            .map(|(id, outcome)| RefusedWindow {
                id: id.clone(),
                outcome: *outcome,
            })
            .collect();

        Ok(MigrationReport {
            transaction: transaction_id,
            applied,
            outcomes: self.outcomes.clone(),
            skipped: preview.skipped,
            unplaced,
            cascaded: preview.plan.cascaded,
            refused,
        })
    }

    /// Submit one window and read it back once.
    ///
    /// This is the *first* pass: it never sleeps, so a hundred windows cost one
    /// round of platform calls rather than a worst-case wait per window. Windows
    /// that have not settled are collected and retried afterwards by
    /// [`Self::apply_with_retry`], which is where the settle delay is paid.
    fn apply_once(&self, id: &WindowId, plan: &WindowPlan) -> WindowOutcome {
        // Async submission: do not block on the target application pumping its
        // queue. The read-back below still verifies the result, and diverging
        // windows are retried synchronously afterwards.
        match self.backend.apply_async(id, plan) {
            Ok(()) => {}
            Err(PlatformError::PermissionDenied) | Err(PlatformError::Unsupported) => {
                return WindowOutcome::Rejected
            }
            Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
            Err(_) => return WindowOutcome::Unknown,
        }

        match self.backend.read_back(id) {
            Ok(observed) => {
                if within_tolerance(
                    observed.visible_rect,
                    plan.visible_rect,
                    self.config.tolerance_px,
                ) {
                    WindowOutcome::Applied
                } else {
                    // Diverged, but not yet a failure: the retry pass gets a
                    // chance once every other window has been submitted.
                    WindowOutcome::Unknown
                }
            }
            Err(PlatformError::WindowGone) => WindowOutcome::Gone,
            Err(_) => WindowOutcome::Unknown,
        }
    }

    /// Retry a window that did not settle on the first pass.
    fn apply_with_retry(&self, id: &WindowId, plan: &WindowPlan) -> WindowOutcome {
        for attempt in 1..=self.config.max_attempts {
            match self.backend.apply(id, plan) {
                Ok(()) => {}
                Err(PlatformError::PermissionDenied) | Err(PlatformError::Unsupported) => {
                    return WindowOutcome::Rejected
                }
                Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
                Err(_) => {
                    if attempt == self.config.max_attempts {
                        return WindowOutcome::Rejected;
                    }
                    continue;
                }
            }

            match self.backend.read_back(id) {
                Ok(observed) => {
                    if within_tolerance(
                        observed.visible_rect,
                        plan.visible_rect,
                        self.config.tolerance_px,
                    ) {
                        return WindowOutcome::Applied;
                    }
                    if attempt == self.config.max_attempts {
                        return WindowOutcome::Rejected;
                    }
                    // Let a self-correcting application settle rather than racing
                    // it; this is the only place the engine sleeps, and the first
                    // retry skips it because most divergences are just a slow
                    // message pump.
                    if attempt > 1 {
                        std::thread::sleep(std::time::Duration::from_millis(
                            self.config.retry_delay_ms,
                        ));
                    }
                }
                Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
                Err(_) => {
                    if attempt == self.config.max_attempts {
                        return WindowOutcome::Unknown;
                    }
                }
            }
        }
        WindowOutcome::Unknown
    }

    /// Whether the user asked us to leave this application alone.
    fn is_excluded(&self, window: &WindowSnapshot) -> bool {
        if window.app_name.is_empty() {
            return false;
        }
        self.config
            .excluded_apps
            .iter()
            .any(|rule| rule.eq_ignore_ascii_case(&window.app_name))
    }

    fn display_of(&self, window: &WindowSnapshot) -> Result<DisplaySnapshot, EngineError> {
        let displays = self
            .backend
            .topology()
            .map_err(|e| EngineError::Platform(e.to_string()))?;
        displays
            .into_iter()
            .find(|d| d.key == window.display)
            .or_else(|| {
                let mut best: Option<(i64, DisplaySnapshot)> = None;
                for d in self.backend.topology().unwrap_or_default().into_iter() {
                    let area = d.bounds.intersection_area(&window.visible_rect);
                    if best.as_ref().is_none_or(|(b, _)| area > *b) {
                        best = Some((area, d));
                    }
                }
                best.map(|(_, d)| d)
            })
            .ok_or(EngineError::TargetDisplayMissing)
    }

    /// Restore every window captured by the current transaction.
    ///
    /// Windows the user moved after we placed them are left alone; the outcome is
    /// `KeptUserChange`. Windows whose original display is gone are clamped into
    /// a visible work area and reported as `FallbackVisible`.
    pub fn restore(&mut self) -> Result<RestoreReport, EngineError> {
        if self.state == EngineState::Idle && self.transaction.is_none() {
            return Ok(RestoreReport {
                transaction: None,
                outcomes: HashMap::new(),
                pending: Vec::new(),
            });
        }
        if self.state == EngineState::Migrating || self.state == EngineState::Planning {
            return Err(EngineError::InvalidTransition { from: self.state });
        }

        // The user's restore delay is deliberately *not* applied here. Sleeping
        // inside this method blocks the engine mutex, which froze status polling
        // and the tray for the whole delay (up to 300 s). The desktop layer owns
        // the wait and can cancel it when a newer request arrives; see
        // `apps/desktop/src-tauri/src/schedule.rs`.
        self.state = EngineState::Restoring;
        self.paused = false;

        let Some(transaction) = self.transaction.as_ref() else {
            self.state = EngineState::Idle;
            return Ok(RestoreReport {
                transaction: None,
                outcomes: HashMap::new(),
                pending: Vec::new(),
            });
        };
        let transaction_id = transaction.id;

        let displays = self
            .backend
            .topology()
            .map_err(|e| EngineError::Platform(e.to_string()))?;

        let preimages: Vec<Preimage> = self.preimages.values().cloned().collect();
        let mut outcomes = HashMap::new();
        let mut pending = Vec::new();

        for preimage in preimages {
            let outcome = self.restore_one(&preimage, &displays);
            outcomes.insert(preimage.id.clone(), outcome);

            if let Some(transaction) = self.transaction.as_mut() {
                let _ = transaction.journal.write_result(&AppliedResult {
                    id: preimage.id.clone(),
                    outcome,
                    observed: self
                        .backend
                        .read_back(&preimage.id)
                        .ok()
                        .map(|o| ObservedRect {
                            visible_rect: o.visible_rect,
                            mode: o.mode,
                        }),
                });
            }

            if matches!(outcome, WindowOutcome::Rejected | WindowOutcome::Unknown) {
                pending.push(preimage.id.clone());
            }
        }

        self.outcomes.extend(outcomes.clone());

        if pending.is_empty() {
            if let Some(transaction) = self.transaction.take() {
                let _ = transaction.journal.close_and_remove();
            }
            self.preimages.clear();
            self.state = EngineState::Idle;
        } else {
            self.state = EngineState::Degraded;
        }
        tracing::info!(
            transaction = %transaction_id,
            restored = outcomes
                .values()
                .filter(|o| **o == WindowOutcome::Restored)
                .count(),
            kept_user_change = outcomes
                .values()
                .filter(|o| **o == WindowOutcome::KeptUserChange)
                .count(),
            fallback = outcomes
                .values()
                .filter(|o| **o == WindowOutcome::FallbackVisible)
                .count(),
            pending = pending.len(),
            state = ?self.state,
            "restore finished"
        );

        Ok(RestoreReport {
            transaction: Some(transaction_id),
            outcomes,
            pending,
        })
    }

    fn restore_one(&self, preimage: &Preimage, displays: &[DisplaySnapshot]) -> WindowOutcome {
        let current = match self.backend.read_back(&preimage.id) {
            Ok(observed) => observed,
            Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
            Err(_) => return WindowOutcome::Unknown,
        };

        // If the user moved the window after we placed it, keep their position.
        if let Some(transaction) = self.transaction.as_ref() {
            if let Some(last) = transaction.last_applied.get(&preimage.id) {
                if !within_tolerance(
                    current.visible_rect,
                    last.visible_rect,
                    self.config.tolerance_px,
                ) {
                    // Only keep the user's position if it is reachable; an
                    // off-screen window must still be rescued.
                    let reachable = displays.iter().any(|d| {
                        current.visible_rect.reachable_in(
                            &d.work_area,
                            self.config.min_reachable.width,
                            self.config.min_reachable.height,
                        )
                    });
                    if reachable {
                        return WindowOutcome::KeptUserChange;
                    }
                }
            }
        }

        let original_display = displays
            .iter()
            .find(|d| d.key.native_id == preimage.display_native_id);

        let target = match original_display {
            Some(display) if display.key.quality.is_restorable() => {
                if same_geometry(&display.work_area, &preimage.display_work_area) {
                    preimage.visible_rect
                } else {
                    rescale_between(
                        preimage.visible_rect,
                        preimage.display_work_area,
                        preimage.display_scale,
                        display.work_area,
                        display.scale,
                    )
                }
            }
            _ => {
                // Original display gone or identity too weak: put it in a
                // visible place rather than at a coordinate that no longer
                // exists.
                let Some(fallback) = displays.iter().find(|d| d.primary).or(displays.first())
                else {
                    return WindowOutcome::Unknown;
                };
                let rect = preimage.visible_rect.clamp_into(&fallback.work_area);
                let plan = WindowPlan {
                    visible_rect: rect,
                    mode: preimage.mode,
                };
                return match self.backend.apply(&preimage.id, &plan) {
                    Ok(()) => WindowOutcome::FallbackVisible,
                    Err(PlatformError::WindowGone) => WindowOutcome::Gone,
                    Err(_) => WindowOutcome::Unknown,
                };
            }
        };

        let plan = WindowPlan {
            visible_rect: target,
            mode: preimage.mode,
        };
        match self.apply_with_retry(&preimage.id, &plan) {
            WindowOutcome::Applied => WindowOutcome::Restored,
            other => other,
        }
    }

    /// Find windows that are not reachable in the current topology and move
    /// them into a visible work area.
    ///
    /// This is the only operation that works without a transaction: it is the
    /// safety net for windows left off-screen by a display change, another tool,
    /// or a previous crash.
    pub fn rescue_offscreen(&mut self) -> Result<Vec<(WindowId, Rect)>, EngineError> {
        let displays = self
            .backend
            .topology()
            .map_err(|e| EngineError::Platform(e.to_string()))?;
        let windows = self
            .backend
            .enumerate()
            .map_err(|e| EngineError::Platform(e.to_string()))?;

        let mut rescued = Vec::new();
        for window in windows {
            if !window.is_migratable() {
                continue;
            }
            let reachable = displays.iter().any(|d| {
                window.visible_rect.reachable_in(
                    &d.work_area,
                    self.config.min_reachable.width,
                    self.config.min_reachable.height,
                )
            });
            if reachable {
                continue;
            }

            let Some(fallback) = displays.iter().find(|d| d.primary).or(displays.first()) else {
                continue;
            };
            let rect = window.visible_rect.clamp_into(&fallback.work_area);
            let plan = WindowPlan {
                visible_rect: rect,
                mode: window.mode,
            };
            if self.backend.apply(&window.id, &plan).is_ok() {
                rescued.push((window.id, rect));
            }
        }
        Ok(rescued)
    }

    /// Commit the current transaction, leaving windows where they are.
    ///
    /// Used when the user explicitly chooses "keep this layout". The journal is
    /// closed so the next launch does not try to restore it.
    /// Re-plan the current set of windows onto `target`, keeping the original
    /// preimages.
    ///
    /// This is what "re-gather to a different display" means to a user: the
    /// windows are already gathered, and the target display changed (a typo, a
    /// second remote session, a monitor re-arranged). Doing it as `restore` then
    /// `migrate` would work but would throw away the preimages and record the
    /// restored positions as the new home, so a later restore could land nowhere
    /// near where the user started.
    ///
    /// The implementable version runs the restore first (to get the windows back
    /// to their original rectangles), then a fresh gather. Window positions, not
    /// the journal, are what the planner reads, so the intermediate state has to
    /// exist. If the restore leaves windows pending, retargeting is refused:
    /// planning on top of half-restored windows would produce a layout the user
    /// cannot reason about.
    pub fn retarget(&mut self, target: &DisplayKey) -> Result<RetargetReport, EngineError> {
        if self.state.may_write() {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        if self.paused {
            return Err(EngineError::Paused);
        }

        // Idle with no transaction: nothing to restore, so this is a plain
        // gather.
        if self.transaction.is_none() {
            let migration = self.migrate(target)?;
            return Ok(RetargetReport {
                restored: 0,
                migration,
            });
        }

        let restore = self.restore()?;
        if !restore.is_complete() {
            // Leave the engine in whatever state restore produced (Degraded).
            // The caller reports the pending count so the UI can offer "try
            // again" instead of pretending the retarget finished.
            return Err(EngineError::RestoreIncomplete {
                pending: restore.pending.len(),
            });
        }

        let migration = self.migrate(target)?;
        Ok(RetargetReport {
            restored: restore
                .outcomes
                .values()
                .filter(|o| **o == WindowOutcome::Restored)
                .count(),
            migration,
        })
    }

    pub fn keep_current_layout(&mut self) -> Result<(), EngineError> {
        if let Some(transaction) = self.transaction.take() {
            transaction
                .journal
                .close_and_remove()
                .map_err(|e| EngineError::JournalWrite(e.to_string()))?;
        }
        self.preimages.clear();
        self.state = EngineState::Idle;
        Ok(())
    }
}

fn within_tolerance(a: Rect, b: Rect, tolerance: i32) -> bool {
    (a.left - b.left).abs() <= tolerance
        && (a.top - b.top).abs() <= tolerance
        && (a.width() - b.width()).abs() <= tolerance
        && (a.height() - b.height()).abs() <= tolerance
}

fn same_geometry(a: &Rect, b: &Rect) -> bool {
    a == b
}

/// Convenience for callers that want to know whether a display identity is good
/// enough to restore without asking the user.
pub fn is_restorable(quality: IdentityQuality) -> bool {
    quality.is_restorable()
}

/// Report of a recovered transaction, for the UI's "unfinished work" screen.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingWork {
    pub transaction: uuid::Uuid,
    pub windows: usize,
    pub may_have_moved: bool,
    pub truncated_bytes: u64,
}

impl PendingWork {
    pub fn from_recovered(tx: &RecoveredTransaction) -> Self {
        Self {
            transaction: tx.transaction,
            windows: tx.preimages.len(),
            may_have_moved: tx.may_have_moved_windows() || tx.has_unresolved_intent(),
            truncated_bytes: tx.truncated_bytes,
        }
    }
}

#[allow(dead_code)]
fn unused_capabilities(_: Capabilities, _: Unplaced) {}
