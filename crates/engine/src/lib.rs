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
//! 5. A read (preview, status) never mutates engine state.
//!
//! # Why the engine is generic
//!
//! `Engine<B: Backend>` takes the platform behind a trait, so the entire state
//! machine runs against the fake backend in tests. The Windows and macOS
//! backends only supply enumeration, apply and read-back.

use std::collections::HashMap;
use std::path::PathBuf;

pub use unfy_domain::EngineState;

use unfy_domain::{
    rescale_between, DisplayKey, DisplaySnapshot, EngineError, IdentityQuality, Rect, SkipReason,
    WindowId, WindowMode, WindowOutcome, WindowSnapshot,
};
use unfy_journal::{
    AppliedResult, Journal, ObservedRect, Preimage, Recovered, RecoveredTransaction,
};
use unfy_layout::{Candidate, LayoutConfig, Plan, Unplaced};
use unfy_platform::{
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
    pub min_size: Option<unfy_domain::Size>,
    pub focused: bool,
    pub capabilities: Capabilities,
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
    /// Windows that were already usable on the target and were not written to.
    pub unchanged: usize,
    /// Windows the engine decided not to touch because their preimage could not
    /// be captured. They received zero platform writes.
    pub not_attempted: Vec<WindowId>,
    pub cascaded: bool,
    /// Windows the application refused to move, with the process to name.
    pub refused: Vec<RefusedWindow>,
}

/// Result of a retarget: the windows that were re-planned onto a new target.
#[derive(Debug, Clone, PartialEq)]
pub struct RetargetReport {
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

    /// Windows that still need attention, including those never attempted.
    pub fn unfinished(&self) -> usize {
        self.failed_count() + self.not_attempted.len()
    }
}

/// Summary of a restore pass.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreReport {
    pub transaction: Option<uuid::Uuid>,
    pub outcomes: HashMap<WindowId, WindowOutcome>,
    /// Windows that could not be restored and still owe a restore.
    pub pending: Vec<WindowId>,
    /// Windows placed in a visible area because their original display is gone.
    /// They are safe to use but their original position is still recorded, so a
    /// later restore can finish the job once the display returns.
    pub awaiting_original: Vec<WindowId>,
}

impl RestoreReport {
    /// Whether the transaction is fully resolved and can be closed.
    pub fn is_complete(&self) -> bool {
        self.pending.is_empty() && self.awaiting_original.is_empty()
    }

    pub fn count(&self, outcome: WindowOutcome) -> usize {
        self.outcomes.values().filter(|o| **o == outcome).count()
    }

    /// Windows that still hold a recovery obligation.
    pub fn outstanding(&self) -> usize {
        self.pending.len() + self.awaiting_original.len()
    }
}

/// Configuration the engine needs from settings.
#[derive(Clone)]
pub struct EngineConfig {
    pub layout: LayoutConfig,
    /// How far a window may drift from the requested rectangle before the engine
    /// retries.
    pub tolerance_px: i32,
    /// Attempts per window before it is reported as `Rejected`.
    pub max_attempts: u8,
    /// Pause before re-requesting a window whose position did not stick.
    pub retry_delay_ms: u64,
    /// Where journal files live. One file per transaction.
    pub journal_dir: PathBuf,
    /// A window narrower than this is not considered "reachable".
    pub min_reachable: unfy_domain::Size,
    /// Advisory restore delay, for diagnostics. The engine never sleeps on it.
    pub restore_delay_secs: u32,
    /// Called after each window has been submitted, with (done, total).
    pub on_progress: Option<std::sync::Arc<dyn Fn(usize, usize) + Send + Sync>>,
    /// Executable names that are never moved by a gather or rescue.
    ///
    /// Never blocks restoring: an excluded application must not be stranded just
    /// because the user asked not to have it rearranged.
    pub excluded_apps: Vec<String>,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            journal_dir: std::env::temp_dir().join("unfy-journal"),
            min_reachable: unfy_domain::Size::new(64, 32),
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
    /// First preimages captured for the current transaction, keyed by window.
    /// Immutable for the lifetime of the transaction: a retarget, a second
    /// gather or a rescue never overwrite the position the user started from.
    preimages: HashMap<WindowId, Preimage>,
    /// Windows whose obligation was already settled in this process. Kept out of
    /// the next restore pass.
    resolved: HashMap<WindowId, WindowOutcome>,
    /// Set by `pause()`; blocks new gathers without discarding the layout.
    paused: bool,
}

