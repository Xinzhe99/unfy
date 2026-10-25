//! Layout planning: pure functions from a window set to target rectangles.
//!
//! Nothing here touches an operating system. Given the same inputs the output is
//! byte-identical, which is what makes the plan preview trustworthy: the user
//! sees exactly what will be requested.
//!
//! # The two modes
//!
//! * `MoveInto` (the default) does not arrange anything. A window already on the
//!   target display is left untouched; any other window keeps its logical size and
//!   is moved to the nearest usable position inside the target work area. Windows
//!   are allowed to overlap - that is what a taskbar and Alt+Tab are for, and it is
//!   what the user means by "bring my windows over here".
//! * `Grid` is an explicit advanced choice that tiles the target display.
//!
//! There is deliberately no third mode. A "primary side" arrangement was removed:
//! it needed its own parameter, behaved badly with fixed-size windows and focus
//! changes, and added nothing the grid could not do. Keeping it meant maintaining
//! a second set of semantics for the same user request.

use serde::{Deserialize, Serialize};
use unfy_domain::{rescale_between, Rect, Scale, Size, WindowId};

/// How windows are arranged on the target display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutMode {
    /// Move every other-display window onto the target, preserving its size and
    /// its window state. This is the product default.
    MoveInto,
    /// Equal cells, best-fit column count. Explicitly chosen by the user.
    Grid,
}

impl LayoutMode {
    pub fn as_str(self) -> &'static str {
        match self {
            // Serialized as `default` because that is the name the interface has
            // always used for the primary mode, and an existing settings file must
            // keep meaning "the normal thing".
            LayoutMode::MoveInto => "default",
            LayoutMode::Grid => "grid",
        }
    }
}

/// How much of a window must be visible to count as reachable.
pub const MIN_REACHABLE: Size = Size::new(64, 32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub mode: LayoutMode,
    /// Space between the work area edge and the outermost windows. Grid only.
    pub margin: i32,
    /// Space between adjacent windows. Grid only.
    pub gap: i32,
    /// Planner floor used when a window's real minimum is unknown.
    pub fallback_min: Size,
    /// Maximum number of grid columns; more than this produces slivers.
    pub max_columns: u8,
    /// Bounds of the source virtual desktop.
    ///
    /// `MoveInto` no longer needs it (it never rescales), but the engine still
    /// records it so a plan can state which topology it was computed against.
    pub source_bounds: Option<Rect>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            mode: LayoutMode::MoveInto,
            margin: 12,
            gap: 8,
            fallback_min: Size::new(320, 200),
            max_columns: 8,
            source_bounds: None,
        }
    }
}

/// One window offered to the planner.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: WindowId,
    /// Current visible rectangle.
    pub current: Rect,
    /// The window's own minimum size when known.
    pub min_size: Option<Size>,
    /// Whether this window currently has keyboard focus.
    pub focused: bool,
    /// The work area of the display the window is on right now.
    pub source_work: Rect,
    /// Effective source scale; zero denotes unknown and prevents a write plan.
    pub source_scale: f64,
    /// Whether the window is already reachable on the target display.
    pub already_on_target: bool,
    /// Whether the platform says this window may be resized at all.
    pub can_resize: bool,
    /// Whether this window may be moved.
    pub can_move: bool,
}

impl Candidate {
    /// A candidate with the fields the planner needs and nothing else known.
    ///
    /// Tests and callers that do not have topology details use this; the engine
    /// fills the real values.
    pub fn simple(id: WindowId, current: Rect, min_size: Option<Size>) -> Self {
        Self {
            id,
            current,
            min_size,
            focused: false,
            source_work: current,
            source_scale: 1.0,
            already_on_target: false,
            can_resize: true,
            can_move: true,
        }
    }
}

/// Why a window was not given a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unplaced {
    /// The target area cannot hold this many windows at usable sizes.
    NoCapacity,
    /// The window cannot be moved at all.
    NotMovable,
    /// A source work area, rectangle or DPI scale is missing or invalid.
    InvalidGeometry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub id: WindowId,
    pub rect: Rect,
    /// True when the window could not keep its size and was placed at a size the
    /// target work area can hold.
    pub clamped_to_min: bool,
    /// True when the window is already where it should be and must not be
    /// written to at all.
    pub unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub mode: LayoutMode,
    pub placements: Vec<Placement>,
    pub unplaced: Vec<(WindowId, Unplaced)>,
    /// True when the planner had to overlap windows because they do not fit.
    pub cascaded: bool,
}

