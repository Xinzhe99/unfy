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
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub layout: LayoutConfig,
    /// How far a window may drift from the requested rectangle before the engine
    /// retries. Four physical pixels matches the product spec's tolerance.
    pub tolerance_px: i32,
    /// Attempts per window before it is reported as `Rejected`.
    pub max_attempts: u8,
    /// Where journal files live. One file per transaction.
    pub journal_dir: PathBuf,
    /// A window narrower than this is not considered "reachable".
    pub min_reachable: conflux_domain::Size,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            layout: LayoutConfig::default(),
            tolerance_px: 4,
            max_attempts: 3,
            journal_dir: std::env::temp_dir().join("conflux-journal"),
            min_reachable: conflux_domain::Size::new(64, 32),
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
        }
    }

    pub fn state(&self) -> EngineState {
        self.state
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn backend(&self) -> &B {
        &self.backend
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

        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
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
            if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
                newest = Some((modified, path));
            }
        }

        let Some((_, path)) = newest else {
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

        let preview = self.build_preview(target)?;
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
        let mut applied = Vec::new();
        let mut unplaced = Vec::new();
        for id in preview.plan.unplaced.iter().map(|(id, _)| id.clone()) {
            unplaced.push(id);
        }

        for placement in &preview.plan.placements {
            let Some(transaction) = self.transaction.as_mut() else {
                break;
            };
            if transaction
                .journal
                .write_intent(&placement.id, placement.rect, WindowMode::Normal)
                .is_err()
            {
                // Cannot record the intent, so we must not perform the write.
                break;
            }

            let plan = WindowPlan {
                visible_rect: placement.rect,
                mode: WindowMode::Normal,
            };

            let outcome = self.apply_with_retry(&placement.id, &plan);
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
            if outcome == WindowOutcome::Applied {
                applied.push(placement.id.clone());
            }
        }

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

    /// Apply one window, reading back and retrying within the tolerance.
    fn apply_with_retry(&self, id: &WindowId, plan: &WindowPlan) -> WindowOutcome {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.backend.apply(id, plan) {
                Ok(()) => {}
                Err(PlatformError::PermissionDenied) | Err(PlatformError::Unsupported) => {
                    return WindowOutcome::Rejected
                }
                Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
                Err(_) => {
                    if attempt >= self.config.max_attempts {
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
                    if attempt >= self.config.max_attempts {
                        return WindowOutcome::Rejected;
                    }
                    // A short pause lets a self-correcting application settle
                    // before the next attempt instead of racing it.
                    std::thread::sleep(std::time::Duration::from_millis(60));
                }
                Err(PlatformError::WindowGone) => return WindowOutcome::Gone,
                Err(_) => {
                    if attempt >= self.config.max_attempts {
                        return WindowOutcome::Unknown;
                    }
                }
            }
        }
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

        self.state = EngineState::Restoring;

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