struct Transaction {
    id: uuid::Uuid,
    journal: Journal,
    /// The rectangle and mode each window was last placed at by us, used to tell
    /// a user move apart from our own write.
    last_applied: HashMap<WindowId, ObservedWindow>,
}

/// What `classify` decided about one enumerated window.
struct Classified {
    snapshot: WindowSnapshot,
    display: DisplaySnapshot,
    min_size: Option<unfy_domain::Size>,
    focused: bool,
    capabilities: Capabilities,
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
            resolved: HashMap::new(),
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
    /// the journal disagree with what the user saw in the preview.
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
    /// Returns the configured display when it is still present, otherwise `None`.
    /// Callers that need a fallback must ask the user: silently choosing the
    /// primary display gathers windows onto a screen the remote client may not be
    /// showing, which is worse than doing nothing.
    pub fn resolve_target(&self, configured_native_id: Option<&str>) -> Option<DisplayKey> {
        let displays = self.backend.topology().ok()?;
        let native_id = configured_native_id?;
        displays
            .iter()
            .find(|d| d.key.native_id == native_id)
            .map(|d| d.key.clone())
    }

    /// How many windows the current transaction has captured.
    pub fn captured_window_count(&self) -> usize {
        self.preimages.len()
    }

    /// How many of those still owe a restore.
    pub fn unresolved_window_count(&self) -> usize {
        self.preimages
            .keys()
            .filter(|id| !self.resolved.contains_key(*id))
            .count()
    }

    /// The transaction this engine currently owns, if any.
    pub fn current_transaction(&self) -> Option<uuid::Uuid> {
        self.transaction.as_ref().map(|t| t.id)
    }