impl Plan {
    pub fn placement_for(&self, id: &WindowId) -> Option<&Placement> {
        self.placements.iter().find(|p| &p.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    /// Placements that require a platform write.
    pub fn changed(&self) -> impl Iterator<Item = &Placement> {
        self.placements.iter().filter(|p| !p.unchanged)
    }
}

/// Plan a layout. `target_work` is the work area in platform desktop units.
pub fn plan(
    candidates: &[Candidate],
    target_work: Rect,
    target_scale: f64,
    config: &LayoutConfig,
) -> Plan {
    if candidates.is_empty() {
        return Plan {
            mode: config.mode,
            placements: Vec::new(),
            unplaced: Vec::new(),
            cascaded: false,
        };
    }

    match config.mode {
        LayoutMode::MoveInto => move_into(candidates, target_work, target_scale, config),
        LayoutMode::Grid => {
            let scale = if target_scale.is_finite() && target_scale > 0.0 {
                target_scale
            } else {
                1.0
            };
            let scaled = LayoutConfig {
                margin: (f64::from(config.margin.max(0)) * scale).round() as i32,
                gap: (f64::from(config.gap.max(0)) * scale).round() as i32,
                fallback_min: Size::new(
                    (f64::from(config.fallback_min.width.max(1)) * scale).ceil() as i32,
                    (f64::from(config.fallback_min.height.max(1)) * scale).ceil() as i32,
                ),
                ..*config
            };
            grid_plan(candidates, target_work, target_scale, &scaled)
        }
    }
}

/// Whether a standard title bar is reachable in desktop units.
pub fn is_usable_on_target(rect: Rect, work: Rect) -> bool {
    is_usable_on_target_scaled(rect, work, 1.0)
}

/// `scale` is physical pixels per logical unit on Windows; point-based
/// backends pass 1.0. The visible patch starts at the title bar, not anywhere
/// in the window's body. Unknown custom drag regions require backend policy.
pub fn is_usable_on_target_scaled(rect: Rect, work: Rect, scale: f64) -> bool {
    if !scale.is_finite() || scale <= 0.0 {
        return false;
    }
    let width = (f64::from(MIN_REACHABLE.width) * scale).ceil() as i32;
    let height = (f64::from(MIN_REACHABLE.height) * scale).ceil() as i32;
    rect.reachable_in(&work, width, height) && rect.left >= work.left && rect.top >= work.top
}

/// The default mode: move windows onto the target without rearranging them.
///
/// Rules, in order:
///
/// 1. A window already usable on the target is `unchanged` - zero platform writes.
/// 2. A window that cannot be moved is reported, not silently dropped.
/// 3. A resizable window keeps its size unless that size cannot fit the target
///    work area at all; then it is scaled down by one uniform factor and clamped
///    to its own minimum. Width and height are never scaled independently, because
///    that is what made windows look squashed.
/// 4. The window is placed at the same relative position it had inside its own
///    source work area, mapped onto the target work area, then clamped so its
///    title bar is reachable. No global desktop-wide compression.
/// 5. Fixed-size windows are moved, never resized.
pub fn move_into(
    candidates: &[Candidate],
    target_work: Rect,
    target_scale: f64,
    config: &LayoutConfig,
) -> Plan {
    let mut placements = Vec::with_capacity(candidates.len());
    let mut unplaced = Vec::new();
    let mut overlapped = false;

    if target_work.is_empty() {
        return Plan {
            mode: LayoutMode::MoveInto,
            placements: Vec::new(),
            unplaced: candidates
                .iter()
                .map(|c| (c.id.clone(), Unplaced::NoCapacity))
                .collect(),
            cascaded: false,
        };
    }

    for candidate in candidates {
        if candidate.already_on_target {
            placements.push(Placement {
                id: candidate.id.clone(),
                rect: candidate.current,
                clamped_to_min: false,
                unchanged: true,
            });
            continue;
        }
        if !candidate.can_move {
            unplaced.push((candidate.id.clone(), Unplaced::NotMovable));
            continue;
        }
        if !valid_geometry(candidate, target_scale) {
            unplaced.push((candidate.id.clone(), Unplaced::InvalidGeometry));
            continue;
        }

        let (rect, clamped) = move_rect(candidate, target_work, target_scale, config);
        if placements
            .iter()
            .any(|placed| rect.intersection_area(&placed.rect) > 0)
        {
            // Overlap is expected and allowed; record it so the UI can explain why
            // windows are stacked instead of pretending the result is a tidy layout.
            overlapped = true;
        }
        placements.push(Placement {
            id: candidate.id.clone(),
            rect,
            clamped_to_min: clamped,
            unchanged: false,
        });
    }

    Plan {
        mode: LayoutMode::MoveInto,
        placements,
        unplaced,
        cascaded: overlapped,
    }
}

/// The rectangle one window should be moved to, and whether it had to be resized.
fn move_rect(
    candidate: &Candidate,
    target_work: Rect,
    target_scale: f64,
    config: &LayoutConfig,
) -> (Rect, bool) {
    let original_width = candidate.current.width().max(1);
    let original_height = candidate.current.height().max(1);
    let valid_scale = candidate.source_scale.is_finite()
        && candidate.source_scale > 0.0
        && target_scale.is_finite()
        && target_scale > 0.0;
    let ratio = if valid_scale && candidate.can_resize {
        target_scale / candidate.source_scale
    } else {
        1.0
    };
    let mut width = (f64::from(original_width) * ratio).round().max(1.0) as i32;
    let mut height = (f64::from(original_height) * ratio).round().max(1.0) as i32;
    let mut clamped = false;

    if candidate.can_resize {
        let minimum = candidate.min_size.filter(|m| m.width > 0 && m.height > 0);
        let (min_width, min_height) = minimum.map_or_else(
            || {
                (
                    ((f64::from(config.fallback_min.width.max(1)) * target_scale).ceil() as i32)
                        .min(width),
                    ((f64::from(config.fallback_min.height.max(1)) * target_scale).ceil() as i32)
                        .min(height),
                )
            },
            |m| {
                (
                    (f64::from(m.width) * ratio).ceil().max(1.0) as i32,
                    (f64::from(m.height) * ratio).ceil().max(1.0) as i32,
                )
            },
        );
        let fit = (f64::from(target_work.width()) / f64::from(width))
            .min(f64::from(target_work.height()) / f64::from(height))
            .min(1.0);
        let minimum_factor = (f64::from(min_width) / f64::from(width))
            .max(f64::from(min_height) / f64::from(height));
        // One factor satisfies both constraints. Independent max(min) calls
        // distort the aspect ratio and can manufacture an illegal rectangle.
        let factor = fit.max(minimum_factor);
        if factor != 1.0 {
            width = (f64::from(width) * factor).ceil().max(1.0) as i32;
            height = (f64::from(height) * factor).ceil().max(1.0) as i32;
            clamped = true;
        }
    }

    // Map the origin using the shared source-local convention. A one-pixel
    // probe avoids clamping the full-size rectangle before applying constraints.
    let origin = rescale_between(
        Rect::new(
            candidate.current.left,
            candidate.current.top,
            candidate.current.left.saturating_add(1),
            candidate.current.top.saturating_add(1),
        ),
        candidate.source_work,
        Scale::IDENTITY,
        target_work,
        Scale::IDENTITY,
    );
    let rect = Rect::new(
        origin.left,
        origin.top,
        origin.left.saturating_add(width),
        origin.top.saturating_add(height),
    );
    (rect.clamp_into(&target_work), clamped)
}

fn valid_geometry(candidate: &Candidate, target_scale: f64) -> bool {
    !candidate.current.is_empty()
        && !candidate.source_work.is_empty()
        && candidate.source_scale.is_finite()
        && candidate.source_scale > 0.0
        && target_scale.is_finite()
        && target_scale > 0.0
}

fn grid_plan(
    candidates: &[Candidate],
    target_work: Rect,
    target_scale: f64,
    config: &LayoutConfig,
) -> Plan {
    let mut ordered: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.can_move && valid_geometry(c, target_scale))
        .collect();
    let effective_min = |c: &Candidate| -> Size {
        if !c.can_resize {
            return Size::new(c.current.width().max(1), c.current.height().max(1));
        }
        if let Some(minimum) = c.min_size {
            let ratio = if c.source_scale.is_finite() && c.source_scale > 0.0 {
                target_scale / c.source_scale
            } else {
                1.0
            };
            Size::new(
                (f64::from(minimum.width.max(1)) * ratio).ceil() as i32,
                (f64::from(minimum.height.max(1)) * ratio).ceil() as i32,
            )
        } else {
            config.fallback_min
        }
    };
    let finish = |mut result: Plan| {
        result.unplaced.extend(candidates.iter().filter_map(|c| {
            if !c.can_move {
                Some((c.id.clone(), Unplaced::NotMovable))
            } else if !valid_geometry(c, target_scale) {
                Some((c.id.clone(), Unplaced::InvalidGeometry))
            } else {
                None
            }
        }));
        result
    };
    let usable = inset(target_work, config.margin);
    if ordered.is_empty() || usable.is_empty() || config.max_columns == 0 {
        return finish(Plan {
            mode: LayoutMode::Grid,
            placements: Vec::new(),
            unplaced: ordered
                .into_iter()
                .map(|c| (c.id.clone(), Unplaced::NoCapacity))
                .collect(),
            cascaded: false,
        });
    }
    ordered.sort_by_key(|c| {
        (
            !c.focused,
            c.current.top,
            c.current.left,
            c.id.native_id,
            c.id.process_id,
        )
    });
    let capacity = capacity(&usable, config, &ordered, &effective_min);
    if capacity == 0 || ordered.len() > capacity {
        let floor = |_: &Candidate| config.fallback_min;
        if crate::capacity(&usable, config, &ordered, &floor) < ordered.len() {
            return finish(cascade(&ordered, &usable, config, &effective_min));
        }
    }
    finish(grid(
        &ordered,
        &usable,
        config,
        &effective_min,
        LayoutMode::Grid,
    ))
}

/// How many windows fit at the minimum usable size.
fn capacity(
    usable: &Rect,
    config: &LayoutConfig,
    candidates: &[&Candidate],
    min_of: &impl Fn(&Candidate) -> Size,
) -> usize {
    // Use the largest minimum among the candidates: mixing sizes makes a
    // per-window count meaningless, and the conservative answer is the safe one.
    let (mut w, mut h) = (0, 0);
    for c in candidates {
        let m = min_of(c);
        w = w.max(m.width);
        h = h.max(m.height);
    }
    if w <= 0 || h <= 0 {
        return 0;
    }
    let cols = (usable.width() + config.gap) / (w + config.gap);
    let rows = (usable.height() + config.gap) / (h + config.gap);
    (cols.max(0) as usize) * (rows.max(0) as usize)
}

fn grid(
    ordered: &[&Candidate],
    usable: &Rect,
    config: &LayoutConfig,
    min_of: &impl Fn(&Candidate) -> Size,
    mode: LayoutMode,
) -> Plan {
    let n = ordered.len();
    let max_cols = usize::from(config.max_columns).min(n).max(1);

    let mut best: Option<(i64, usize, i32, i32)> = None;
    for cols in 1..=max_cols {
        let rows = n.div_ceil(cols);
        let cell_w = (usable.width() - config.gap * (cols as i32 - 1)) / cols as i32;
        let cell_h = (usable.height() - config.gap * (rows as i32 - 1)) / rows as i32;
        if cell_w <= 0 || cell_h <= 0 {
            continue;
        }

        let violations = ordered
            .iter()
            .filter(|c| {
                let m = min_of(c);
                cell_w < m.width || cell_h < m.height
            })
            .count() as i64;
        let empty = (rows * cols - n) as i64;
        // A grid that wastes an entire row or column is qualitatively worse than
        // one that merely has a ragged last row: 9 windows in 4x3 leaves a whole
        // column unused and squeezes every cell, while 5 in 3x2 leaves one hole.
        let wastes_a_track = empty > 0 && empty >= rows.min(cols) as i64;

        let avg_ratio = average_aspect(ordered);
        let cell_ratio = f64::from(cell_w) / f64::from(cell_h);
        // Two aspect terms, because matching the average ratio alone is not
        // enough: on a 1920x1032 target, five 1-row cells have ratio 1.86, which
        // is *closer* to a landscape window's 1.5 than a 3x2 cell's 1.23.
        let ratio_deviation = (cell_ratio / avg_ratio.max(0.1) - 1.0).abs();
        let absolute_squash = if cell_ratio > 2.5 {
            cell_ratio - 2.5
        } else if cell_ratio < 0.4 {
            0.4 / cell_ratio.max(0.01)
        } else {
            0.0
        };
        let aspect_penalty =
            ((ratio_deviation * 6_000.0) + (absolute_squash * 20_000.0)).min(999_000.0) as i64;

        let score = violations * 1_000_000_000
            + i64::from(wastes_a_track) * 50_000_000
            + aspect_penalty * 1_000
            + empty * 400_000;
        let better = best.is_none_or(|(s, ..)| score < s);
        if better {
            best = Some((score, cols, cell_w, cell_h));
        }
    }

    let Some((_, cols, cell_w, cell_h)) = best else {
        return Plan {
            mode,
            placements: Vec::new(),
            unplaced: ordered
                .iter()
                .map(|c| (c.id.clone(), Unplaced::NoCapacity))
                .collect(),
            cascaded: false,
        };
    };

    let mut placements = Vec::with_capacity(n);
    for (index, candidate) in ordered.iter().enumerate() {
        let col = index % cols;
        let row = index / cols;
        let left = usable.left + col as i32 * (cell_w + config.gap);
        let top = usable.top + row as i32 * (cell_h + config.gap);

        let min = min_of(candidate);
        let width = if candidate.can_resize {
            cell_w.max(min.width)
        } else {
            candidate.current.width()
        };
        let height = if candidate.can_resize {
            cell_h.max(min.height)
        } else {
            candidate.current.height()
        };
        let rect = Rect::new(left, top, left + width, top + height).clamp_into(usable);

        placements.push(Placement {
            id: candidate.id.clone(),
            rect,
            clamped_to_min: width > cell_w || height > cell_h,
            unchanged: false,
        });
    }

    Plan {
        mode,
        placements,
        unplaced: Vec::new(),
        cascaded: false,
    }
}

/// Last resort when the target area cannot tile every window at a usable size:
/// stack them with a visible offset so each title bar stays reachable.
fn cascade(
    ordered: &[&Candidate],
    usable: &Rect,
    config: &LayoutConfig,
    min_of: &impl Fn(&Candidate) -> Size,
) -> Plan {
    let step = 28;
    let mut placements = Vec::new();
    let mut unplaced = Vec::new();

    let max_visible = ((usable.width() - config.fallback_min.width) / step).max(0) + 1;
    let max_visible = usize::try_from(max_visible).unwrap_or(0).max(1);

    for (index, candidate) in ordered.iter().enumerate() {
        if index >= max_visible {
            unplaced.push((candidate.id.clone(), Unplaced::NoCapacity));
            continue;
        }
        let min = min_of(candidate);
        let width = if candidate.can_resize {
            min.width
        } else {
            candidate.current.width()
        };
        let height = if candidate.can_resize {
            min.height
        } else {
            candidate.current.height()
        };
        let offset = index as i32 * step;
        let left = usable.left + offset;
        let top = usable.top + offset;
        placements.push(Placement {
            id: candidate.id.clone(),
            rect: Rect::new(left, top, left + width, top + height).clamp_into(usable),
            clamped_to_min: width > usable.width() || height > usable.height(),
            unchanged: false,
        });
    }

    Plan {
        mode: LayoutMode::Grid,
        placements,
        unplaced,
        cascaded: true,
    }
}

fn average_aspect(windows: &[&Candidate]) -> f64 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for w in windows {
        if w.current.height() > 0 {
            sum += f64::from(w.current.width()) / f64::from(w.current.height());
            count += 1.0;
        }
    }
    if count == 0.0 {
        1.6
    } else {
        sum / count
    }
}

fn inset(area: Rect, margin: i32) -> Rect {
    let margin = margin.max(0);
    let inset = Rect::new(
        area.left + margin,
        area.top + margin,
        area.right - margin,
        area.bottom - margin,
    );
    if inset.is_empty() {
        area
    } else {
        inset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1040);
    const OTHER: Rect = Rect::new(1920, 0, 3840, 1040);

    fn id(n: u64) -> WindowId {
        WindowId {
            native_id: n,
            process_id: 1,
            process_start: 1,
            instance: 1,
        }
    }

    fn candidate(n: u64, rect: Rect) -> Candidate {
        Candidate {
            id: id(n),
            current: rect,
            min_size: None,
            focused: false,
            source_work: OTHER,
            source_scale: 1.0,
            already_on_target: false,
            can_resize: true,
            can_move: true,
        }
    }

    #[test]
    fn invalid_dpi_never_produces_a_write_plan() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            for config in [LayoutConfig::default(), grid_config()] {
                let c = candidate(1, Rect::new(2000, 100, 2800, 700));
                let result = plan(&[c], WORK, scale, &config);
                assert!(result.placements.is_empty());
                assert_eq!(result.unplaced, vec![(id(1), Unplaced::InvalidGeometry)]);
            }
        }
    }
    #[test]
    fn titlebar_reachability_uses_logical_scale() {
        let rect = Rect::new(0, 1000, 800, 1600);
        assert!(is_usable_on_target_scaled(rect, WORK, 1.0));
        assert!(!is_usable_on_target_scaled(rect, WORK, 1.5));
        assert!(!is_usable_on_target_scaled(
            Rect::new(0, -100, 800, 500),
            WORK,
            1.0
        ));
    }

    #[test]
    fn grid_never_resizes_fixed_windows_or_places_immovable_windows() {
        let mut fixed = candidate(1, Rect::new(2000, 100, 2180, 220));
        fixed.can_resize = false;
        let mut immovable = candidate(2, Rect::new(2300, 100, 3000, 700));
        immovable.can_move = false;
        let result = plan(&[fixed, immovable], WORK, 1.0, &grid_config());
        assert_eq!(result.placements.len(), 1);
        assert_eq!(
            (
                result.placements[0].rect.width(),
                result.placements[0].rect.height()
            ),
            (180, 120)
        );
        assert_eq!(result.unplaced, vec![(id(2), Unplaced::NotMovable)]);
    }
    #[test]
    fn fixed_size_preserves_pixels_across_dpi() {
        let mut c = candidate(1, Rect::new(2000, 100, 2600, 500));
        c.can_resize = false;
        c.source_scale = 1.5;
        let result = plan(&[c], WORK, 1.0, &LayoutConfig::default());
        assert_eq!(result.placements[0].rect.width(), 600);
        assert_eq!(result.placements[0].rect.height(), 400);
    }

    #[test]
    fn logical_size_converts_between_displays() {
        let mut c = candidate(1, Rect::new(2000, 100, 3200, 1000));
        c.source_scale = 1.5;
        let result = plan(&[c], WORK, 1.0, &LayoutConfig::default());
        assert_eq!(result.placements[0].rect.width(), 800);
        assert_eq!(result.placements[0].rect.height(), 600);
    }

    #[test]
    fn minimum_constraint_preserves_aspect_when_target_is_too_small() {
        let mut c = candidate(1, Rect::new(2000, 100, 6000, 1100));
        c.min_size = Some(Size::new(900, 700));
        let result = plan(&[c], WORK, 1.0, &LayoutConfig::default());
        let rect = result.placements[0].rect;
        assert_eq!((rect.width(), rect.height()), (2800, 700));
        assert!(result.placements[0].clamped_to_min);
    }

    #[test]
    fn immovable_window_already_on_target_is_unchanged() {
        let mut c = on_target(1).remove(0);
        c.can_move = false;
        let result = plan(&[c], WORK, 1.0, &LayoutConfig::default());
        assert_eq!(result.placements.len(), 1);
        assert!(result.placements[0].unchanged);
        assert!(result.unplaced.is_empty());
    }
    fn candidates(count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|i| {
                candidate(
                    i as u64,
                    Rect::new(1920, i as i32 * 40, 2720, i as i32 * 40 + 600),
                )
            })
            .collect()
    }

    fn on_target(count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|i| {
                let mut c = candidate(i as u64, Rect::new(100, 100 + i as i32 * 40, 900, 700));
                c.already_on_target = true;
                c.source_work = WORK;
                c
            })
            .collect()
    }

    fn grid_config() -> LayoutConfig {
        LayoutConfig {
            mode: LayoutMode::Grid,
            ..Default::default()
        }
    }

    #[test]
    fn default_mode_is_move_into() {
        assert_eq!(LayoutConfig::default().mode, LayoutMode::MoveInto);
        assert_eq!(LayoutMode::MoveInto.as_str(), "default");
    }

    #[test]
    fn empty_input_produces_empty_plan() {
        let plan = plan(&[], WORK, 1.0, &LayoutConfig::default());
        assert!(plan.is_empty());
        assert!(!plan.cascaded);
    }

    #[test]
    fn windows_already_on_target_are_not_touched() {
        // The defect this replaces: the old default rescaled the entire virtual
        // desktop, so windows the user could already see were shrunk and moved.
        let input = on_target(3);
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        assert_eq!(plan.placements.len(), 3);
        for placement in &plan.placements {
            assert!(placement.unchanged, "must not request a write");
        }
        assert_eq!(plan.changed().count(), 0);
    }

    #[test]
    fn other_display_window_keeps_its_size() {
        let input = vec![candidate(1, Rect::new(2000, 100, 3200, 1000))];
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        let placed = &plan.placements[0];
        assert_eq!(placed.rect.width(), 1200);
        assert_eq!(placed.rect.height(), 900);
        assert!(!placed.unchanged);
        assert!(placed
            .rect
            .reachable_in(&WORK, MIN_REACHABLE.width, MIN_REACHABLE.height));
    }

    #[test]
    fn adding_an_unrelated_display_does_not_change_the_result() {
        // Sizes are never derived from the whole desktop, so the presence of a
        // third screen cannot affect a window moving between two others.
        let input = vec![candidate(1, Rect::new(2000, 100, 3200, 1000))];
        let a = plan(&input, WORK, 1.0, &LayoutConfig::default());

        let wide = LayoutConfig {
            source_bounds: Some(Rect::new(-1920, 0, 5760, 1080)),
            ..LayoutConfig::default()
        };
        let b = plan(&input, WORK, 1.0, &wide);
        assert_eq!(a.placements, b.placements);
    }

    #[test]
    fn oversized_window_is_scaled_by_one_factor() {
        let mut input = vec![candidate(1, Rect::new(0, 0, 4000, 1000))];
        input[0].source_work = Rect::new(0, 0, 4000, 1000);
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        let rect = plan.placements[0].rect;
        assert!(rect.width() <= WORK.width());
        assert!(rect.height() <= WORK.height());
        // Uniform scaling: original ratio 4:1 must be preserved within rounding.
        let ratio = f64::from(rect.width()) / f64::from(rect.height());
        assert!((ratio - 4.0).abs() < 0.05, "ratio {ratio} is not uniform");
    }

    #[test]
    fn small_window_is_not_inflated() {
        let input = vec![candidate(1, Rect::new(2000, 100, 2180, 220))];
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        assert_eq!(plan.placements[0].rect.width(), 180);
        assert_eq!(plan.placements[0].rect.height(), 120);
        assert!(!plan.placements[0].clamped_to_min);
    }

    #[test]
    fn minimum_size_is_never_violated() {
        let mut input = vec![candidate(1, Rect::new(0, 0, 4000, 1000))];
        input[0].source_work = Rect::new(0, 0, 4000, 1000);
        input[0].min_size = Some(Size::new(900, 700));
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        let rect = plan.placements[0].rect;
        assert!(rect.width() >= 900);
        assert!(rect.height() >= 700);
    }

    #[test]
    fn fixed_size_window_is_only_moved() {
        let mut input = vec![candidate(1, Rect::new(0, 0, 4000, 1000))];
        input[0].source_work = Rect::new(0, 0, 4000, 1000);
        input[0].can_resize = false;
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        let rect = plan.placements[0].rect;
        assert_eq!(rect.width(), 4000);
        assert_eq!(rect.height(), 1000);
        assert!(rect.reachable_in(&WORK, MIN_REACHABLE.width, MIN_REACHABLE.height));
    }

    #[test]
    fn immovable_window_is_reported_not_dropped() {
        let mut input = vec![candidate(1, Rect::new(2000, 100, 2400, 400))];
        input[0].can_move = false;
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        assert!(plan.placements.is_empty());
        assert_eq!(plan.unplaced, vec![(id(1), Unplaced::NotMovable)]);
    }

    #[test]
    fn overlapping_is_allowed_and_reported() {
        let input = vec![
            candidate(1, Rect::new(2000, 100, 2600, 500)),
            candidate(2, Rect::new(2000, 100, 2600, 500)),
        ];
        let plan = plan(&input, WORK, 1.0, &LayoutConfig::default());
        assert_eq!(plan.placements.len(), 2);
        assert_eq!(
            plan.placements[0]
                .rect
                .intersection_area(&plan.placements[1].rect),
            600 * 400,
            "the two windows are expected to overlap, not to be tiled"
        );
    }

    #[test]
    fn negative_and_portrait_work_areas_keep_windows_reachable() {
        for work in [
            Rect::new(-1920, -100, 0, 940),
            Rect::new(0, 0, 1080, 1920),
            Rect::new(0, -1080, 1920, 0),
        ] {
            let input = vec![candidate(1, Rect::new(2000, 100, 2600, 500))];
            let plan = plan(&input, work, 1.0, &LayoutConfig::default());
            let rect = plan.placements[0].rect;
            assert!(
                rect.reachable_in(&work, MIN_REACHABLE.width, MIN_REACHABLE.height),
                "{rect:?} escaped {work:?}"
            );
        }
    }

    #[test]
    fn grid_still_tiles_on_request() {
        let plan = plan(&candidates(4), WORK, 1.0, &grid_config());
        assert_eq!(plan.mode, LayoutMode::Grid);
        assert_eq!(plan.placements.len(), 4);
        for (i, a) in plan.placements.iter().enumerate() {
            for b in plan.placements.iter().skip(i + 1) {
                assert_eq!(a.rect.intersection_area(&b.rect), 0);
            }
        }
    }

    #[test]
    fn four_windows_form_a_2x2_grid() {
        let plan = plan(&candidates(4), WORK, 1.0, &grid_config());
        let mut lefts: Vec<i32> = plan.placements.iter().map(|p| p.rect.left).collect();
        lefts.sort_unstable();
        lefts.dedup();
        assert_eq!(lefts.len(), 2);
    }

    #[test]
    fn grid_placements_stay_inside_work_area() {
        for count in [1usize, 3, 7, 20] {
            let plan = plan(&candidates(count), WORK, 1.0, &grid_config());
            for p in &plan.placements {
                assert!(p.rect.reachable_in(&WORK, 64, 32));
            }
        }
    }

    #[test]
    fn grid_respects_minimum_size() {
        let mut input = candidates(4);
        for c in &mut input {
            c.min_size = Some(Size::new(700, 500));
        }
        let plan = plan(&input, WORK, 1.0, &grid_config());
        for p in &plan.placements {
            assert!(p.rect.width() >= 700);
            assert!(p.rect.height() >= 500);
        }
    }

    #[test]
    fn too_many_windows_cascades_instead_of_shrinking() {
        let plan = plan(&candidates(200), WORK, 1.0, &grid_config());
        assert!(plan.cascaded);
        assert!(!plan.placements.is_empty());
        for p in &plan.placements {
            assert!(p.rect.width() >= LayoutConfig::default().fallback_min.width);
            assert!(p.rect.reachable_in(&WORK, 64, 32));
        }
        assert!(!plan.unplaced.is_empty(), "overflow must be reported");
    }

    #[test]
    fn degenerate_work_area_yields_no_placements() {
        let zero = Rect::new(0, 0, 0, 0);
        let plan = plan(&candidates(3), zero, 1.0, &LayoutConfig::default());
        assert!(plan.placements.is_empty());
        assert_eq!(plan.unplaced.len(), 3);
    }

    #[test]
    fn plan_is_deterministic() {
        let input = candidates(11);
        let a = plan(&input, WORK, 1.0, &grid_config());
        let b = plan(&input, WORK, 1.0, &grid_config());
        assert_eq!(a.placements, b.placements);
        assert_eq!(a.mode, b.mode);
    }
}