    /// A cheap fingerprint of the current display topology.
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
    pub fn pause(&mut self) -> Result<(), EngineError> {
        if self.state == EngineState::Migrating || self.state == EngineState::Restoring {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        self.paused = true;
        tracing::info!("engine paused; layout and journal retained");
        Ok(())
    }

    /// Resume acting on gather requests.
    ///
    /// Pause is independent of whether a transaction exists, and resuming never
    /// clears an error state: an unresolved journal must stay visible.
    pub fn resume(&mut self) -> Result<(), EngineError> {
        if !self.paused {
            return Ok(());
        }
        self.paused = false;
        if self.transaction.is_some() && self.state != EngineState::Degraded {
            self.state = EngineState::Active;
        }
        tracing::info!(state = ?self.state, "engine resumed");
        Ok(())
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Adopt a journal left behind by a previous run.
    ///
    /// Must be called before the engine is allowed to migrate, and before the UI
    /// offers "restore". Only unresolved windows are loaded: a window that was
    /// already put back must not be moved again because the process restarted.
    pub fn recover(&mut self, path: impl AsRef<std::path::Path>) -> Result<Recovered, EngineError> {
        let path = path.as_ref();
        let recovered = match unfy_journal::recover(path) {
            Ok(recovered) => recovered,
            Err(e) => {
                self.state = EngineState::Error;
                return Err(EngineError::JournalCorrupt(e.to_string()));
            }
        };

        let Recovered::Unfinished(tx) = &recovered else {
            return Ok(recovered);
        };

        if !tx.has_open_obligation() {
            // Every captured window already reached a terminal state. The file is
            // only history; remove it so the next launch does not report work that
            // no longer exists.
            std::fs::remove_file(path).ok();
            self.state = EngineState::Idle;
            return Ok(Recovered::Clean);
        }

        let journal = match Journal::reopen(path, tx.transaction) {
            Ok(journal) => journal,
            Err(e) => {
                self.state = EngineState::Error;
                return Err(EngineError::JournalCorrupt(e.to_string()));
            }
        };

        self.preimages = tx
            .preimages
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        self.resolved = tx.resolved.iter().cloned().collect();
        self.transaction = Some(Transaction {
            id: tx.transaction,
            journal,
            last_applied: HashMap::new(),
        });
        self.state = EngineState::Degraded;

        tracing::warn!(
            transaction = %tx.transaction,
            preimages = tx.preimages.len(),
            resolved = tx.resolved.len(),
            may_have_moved = tx.may_have_moved_windows(),
            "adopted unfinished transaction"
        );
        Ok(recovered)
    }

    /// Adopt the unfinished transaction in `dir`, if any.
    ///
    /// Every journal is examined. A file is never deleted because a newer one
    /// exists: two files can describe two different sets of displaced windows, and
    /// the modification time says nothing about whether their obligations were
    /// settled.
    pub fn recover_latest(
        &mut self,
        dir: impl AsRef<std::path::Path>,
    ) -> Result<Option<uuid::Uuid>, EngineError> {
        let dir = dir.as_ref();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(None);
        };

        // Examine every journal. A file is never deleted because a newer one
        // exists: two files can describe two different sets of displaced windows,
        // and the modification time says nothing about whether their obligations
        // were settled. Deleting a superseded journal was the reason a crashed
        // gather could silently lose the only record of where its windows were.
        let mut unfinished: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        let mut failures = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "journal") {
                continue;
            }
            match unfy_journal::recover(&path) {
                Ok(Recovered::Clean) => {
                    if std::fs::metadata(&path)
                        .map(|m| m.len() > 0)
                        .unwrap_or(false)
                    {
                        if let Err(e) = std::fs::remove_file(&path) {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "could not remove a closed journal"
                            );
                        }
                    }
                }
                Ok(Recovered::Unfinished(tx)) => {
                    if !tx.has_open_obligation() {
                        std::fs::remove_file(&path).ok();
                        continue;
                    }
                    let at = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    unfinished.push((at, path));
                }
                Err(e) => failures.push(format!("{}: {e}", path.display())),
            }
        }

        if !failures.is_empty() {
            self.state = EngineState::Error;
            return Err(EngineError::JournalCorrupt(failures.join("; ")));
        }

        unfinished.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        let Some((_, newest)) = unfinished.first().cloned() else {
            return Ok(None);
        };
        if unfinished.len() > 1 {
            tracing::warn!(
                count = unfinished.len() - 1,
                "more than one unfinished journal; the others are kept and will be adopted on a later launch"
            );
        }

        match self.recover(&newest)? {
            Recovered::Unfinished(tx) => Ok(Some(tx.transaction)),
            Recovered::Clean => Ok(None),
        }
    }

    /// Enumerate, filter and plan without touching a window or the engine state.
    ///
    /// This used to set `Planning` and then unconditionally return to `Idle`. A
    /// preview taken while a transaction was open therefore erased `Active`,
    /// `Degraded` and `Error`, and the UI lost its restore entry point while the
    /// windows were still displaced. A read must not mutate state.
    pub fn preview(&self, target: &DisplayKey) -> Result<Preview, EngineError> {
        self.build_preview(target)
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
        let mut candidates = Vec::new();
        for window in windows {
            match self.classify(&window, &displays, target) {
                Ok(Some(classified)) => {
                    let on_target = classified.display.key == *target;
                    candidates.push(Candidate {
                        id: classified.snapshot.id.clone(),
                        current: classified.snapshot.visible_rect,
                        min_size: classified.min_size,
                        focused: classified.focused,
                        source_work: classified.display.work_area,
                        source_scale: classified.display.scale.x,
                        already_on_target: on_target
                            && unfy_layout::is_usable_on_target(
                                classified.snapshot.visible_rect,
                                target_display.work_area,
                            ),
                        can_resize: classified.capabilities.can_resize,
                        can_move: classified.capabilities.can_move,
                    });
                    eligible.push(classified.snapshot.id.clone());
                }
                Ok(None) => {}
                Err(reason) => skipped.push(SkippedWindow {
                    id: window.id.clone(),
                    app_name: window.app_name.clone(),
                    reason,
                }),
            }
        }

        let plan = unfy_layout::plan(
            &candidates,
            target_display.work_area,
            target_display.scale.x,
            &self.config.layout,
        );

        Ok(Preview {
            target: target.clone(),
            plan,
            eligible,
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
        target: &DisplayKey,
    ) -> Result<Option<Classified>, SkipReason> {
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

        // Resolve by geometry, not by the snapshot's `display` field: the Windows
        // backend cannot fill that field during enumeration without a second
        // round trip per window, and a stale value would make "already on target"
        // wrong in both directions.
        let display = displays
            .iter()
            .max_by_key(|d| d.bounds.intersection_area(&window.visible_rect))
            .filter(|d| d.bounds.intersection_area(&window.visible_rect) > 0)
            .or_else(|| {
                displays
                    .iter()
                    .find(|d| d.key == window.display || d.key == *target)
            })
            .ok_or(SkipReason::Invisible)?;

        Ok(Some(Classified {
            snapshot: window.clone(),
            display: display.clone(),
            min_size: capabilities.min_size,
            focused: window.focused,
            capabilities,
        }))
    }

    /// Move every eligible window onto `target`, starting a new transaction.
    pub fn migrate(&mut self, target: &DisplayKey) -> Result<MigrationReport, EngineError> {
        self.gather(target, false)
    }

    /// Bring newly appeared windows into an existing transaction, keeping every
    /// first preimage already captured.
    pub fn gather_new(&mut self, target: &DisplayKey) -> Result<MigrationReport, EngineError> {
        self.gather(target, true)
    }

    /// Re-plan the captured windows onto a different target.
    ///
    /// The original preimages are kept: this is what "the target display changed"
    /// means, not "start over from where the windows are now". The old
    /// implementation ran a full restore first, which recorded the restored
    /// positions as the new home and could overwrite the real baseline when the
    /// original display was missing.
    pub fn retarget(&mut self, target: &DisplayKey) -> Result<RetargetReport, EngineError> {
        if self.transaction.is_none() {
            return Ok(RetargetReport {
                migration: self.gather(target, false)?,
            });
        }
        Ok(RetargetReport {
            migration: self.gather(target, true)?,
        })
    }

    fn gather(
        &mut self,
        target: &DisplayKey,
        allow_existing: bool,
    ) -> Result<MigrationReport, EngineError> {
        if self.state.may_write() {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        if self.state == EngineState::Error {
            return Err(EngineError::InvalidTransition { from: self.state });
        }
        if self.paused {
            return Err(EngineError::Paused);
        }
        if !allow_existing && self.transaction.is_some() {
            return Err(EngineError::TransactionAlreadyOpen);
        }
        // With an open transaction, every captured window already has a preimage.
        // A window whose obligation is still open must not be re-planned without
        // its original baseline, so retargeting is allowed; a second *fresh*
        // gather is not.

        let t0 = std::time::Instant::now();
        let preview = self.build_preview(target)?;
        let t_preview = t0.elapsed().as_millis();

        let mut unplaced: Vec<WindowId> = preview
            .plan
            .unplaced
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        let mut not_attempted: Vec<WindowId> = Vec::new();

        // Only windows that need a write, and whose preimage is not already
        // captured, are candidates for this pass.
        let placements: Vec<unfy_layout::Placement> = preview
            .plan
            .placements
            .iter()
            .filter(|p| !p.unchanged)
            .cloned()
            .collect();
        let unchanged = preview.plan.placements.len() - placements.len();

        if placements.is_empty() {
            if self.transaction.is_none() {
                self.state = EngineState::Idle;
            }
            return Ok(MigrationReport {
                transaction: self
                    .transaction
                    .as_ref()
                    .map(|t| t.id)
                    .unwrap_or(uuid::Uuid::nil()),
                applied: Vec::new(),
                outcomes: self.outcomes.clone(),
                skipped: preview.skipped,
                unplaced,
                unchanged,
                not_attempted,
                cascaded: preview.plan.cascaded,
                refused: Vec::new(),
            });
        }

        // 1. Open a transaction if this is a fresh gather.
        let mut started_here = false;
        if self.transaction.is_none() {
            let transaction_id = uuid::Uuid::new_v4();
            let journal_path = self
                .config
                .journal_dir
                .join(format!("{transaction_id}.journal"));
            let journal = Journal::begin_named(
                &journal_path,
                transaction_id,
                &format!(
                    "target={} windows={}",
                    preview.target.native_id,
                    placements.len()
                ),
            )
            .map_err(|e| {
                self.state = EngineState::Error;
                EngineError::JournalWrite(e.to_string())
            })?;
            self.outcomes.clear();
            self.resolved.clear();
            self.transaction = Some(Transaction {
                id: journal.transaction(),
                journal,
                last_applied: HashMap::new(),
            });
            started_here = true;
        }
        self.state = EngineState::Migrating;

        // 2. Capture the first preimage of every window this pass will touch.
        //    A failure here removes the window from the commit set: no preimage,
        //    no write.
        let t1 = std::time::Instant::now();
        let mut captured_now = 0usize;
        for placement in &placements {
            if self.preimages.contains_key(&placement.id) {
                continue;
            }
            match self.capture_preimage(&placement.id) {
                Ok(preimage) => {
                    if let Some(transaction) = self.transaction.as_mut() {
                        if let Err(e) = transaction.journal.write_preimage(&preimage) {
                            self.state = EngineState::Error;
                            return Err(EngineError::JournalWrite(e.to_string()));
                        }
                    }
                    self.preimages.insert(placement.id.clone(), preimage);
                    captured_now += 1;
                }
                Err(outcome) => {
                    not_attempted.push(placement.id.clone());
                    self.outcomes.insert(placement.id.clone(), outcome);
                }
            }
        }

        // 3. The durability barrier. Without it the preimages are only in the page
        //    cache and a crash could lose them after the windows moved.
        if let Some(transaction) = self.transaction.as_mut() {
            if let Err(e) = transaction.journal.durable() {
                self.state = EngineState::Error;
                return Err(EngineError::JournalWrite(e.to_string()));
            }
        }
        let t_preimage = t1.elapsed().as_millis();

        // Nothing captured at all: no window may be written, so close the journal
        // we may have just created and report honestly.
        if self.preimages.is_empty() {
            if started_here {
                if let Some(transaction) = self.transaction.take() {
                    let _ = transaction.journal.close_and_remove();
                }
                self.state = EngineState::Idle;
            }
            return Ok(MigrationReport {
                transaction: uuid::Uuid::nil(),
                applied: Vec::new(),
                outcomes: self.outcomes.clone(),
                skipped: preview.skipped,
                unplaced,
                unchanged,
                not_attempted,
                cascaded: preview.plan.cascaded,
                refused: Vec::new(),
            });
        }

        // 4. Commit only prepared windows.
        let prepared: Vec<unfy_layout::Placement> = placements
            .into_iter()
            .filter(|p| self.preimages.contains_key(&p.id))
            .collect();

        let mut applied = Vec::new();
        let mut settling: Vec<(WindowId, WindowPlan)> = Vec::new();

        const BATCH: usize = 16;
        let mut next = 0usize;
        while next < prepared.len() {
            let end = (next + BATCH).min(prepared.len());
            let batch = &prepared[next..end];

            let mut wrote_all = true;
            if let Some(transaction) = self.transaction.as_mut() {
                for placement in batch {
                    let mode = self
                        .preimages
                        .get(&placement.id)
                        .map(|p| p.mode)
                        .unwrap_or(WindowMode::Normal);
                    if transaction
                        .journal
                        .write_intent(&placement.id, placement.rect, mode)
                        .is_err()
                    {
                        wrote_all = false;
                        break;
                    }
                }
            }
            let durable = self
                .transaction
                .as_mut()
                .map(|t| t.journal.durable().is_ok())
                .unwrap_or(false);
            if !wrote_all || !durable {
                self.state = EngineState::Error;
                return Err(EngineError::JournalWrite(
                    "could not persist the move intent; no further windows were touched"
                        .to_string(),
                ));
            }

            for placement in batch {
                let mode = self
                    .preimages
                    .get(&placement.id)
                    .map(|p| p.mode)
                    .unwrap_or(WindowMode::Normal);
                let plan = WindowPlan {
                    visible_rect: placement.rect,
                    mode,
                };
                let outcome = self.apply_once(&placement.id, &plan);
                if outcome == WindowOutcome::Unknown {
                    settling.push((placement.id.clone(), plan));
                }
                let observed = self.backend.read_back(&placement.id).ok();
                if let Err(e) = self.record_result(&placement.id, outcome, observed) {
                    self.state = EngineState::Error;
                    return Err(e);
                }
                if let Some(progress) = self.config.on_progress.as_ref() {
                    progress(self.outcomes.len(), prepared.len());
                }
                if outcome == WindowOutcome::Applied {
                    applied.push(placement.id.clone());
                }
            }
            next = end;
        }

        let t_apply = t1.elapsed().as_millis().saturating_sub(t_preimage);

        // 5. One retry pass for windows that had not settled.
        for (id, plan) in settling {
            let outcome = self.apply_with_retry(&id, &plan);
            let observed = self.backend.read_back(&id).ok();
            if let Err(e) = self.record_result(&id, outcome, observed) {
                self.state = EngineState::Error;
                return Err(e);
            }
            if outcome == WindowOutcome::Applied && !applied.contains(&id) {
                applied.push(id.clone());
            }
        }

        tracing::debug!(
            enumerate_and_plan_ms = t_preview,
            preimage_ms = t_preimage,
            apply_ms = t_apply,
            captured_now,
            "gather phase timings"
        );

        self.state = if applied.is_empty() {
            EngineState::Degraded
        } else {
            EngineState::Active
        };

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

        tracing::info!(
            transaction = %self.transaction.as_ref().map(|t| t.id).unwrap_or_default(),
            applied = applied.len(),
            unchanged,
            not_attempted = not_attempted.len(),
            skipped = preview.skipped.len(),
            state = ?self.state,
            "gather finished"
        );

        Ok(MigrationReport {
            transaction: self
                .transaction
                .as_ref()
                .map(|t| t.id)
                .unwrap_or(uuid::Uuid::nil()),
            applied,
            outcomes: self.outcomes.clone(),
            skipped: preview.skipped,
            unplaced: std::mem::take(&mut unplaced),
            unchanged,
            not_attempted,
            cascaded: preview.plan.cascaded,
            refused,
        })
    }

    /// Re-read one window and build the preimage the journal must hold before any
    /// write. `Err` carries the outcome to report for a window that cannot be
    /// snapshotted; such a window must receive zero platform writes.
    fn capture_preimage(&self, id: &WindowId) -> Result<Preimage, WindowOutcome> {
        let observed = match self.backend.read_back(id) {
            Ok(observed) => observed,
            Err(PlatformError::WindowGone) => return Err(WindowOutcome::Gone),
            Err(PlatformError::PermissionDenied) | Err(PlatformError::Unsupported) => {
                return Err(WindowOutcome::Skipped)
            }
            Err(_) => return Err(WindowOutcome::Unknown),
        };

        let windows = self
            .backend
            .enumerate()
            .map_err(|_| WindowOutcome::Unknown)?;
        let snapshot = windows
            .into_iter()
            .find(|w| w.id == *id)
            .ok_or(WindowOutcome::Gone)?;

        let displays = self
            .backend
            .topology()
            .map_err(|_| WindowOutcome::Unknown)?;
        let display = displays
            .iter()
            .max_by_key(|d| d.bounds.intersection_area(&observed.visible_rect))
            .or_else(|| displays.iter().find(|d| d.key == snapshot.display))
            .ok_or(WindowOutcome::Unknown)?;

        Ok(Preimage {
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
        })
    }

    /// Persist one result and update the last-write record the restore path uses
    /// to distinguish a user move from our own.
    fn record_result(
        &mut self,
        id: &WindowId,
        outcome: WindowOutcome,
        observed: Option<ObservedWindow>,
    ) -> Result<(), EngineError> {
        let record = AppliedResult {
            id: id.clone(),
            outcome,
            observed: observed.map(|o| ObservedRect {
                visible_rect: o.visible_rect,
                mode: o.mode,
            }),
        };
        if let Some(transaction) = self.transaction.as_mut() {
            transaction
                .journal
                .write_result(&record)
                .map_err(|e| EngineError::JournalWrite(e.to_string()))?;
            if let Some(observed) = observed {
                transaction.last_applied.insert(id.clone(), observed);
            }
        }
        self.outcomes.insert(id.clone(), outcome);
        Ok(())
    }

    /// Submit one window and read it back once, without sleeping.
    fn apply_once(&self, id: &WindowId, plan: &WindowPlan) -> WindowOutcome {
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
                if matches_plan(&observed, plan, self.config.tolerance_px) {
                    WindowOutcome::Applied
                } else {
                    WindowOutcome::Unknown
                }
            }
            Err(PlatformError::WindowGone) => WindowOutcome::Gone,
            Err(_) => WindowOutcome::Unknown,
        }
    }

    /// Put a rejected window back to the size it had before we touched it.
    fn revert_size(&self, id: &WindowId) {
        let Some(preimage) = self.preimages.get(id) else {
            return;
        };
        let Ok(observed) = self.backend.read_back(id) else {
            return;
        };
        if same_geometry(&observed.visible_rect, &preimage.visible_rect) {
            return;
        }
        let plan = WindowPlan {
            visible_rect: Rect::new(
                observed.visible_rect.left,
                observed.visible_rect.top,
                observed.visible_rect.left + preimage.visible_rect.width(),
                observed.visible_rect.top + preimage.visible_rect.height(),
            ),
            mode: preimage.mode,
        };
        if self.backend.apply(id, &plan).is_err() {
            tracing::debug!(window = ?id, "could not restore a rejected window's size");
        }
    }

    /// Retry a window that did not settle on the first pass.
    fn apply_with_retry(&self, id: &WindowId, plan: &WindowPlan) -> WindowOutcome {
        let outcome = self.apply_with_retry_inner(id, plan);
        if matches!(outcome, WindowOutcome::Rejected | WindowOutcome::Unknown) {
            self.revert_size(id);
        }
        outcome
    }

    fn apply_with_retry_inner(&self, id: &WindowId, plan: &WindowPlan) -> WindowOutcome {
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
                    if matches_plan(&observed, plan, self.config.tolerance_px) {
                        return WindowOutcome::Applied;
                    }
                    if attempt == self.config.max_attempts {
                        return WindowOutcome::Rejected;
                    }
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

    /// Restore every window captured by the current transaction.
    ///
    /// Windows the user moved or resized after we placed them are left alone; the
    /// outcome is `KeptUserChange`. Windows whose original display is gone are
    /// clamped into a visible work area and reported as `FallbackVisible`, while
    /// keeping their original position on record.
    pub fn restore(&mut self) -> Result<RestoreReport, EngineError> {
        if self.state == EngineState::Idle && self.transaction.is_none() {
            return Ok(RestoreReport {
                transaction: None,
                outcomes: HashMap::new(),
                pending: Vec::new(),
                awaiting_original: Vec::new(),
            });
        }
        if self.state == EngineState::Migrating || self.state == EngineState::Planning {
            return Err(EngineError::InvalidTransition { from: self.state });
        }

        self.state = EngineState::Restoring;

        let Some(transaction) = self.transaction.as_ref() else {
            self.state = EngineState::Idle;
            return Ok(RestoreReport {
                transaction: None,
                outcomes: HashMap::new(),
                pending: Vec::new(),
                awaiting_original: Vec::new(),
            });
        };
        let transaction_id = transaction.id;

        let displays = self.backend.topology().map_err(|e| {
            self.state = EngineState::Degraded;
            EngineError::Platform(e.to_string())
        })?;

        let preimages: Vec<Preimage> = self
            .preimages
            .iter()
            .filter(|(id, _)| !self.resolved.contains_key(*id))
            .map(|(_, p)| p.clone())
            .collect();
        let mut outcomes = HashMap::new();
        let mut pending = Vec::new();
        let mut awaiting_original = Vec::new();

        for preimage in preimages {
            let outcome = self.restore_one(&preimage, &displays);
            let observed = self.backend.read_back(&preimage.id).ok();
            let record = AppliedResult {
                id: preimage.id.clone(),
                outcome,
                observed: observed.map(|o| ObservedRect {
                    visible_rect: o.visible_rect,
                    mode: o.mode,
                }),
            };

            // A failed journal write stops the pass. Continuing would move more
            // windows without a durable record of what happened to them.
            if let Some(transaction) = self.transaction.as_mut() {
                if let Err(e) = transaction.journal.write_result(&record) {
                    self.state = EngineState::Error;
                    return Err(EngineError::JournalWrite(e.to_string()));
                }
                if let Some(observed) = observed {
                    transaction
                        .last_applied
                        .insert(preimage.id.clone(), observed);
                }
            }
            outcomes.insert(preimage.id.clone(), outcome);

            let settled = matches!(
                outcome,
                WindowOutcome::Restored
                    | WindowOutcome::KeptUserChange
                    | WindowOutcome::Gone
                    | WindowOutcome::Skipped
            );
            if settled {
                if let Err(e) = self.resolve(&preimage.id, outcome) {
                    self.state = EngineState::Error;
                    return Err(e);
                }
            } else if outcome == WindowOutcome::FallbackVisible {
                awaiting_original.push(preimage.id.clone());
            } else {
                pending.push(preimage.id.clone());
            }
        }

        self.outcomes.extend(outcomes.clone());

        if pending.is_empty() && awaiting_original.is_empty() {
            self.close_transaction()?;
            self.state = EngineState::Idle;
        } else {
            self.state = EngineState::Degraded;
        }

        tracing::info!(
            transaction = %transaction_id,
            restored = outcomes.values().filter(|o| **o == WindowOutcome::Restored).count(),
            kept_user_change = outcomes.values().filter(|o| **o == WindowOutcome::KeptUserChange).count(),
            fallback = outcomes.values().filter(|o| **o == WindowOutcome::FallbackVisible).count(),
            pending = pending.len(),
            awaiting_original = awaiting_original.len(),
            state = ?self.state,
            "restore finished"
        );

        Ok(RestoreReport {
            transaction: Some(transaction_id),
            outcomes,
            pending,
            awaiting_original,
        })
    }

    /// Persist that one window owes nothing further, then drop its obligation from
    /// memory. The journal record comes first: a crash between the two must not
    /// resurrect a window that was already put back.
    fn resolve(&mut self, id: &WindowId, outcome: WindowOutcome) -> Result<(), EngineError> {
        if let Some(transaction) = self.transaction.as_mut() {
            transaction
                .journal
                .write_resolved(id, outcome)
                .map_err(|e| EngineError::JournalWrite(e.to_string()))?;
            transaction
                .journal
                .durable()
                .map_err(|e| EngineError::JournalWrite(e.to_string()))?;
        }
        self.resolved.insert(id.clone(), outcome);
        Ok(())
    }

    fn close_transaction(&mut self) -> Result<(), EngineError> {
        let Some(transaction) = self.transaction.take() else {
            self.preimages.clear();
            self.resolved.clear();
            return Ok(());
        };
        // `close_and_remove` writes Closed and fsyncs before deleting. If the
        // close fails the file stays and the caller must keep the obligations, so
        // the transaction is put back rather than dropped.
        match transaction.journal.close_and_remove() {
            Ok(()) => {
                self.preimages.clear();
                self.resolved.clear();
                Ok(())
            }
            Err(e) => {
                self.state = EngineState::Error;
                Err(EngineError::JournalWrite(e.to_string()))
            }
        }
    }

    fn restore_one(&self, preimage: &Preimage, displays: &[DisplaySnapshot]) -> WindowOutcome {
        let current = match self.backend.read_back(&preimage.id) {
            Ok(observed) => observed,
            Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
            Err(PlatformError::PermissionDenied) | Err(PlatformError::Unsupported) => {
                return WindowOutcome::Skipped
            }
            Err(_) => return WindowOutcome::Unknown,
        };

        // If the user changed the window after we placed it, keep their change.
        // Both the rectangle and the mode matter: maximising a window we tiled is
        // an external change even though the rectangle may coincide.
        if let Some(transaction) = self.transaction.as_ref() {
            if let Some(last) = transaction.last_applied.get(&preimage.id) {
                let moved = !within_tolerance(
                    current.visible_rect,
                    last.visible_rect,
                    self.config.tolerance_px,
                );
                let restated = current.mode != last.mode;
                if moved || restated {
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
                // Original display gone or identity too weak: put it in a visible
                // place rather than at a coordinate that no longer exists, and
                // keep the original baseline so a later restore can finish.
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
                    Err(PlatformError::PermissionDenied) | Err(PlatformError::Unsupported) => {
                        WindowOutcome::Skipped
                    }
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
            if self.is_excluded(&window) {
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

            // A rescue of a window this transaction already moved is still one of
            // our writes. Recording it prevents the next restore from mistaking it
            // for a user drag.
            if self.preimages.contains_key(&window.id) {
                if let Some(transaction) = self.transaction.as_mut() {
                    if transaction
                        .journal
                        .write_intent(&window.id, rect, window.mode)
                        .is_err()
                    {
                        continue;
                    }
                    if transaction.journal.durable().is_err() {
                        continue;
                    }
                }
            }

            match self.backend.apply(&window.id, &plan) {
                Ok(()) => {
                    if self.preimages.contains_key(&window.id) {
                        if let Ok(observed) = self.backend.read_back(&window.id) {
                            if let Some(transaction) = self.transaction.as_mut() {
                                transaction.last_applied.insert(window.id.clone(), observed);
                                let _ = transaction.journal.write_result(&AppliedResult {
                                    id: window.id.clone(),
                                    outcome: WindowOutcome::FallbackVisible,
                                    observed: Some(ObservedRect {
                                        visible_rect: observed.visible_rect,
                                        mode: observed.mode,
                                    }),
                                });
                            }
                        }
                    }
                    rescued.push((window.id, rect));
                }
                Err(_) => continue,
            }
        }
        Ok(rescued)
    }

    /// Commit the current transaction, leaving windows where they are.
    ///
    /// Used when the user explicitly chooses "keep this layout". Every window is
    /// resolved in the journal *before* the file is closed, so a crash mid-way
    /// cannot reopen windows the user had accepted.
    pub fn keep_current_layout(&mut self) -> Result<(), EngineError> {
        if self.transaction.is_none() {
            self.preimages.clear();
            self.resolved.clear();
            self.state = EngineState::Idle;
            return Ok(());
        }
        let ids: Vec<WindowId> = self
            .preimages
            .keys()
            .filter(|id| !self.resolved.contains_key(*id))
            .cloned()
            .collect();
        for id in ids {
            self.resolve(&id, WindowOutcome::KeptUserChange)?;
        }
        self.close_transaction()?;
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

/// Whether an observation satisfies a plan.
///
/// The mode is part of the contract: a window that ends up maximized after being
/// asked to tile covers every other window on the target, so reporting success on
/// the rectangle alone would be a lie the user can see.
fn matches_plan(observed: &ObservedWindow, plan: &WindowPlan, tolerance: i32) -> bool {
    within_tolerance(observed.visible_rect, plan.visible_rect, tolerance)
        && observed.mode == plan.mode
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
            windows: tx.touched_windows().len(),
            may_have_moved: tx.may_have_moved_windows() || tx.has_unresolved_intent(),
            truncated_bytes: tx.truncated_bytes,
        }
    }
}

#[allow(dead_code)]
fn unused_capabilities(_: Capabilities, _: Unplaced) {}