#[cfg(test)]
mod column_choice_tests {
    //! The preview exposed a real defect: 5 windows in a 1920x1032 area were laid
    //! out as 5 vertical slivers because "zero empty cells" outranked "usable
    //! aspect ratio". These cases pin the intuitive result for the small counts
    //! users actually hit in the explicit grid mode.

    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1032);

    fn windows(count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|i| Candidate {
                id: WindowId {
                    native_id: i as u64,
                    process_id: 1,
                    process_start: 1,
                    instance: 1,
                },
                current: Rect::new(0, 0, 1200, 800),
                min_size: None,
                focused: false,
                source_work: Rect::new(0, 0, 1200, 800),
                source_scale: 1.0,
                already_on_target: false,
                can_resize: true,
                can_move: true,
            })
            .collect()
    }

    fn config() -> LayoutConfig {
        LayoutConfig {
            mode: LayoutMode::Grid,
            ..Default::default()
        }
    }

    fn columns(plan: &Plan) -> usize {
        let mut lefts: Vec<i32> = plan.placements.iter().map(|p| p.rect.left).collect();
        lefts.sort_unstable();
        lefts.dedup();
        lefts.len()
    }

    #[test]
    fn five_windows_use_three_columns_not_five() {
        let plan = plan(&windows(5), WORK, 1.0, &config());
        assert_eq!(columns(&plan), 3, "5 windows should tile 3x2, not 5x1");
        for p in &plan.placements {
            assert!(
                p.rect.width() > p.rect.height() / 2,
                "a cell {:?} is a sliver",
                p.rect
            );
        }
    }

    #[test]
    fn four_windows_stay_two_by_two() {
        assert_eq!(columns(&plan(&windows(4), WORK, 1.0, &config())), 2);
    }

    #[test]
    fn six_windows_use_three_columns() {
        assert_eq!(columns(&plan(&windows(6), WORK, 1.0, &config())), 3);
    }

    #[test]
    fn nine_windows_use_three_columns() {
        assert_eq!(columns(&plan(&windows(9), WORK, 1.0, &config())), 3);
    }
}
