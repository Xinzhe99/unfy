//! Layout planning: pure functions from a window set to target rectangles.
//!
//! Nothing here touches an operating system. Given the same inputs the output is
//! byte-identical, which is what makes the plan preview trustworthy: the user
//! sees exactly what will be requested.
//!
//! The planner works in **physical pixels** of the target display, because that
//! is what the platform `apply` call takes. It never resizes a window below its
//! known minimum, and when the target work area cannot hold every window at a
//! usable size it falls back to a cascade instead of producing unusable cells.

use conflux_domain::{Rect, Size, WindowId};
use serde::{Deserialize, Serialize};

/// How windows are arranged on the target display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutMode {
    /// Equal cells, best-fit column count.
    Grid,
    /// Focused window takes the main area, the rest tile beside it.
    PrimarySide,
    /// The arrangement the windows already have: the source desktop maps onto
    /// the target work area as a whole, so relative positions and relative
    /// sizes survive the move. This is the product's default.
    KeepLayout,
}

impl LayoutMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LayoutMode::Grid => "grid",
            LayoutMode::PrimarySide => "primary_side",
            // Serialized as "default" because that is what the interface calls
            // it; the previous meaning of this slot ("preserve_order") only
            // kept the slot *order* inside an ordinary grid, which is why it
            // looked like the setting did nothing.
            LayoutMode::KeepLayout => "default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub mode: LayoutMode,
    /// Space between the work area edge and the outermost windows.
    pub margin: i32,
    /// Space between adjacent windows.
    pub gap: i32,
    /// Planner floor used when a window's real minimum is unknown.
    pub fallback_min: Size,
    /// Maximum number of grid columns; more than this produces slivers.
    pub max_columns: u8,
    /// Share of the width given to the focused window in `PrimarySide`.
    pub primary_share_percent: u8,
    /// Bounds of the source virtual desktop, used only by `KeepLayout` to map
    /// the old arrangement onto the target. `None` (or an empty rectangle)
    /// falls back to the grid: without a source rectangle there is nothing to
    /// map relative positions against.
    pub source_bounds: Option<Rect>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            mode: LayoutMode::Grid,
            margin: 12,
            gap: 8,
            fallback_min: Size::new(320, 200),
            max_columns: 8,
            primary_share_percent: 60,
            source_bounds: None,
        }
    }
}

/// One window offered to the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: WindowId,
    /// Current visible rectangle, used for relative ordering.
    pub current: Rect,
    /// The window's own minimum size when known.
    pub min_size: Option<Size>,
    /// Whether this window currently has keyboard focus.
    pub focused: bool,
}

/// Why a window was not given a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unplaced {
    /// The target area cannot hold this many windows at usable sizes.
    NoCapacity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub id: WindowId,
    pub rect: Rect,
    /// True when the window was placed at its minimum size because the slot was
    /// too small, so it may overlap its neighbours.
    pub clamped_to_min: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub mode: LayoutMode,
    pub placements: Vec<Placement>,
    pub unplaced: Vec<(WindowId, Unplaced)>,
    /// True when the planner fell back to a cascade.
    pub cascaded: bool,
}

impl Plan {
    pub fn placement_for(&self, id: &WindowId) -> Option<&Placement> {
        self.placements.iter().find(|p| &p.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }
}

/// Plan a layout. `target_work` is the work area in physical pixels.
///
/// `scale` is accepted for callers that want to reason about apparent size, but
/// does not change the result: tiling is defined in the target's own pixels.
pub fn plan(candidates: &[Candidate], target_work: Rect, config: &LayoutConfig) -> Plan {
    if candidates.is_empty() {
        return Plan {
            mode: config.mode,
            placements: Vec::new(),
            unplaced: Vec::new(),
            cascaded: false,
        };
    }

    // KeepLayout does not use cells at all, so it bypasses the column/capacity
    // machinery below and returns before the grid-specific early-outs.
    if config.mode == LayoutMode::KeepLayout {
        return keep_layout(candidates, target_work, config);
    }

    let mut ordered: Vec<&Candidate> = candidates.iter().collect();
    if config.max_columns == 0 || ordered.is_empty() {
        return Plan {
            mode: LayoutMode::Grid,
            placements: Vec::new(),
            unplaced: ordered
                .into_iter()
                .map(|c| (c.id.clone(), Unplaced::NoCapacity))
                .collect(),
            cascaded: false,
        };
    }

    let usable = inset(target_work, config.margin);
    if usable.is_empty() {
        return Plan {
            mode: LayoutMode::Grid,
            placements: Vec::new(),
            unplaced: ordered
                .into_iter()
                .map(|c| (c.id.clone(), Unplaced::NoCapacity))
                .collect(),
            cascaded: false,
        };
    }

    // Deterministic order: the original spatial order (top, then left, so a row
    // keeps its left-to-right reading order on every platform), with the id as the
    // tie-break.
    //
    // Whether focus comes into it is what separates two of the modes. `Grid` puts
    // the focused window in the first cell - the one the eye goes to - while
    // `PrimarySide` needs focus for its large pane. `KeepLayout` never reaches
    // this code: it maps positions directly and has no slots to fill.
    let focus_first = true;
    ordered.sort_by_key(|c| {
        (
            focus_first && !c.focused,
            c.current.top,
            c.current.left,
            c.id.native_id,
            c.id.process_id,
        )
    });

    let effective_min = |c: &Candidate| -> Size {
        let m = c.min_size.unwrap_or(config.fallback_min);
        Size::new(m.width.max(1), m.height.max(1))
    };

    let capacity = capacity(&usable, config, candidates, &effective_min);
    if capacity == 0 || candidates.len() > capacity {
        // The windows' own minimums do not lay out at those sizes. That is a
        // preference, not a prohibition: `grid` ranks arrangements by how many
        // windows they would squeeze below their own minimum and marks those places
        // `clamped_to_min`, so it can still produce something usable. Cascade only
        // when even the planner's floor cannot hold everyone, which is what the
        // cascade is for.
        //
        // Measured on a real Windows 11 desktop: eight windows, every one reporting
        // its *current* size as its minimum (Notepad, Terminal, Explorer and the
        // Calculator all do this through their composition layer), so no grid could
        // satisfy any of them and a gather piled all eight diagonally - overlapping,
        // which is the opposite of what "gather onto one screen" is for. A 3x3 grid
        // squeezes two of those eight and shows all of them.
        let floor = |_: &Candidate| config.fallback_min;
        if crate::capacity(&usable, config, candidates, &floor) < candidates.len() {
            return cascade(&ordered, &usable, config, &effective_min);
        }
    }

    match config.mode {
        LayoutMode::Grid => grid(&ordered, &usable, config, &effective_min, LayoutMode::Grid),
        LayoutMode::PrimarySide => primary_side(&ordered, &usable, config, &effective_min),
        LayoutMode::KeepLayout => unreachable!("handled before the grid machinery"),
    }
}

/// Map the windows' current arrangement onto the target work area.
///
/// The source desktop and the target work area are treated as two rectangles
/// and each window is placed where its fraction of the source sits in the
/// target: a window that covered the left half of the virtual desktop covers
/// the left half of the target afterwards, on every axis independently. That
/// is the promise of this mode - relative positions and relative sizes are
/// the same before and after, so a multi-monitor desktop folds onto the one
/// remote screen the way the user already had it in mind.
///
/// Corners cut, on purpose and in this order:
///
/// * Sizes never go below the window's minimum (real when known, planner
///   floor otherwise). Fidelity loses to usability, exactly as in the grid.
/// * The result is kept inside the target work area. A window parked partly
///   on the taskbar would otherwise map below the work area's bottom edge.
/// * Without a source rectangle the mode degrades to the grid rather than
///   inventing one; the caller normally supplies the virtual desktop bounds.
fn keep_layout(candidates: &[Candidate], target_work: Rect, config: &LayoutConfig) -> Plan {
    let Some(src) = config.source_bounds.filter(|r| !r.is_empty()) else {
        let fallback = LayoutConfig {
            mode: LayoutMode::Grid,
            ..*config
        };
        return plan(candidates, target_work, &fallback);
    };

    let sw = (src.right - src.left) as i64;
    let sh = (src.bottom - src.top) as i64;
    let tw = (target_work.right - target_work.left) as i64;
    let th = (target_work.bottom - target_work.top) as i64;
    // The guard above filters empty rectangles, so these cannot divide by zero
    // unless a caller hands over bounds that overflow i32 span - not a real
    // desktop.
    debug_assert!(sw > 0 && sh > 0 && tw > 0 && th > 0);

    let mut placements = Vec::with_capacity(candidates.len());
    for c in candidates {
        let r = c.current;
        // i64 intermediates: 4k-wide coordinates times each other exceed i32.
        let left = target_work.left as i64 + (r.left - src.left) as i64 * tw / sw;
        let top = target_work.top as i64 + (r.top - src.top) as i64 * th / sh;
        let width = (r.right - r.left) as i64 * tw / sw;
        let height = (r.bottom - r.top) as i64 * th / sh;

        let min = {
            let m = c.min_size.unwrap_or(config.fallback_min);
            (m.width.max(1), m.height.max(1))
        };
        // Judged on the mapped size, not the original one: a big window that maps
        // small needs the lift just as much as a small one.
        let below_min = width < min.0 as i64 || height < min.1 as i64;
        let width = (width.max(min.0 as i64)).min(tw) as i32;
        let height = (height.max(min.1 as i64)).min(th) as i32;

        let left = (left.max(target_work.left as i64)).min((target_work.right - width) as i64) as i32;
        let top = (top.max(target_work.top as i64)).min((target_work.bottom - height) as i64) as i32;

        placements.push(Placement {
            id: c.id.clone(),
            rect: Rect::new(left, top, left + width, top + height),
            clamped_to_min: below_min,
        });
    }
    placements.sort_by_key(|p| p.id.native_id);

    Plan {
        mode: LayoutMode::KeepLayout,
        placements,
        unplaced: Vec::new(),
        cascaded: false,
    }
}

/// How many windows fit at the minimum usable size.
fn capacity(
    usable: &Rect,
    config: &LayoutConfig,
    candidates: &[Candidate],
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
        // Ratio-only scoring cannot tell these apart, which is why 9 windows used
        // to land in 4 columns.
        let wastes_a_track = empty > 0 && empty >= rows.min(cols) as i64;

        // Rank candidate grids by, in order of importance:
        //
        // 1. windows that would be squeezed below their minimum size,
        // 2. how far each cell's aspect ratio is from the average window's,
        // 3. empty cells.
        //
        // Aspect ratio outranks empty cells deliberately: 5 windows in 3x2 leaves
        // one cell empty and gives usable ~640x510 cells, while 5x1 has no empty
        // cells but produces 5 slivers. The earlier weighting (empty cells before
        // aspect) chose the sliver layout, which the preview made obvious.
        let avg_ratio = average_aspect(ordered);
        let cell_ratio = f64::from(cell_w) / f64::from(cell_h);
        // Two aspect terms, because matching the average ratio alone is not
        // enough: on a 1920x1032 target, five 1-row cells have ratio 1.86, which
        // is *closer* to a landscape window's 1.5 than a 3x2 cell's 1.23. Ranking
        // by closeness alone would therefore pick the five-sliver layout, which
        // is exactly what the preview showed. The second term punishes cells far
        // outside a normal window's range in absolute terms.
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

        // Order of importance: squeeze below the minimum size, then unusable cell
        // shape, then empty cells. The empty-cell weight is high enough to keep
        // 2x2 for four windows (where 3x2 has only a marginally better aspect but
        // two holes) and low enough that 5 windows still prefer 3x2 over 1x5.
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
        let width = cell_w.max(min.width);
        let height = cell_h.max(min.height);
        let rect = Rect::new(left, top, left + width, top + height).clamp_into(usable);

        placements.push(Placement {
            id: candidate.id.clone(),
            rect,
            clamped_to_min: width > cell_w || height > cell_h,
        });
    }

    Plan {
        mode,
        placements,
        unplaced: Vec::new(),
        cascaded: false,
    }
}

fn primary_side(
    ordered: &[&Candidate],
    usable: &Rect,
    config: &LayoutConfig,
    min_of: &impl Fn(&Candidate) -> Size,
) -> Plan {
    let Some((primary, rest)) = ordered.split_first() else {
        return Plan {
            mode: LayoutMode::PrimarySide,
            placements: Vec::new(),
            unplaced: Vec::new(),
            cascaded: false,
        };
    };

    if rest.is_empty() {
        return Plan {
            mode: LayoutMode::PrimarySide,
            placements: vec![Placement {
                id: primary.id.clone(),
                rect: *usable,
                clamped_to_min: false,
            }],
            unplaced: Vec::new(),
            cascaded: false,
        };
    }

    let share = i32::from(config.primary_share_percent.clamp(30, 80));
    let main_width = (usable.width() - config.gap) * share / 100;
    let side_left = usable.left + main_width + config.gap;
    let side_width = usable.right - side_left;

    let min_side = min_of(rest[0]).width;
    // Not enough room for a side column: fall back to a plain grid rather than
    // producing a 40 px column.
    if side_width < min_side {
        return grid(ordered, usable, config, min_of, LayoutMode::Grid);
    }

    let mut placements = vec![Placement {
        id: primary.id.clone(),
        rect: Rect::new(
            usable.left,
            usable.top,
            usable.left + main_width,
            usable.bottom,
        ),
        clamped_to_min: false,
    }];

    let side_area = Rect::new(side_left, usable.top, usable.right, usable.bottom);
    let side_plan = grid(rest, &side_area, config, min_of, LayoutMode::PrimarySide);
    placements.extend(side_plan.placements);
    placements[0].clamped_to_min = side_plan.cascaded;

    Plan {
        mode: LayoutMode::PrimarySide,
        placements,
        unplaced: side_plan.unplaced,
        cascaded: side_plan.cascaded,
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

    // How many cascaded windows can be reached before the offsets push the last
    // one out of the work area.
    let max_visible = ((usable.width() - config.fallback_min.width) / step).max(0) + 1;
    let max_visible = usize::try_from(max_visible).unwrap_or(0).max(1);

    for (index, candidate) in ordered.iter().enumerate() {
        if index >= max_visible {
            unplaced.push((candidate.id.clone(), Unplaced::NoCapacity));
            continue;
        }
        let min = min_of(candidate);
        let width = min.width.min(usable.width());
        let height = min.height.min(usable.height());
        let offset = index as i32 * step;
        let left = usable.left + offset;
        let top = usable.top + offset;
        let rect = Rect::new(left, top, left + width, top + height).clamp_into(usable);
        placements.push(Placement {
            id: candidate.id.clone(),
            rect,
            clamped_to_min: true,
        });
    }

    Plan {
        mode: LayoutMode::Grid,
        placements,
        unplaced,
        cascaded: true,
    }
}

fn inset(rect: Rect, by: i32) -> Rect {
    let by = by.max(0);
    let candidate = Rect::new(
        rect.left + by,
        rect.top + by,
        rect.right - by,
        rect.bottom - by,
    );
    if candidate.is_empty() {
        rect
    } else {
        candidate
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

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1040);

    fn id(n: u64) -> WindowId {
        WindowId {
            native_id: n,
            process_id: 1,
            process_start: 1,
        }
    }

    fn candidate(n: u64, rect: Rect) -> Candidate {
        Candidate {
            id: id(n),
            current: rect,
            min_size: None,
            focused: false,
        }
    }

    fn candidates(count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|i| candidate(i as u64, Rect::new(0, i as i32 * 40, 800, 600)))
            .collect()
    }

    #[test]
    fn empty_input_produces_empty_plan() {
        let plan = plan(&[], WORK, &LayoutConfig::default());
        assert!(plan.is_empty());
        assert!(!plan.cascaded);
    }

    #[test]
    fn single_window_fills_work_area_minus_margin() {
        let plan = plan(&candidates(1), WORK, &LayoutConfig::default());
        let rect = plan.placements[0].rect;
        assert_eq!(rect, inset(WORK, 12));
    }

    #[test]
    fn four_windows_form_a_2x2_grid() {
        let plan = plan(&candidates(4), WORK, &LayoutConfig::default());
        assert_eq!(plan.placements.len(), 4);
        let rects: Vec<Rect> = plan.placements.iter().map(|p| p.rect).collect();
        // Two distinct columns and two distinct rows.
        let mut lefts: Vec<i32> = rects.iter().map(|r| r.left).collect();
        lefts.sort_unstable();
        lefts.dedup();
        assert_eq!(lefts.len(), 2);
        let mut tops: Vec<i32> = rects.iter().map(|r| r.top).collect();
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len(), 2);
    }

    #[test]
    fn placements_never_overlap_in_grid_mode() {
        for count in 1..=9 {
            let plan = plan(&candidates(count), WORK, &LayoutConfig::default());
            assert!(!plan.cascaded, "{count} windows should tile");
            for (i, a) in plan.placements.iter().enumerate() {
                for b in plan.placements.iter().skip(i + 1) {
                    assert_eq!(
                        a.rect.intersection_area(&b.rect),
                        0,
                        "windows {} and {} overlap: {:?} vs {:?}",
                        a.id.native_id,
                        b.id.native_id,
                        a.rect,
                        b.rect
                    );
                }
            }
        }
    }

    #[test]
    fn all_placements_stay_inside_work_area() {
        for count in [1usize, 3, 7, 20] {
            let plan = plan(&candidates(count), WORK, &LayoutConfig::default());
            for p in &plan.placements {
                assert!(
                    p.rect.reachable_in(&WORK, 64, 32),
                    "placement {:?} escaped the work area",
                    p.rect
                );
            }
        }
    }

    #[test]
    fn focus_wins_the_main_area_in_primary_side() {
        let mut input = candidates(4);
        input[3].focused = true;
        let config = LayoutConfig {
            mode: LayoutMode::PrimarySide,
            ..Default::default()
        };
        let plan = plan(&input, WORK, &config);
        let main = plan
            .placements
            .iter()
            .find(|p| p.id == id(3))
            .expect("focused window placed");
        let others: Vec<&Placement> = plan.placements.iter().filter(|p| p.id != id(3)).collect();
        assert!(main.rect.width() > others[0].rect.width());
    }

    #[test]
    fn primary_side_degrades_to_grid_when_side_is_too_narrow() {
        let narrow = Rect::new(0, 0, 700, 1040);
        let mut input = candidates(3);
        input[0].min_size = Some(Size::new(600, 300));
        let config = LayoutConfig {
            mode: LayoutMode::PrimarySide,
            ..Default::default()
        };
        let plan = plan(&input, narrow, &config);
        assert_eq!(plan.mode, LayoutMode::Grid);
        assert_eq!(plan.placements.len(), 3);
    }

    #[test]
    fn too_many_windows_cascades_instead_of_shrinking() {
        let plan = plan(&candidates(200), WORK, &LayoutConfig::default());
        assert!(plan.cascaded);
        assert!(!plan.placements.is_empty());
        for p in &plan.placements {
            assert!(p.rect.width() >= LayoutConfig::default().fallback_min.width);
            assert!(p.rect.reachable_in(&WORK, 64, 32));
        }
        assert!(!plan.unplaced.is_empty(), "overflow must be reported");
    }

    #[test]
    fn minimum_size_is_respected_even_when_the_cell_is_smaller() {
        let mut input = candidates(4);
        for c in &mut input {
            c.min_size = Some(Size::new(700, 500));
        }
        let plan = plan(&input, WORK, &LayoutConfig::default());
        for p in &plan.placements {
            assert!(p.rect.width() >= 700);
            assert!(p.rect.height() >= 500);
        }
    }

    #[test]
    fn keep_layout_maps_relative_positions_and_sizes() {
        // Two 1920x1080 side by side, folding onto one 1920x1040 work area. A
        // window on the left screen's top quarter and one on the right screen's
        // bottom quarter must land on the target's top-left and bottom-right
        // quarters respectively, at their fractional sizes.
        let src = Rect::new(0, 0, 3840, 2160);
        let input = vec![
            candidate(1, Rect::new(0, 0, 960, 540)),
            candidate(2, Rect::new(2880, 1080, 3840, 2160)),
        ];
        let config = LayoutConfig {
            source_bounds: Some(src),
            ..Default::default()
        };
        // The mode default is now KeepLayout; spell it out anyway so the test
        // survives a future default change.
        let config = LayoutConfig {
            mode: LayoutMode::KeepLayout,
            ..config
        };
        let plan = plan(&input, WORK, &config);
        assert_eq!(plan.mode, LayoutMode::KeepLayout);
        assert_eq!(plan.placements.len(), 2);

        let a = plan.placement_for(&candidate(1, WORK).id).unwrap();
        // x: (0-0)*1920/3840 = 0, w: 960*1920/3840 = 480
        // y: (0-0)*1040/2160 = 0, h: 540*1040/2160 = 260
        assert_eq!(a.rect, Rect::new(0, 0, 480, 260));

        let b = plan.placement_for(&candidate(2, WORK).id).unwrap();
        // x: 2880*1920/3840 = 1440; y: 1080*1040/2160 = 520. The window is the
        // bottom half of the right screen, so its mapped height is half the
        // target's: 520, reaching the work area's bottom edge.
        assert_eq!(b.rect, Rect::new(1440, 520, 1920, 1040));
    }

    #[test]
    fn keep_layout_is_identity_when_source_equals_target() {
        // Gathering with the target already filling the desktop must move
        // nothing: the mapping is the identity.
        let input = vec![
            candidate(1, Rect::new(100, 200, 700, 600)),
            candidate(2, Rect::new(900, 50, 1500, 400)),
        ];
        let config = LayoutConfig {
            mode: LayoutMode::KeepLayout,
            source_bounds: Some(WORK),
            ..Default::default()
        };
        let plan = plan(&input, WORK, &config);
        assert_eq!(plan.placements[0].rect, input[0].current);
        assert_eq!(plan.placements[1].rect, input[1].current);
    }

    #[test]
    fn keep_layout_lifts_windows_below_their_minimum() {
        // A small window maps smaller still when two screens fold onto one.
        // Usability wins over fidelity: it is raised to its minimum and marked,
        // the same contract as the grid's clamped_to_min.
        let src = Rect::new(0, 0, 3840, 2160);
        let mut input = vec![candidate(1, Rect::new(0, 0, 960, 540))];
        input[0].min_size = Some(Size::new(700, 500));
        let config = LayoutConfig {
            mode: LayoutMode::KeepLayout,
            source_bounds: Some(src),
            ..Default::default()
        };
        let plan = plan(&input, WORK, &config);
        let p = &plan.placements[0];
        assert_eq!(
            (p.rect.width(), p.rect.height()),
            (700, 500),
            "mapped size 480x260 must be raised to the minimum"
        );
        assert!(p.clamped_to_min);
        assert!(p.rect.reachable_in(&WORK, 64, 32), "must stay on screen");
    }

    #[test]
    fn keep_layout_without_source_bounds_falls_back_to_the_grid() {
        // Nothing to map against, so the honest answer is the default
        // arrangement rather than an invented one.
        let input = candidates(4);
        let config = LayoutConfig {
            mode: LayoutMode::KeepLayout,
            source_bounds: None,
            ..Default::default()
        };
        let mapped = plan(&input, WORK, &config);
        let plain = plan(&input, WORK, &LayoutConfig::default());
        assert_eq!(mapped.mode, LayoutMode::Grid);
        assert_eq!(mapped.placements, plain.placements);
    }

    #[test]
    fn grid_puts_the_focused_window_first() {
        // The other half of the distinction: in grid mode the focused window takes
        // the first cell, which is the visible difference between the two modes.
        let mut input = vec![
            candidate(1, Rect::new(0, 400, 800, 600)),
            candidate(2, Rect::new(0, 0, 800, 200)),
            candidate(3, Rect::new(0, 200, 800, 400)),
        ];
        input[0].focused = true;
        let config = LayoutConfig {
            mode: LayoutMode::Grid,
            ..Default::default()
        };
        let order: Vec<u64> = plan(&input, WORK, &config)
            .placements
            .iter()
            .map(|p| p.id.native_id)
            .collect();
        assert_eq!(
            order,
            vec![1, 2, 3],
            "the focused window goes to the first cell"
        );
    }

    /// The minimum sizes measured on a real Windows 11 desktop, in physical pixels.
    ///
    /// Every one of them equals the window's current size, which is what a
    /// composition-based window reports when asked: `WM_GETMINMAXINFO` comes back
    /// with "I am this big now" rather than a limit the application enforces.
    fn desktop_of_eight() -> Vec<Candidate> {
        let sizes: [(Rect, Size); 8] = [
            (Rect::new(180, 180, 660, 283), Size::new(480, 103)),
            (Rect::new(152, 152, 484, 354), Size::new(332, 202)),
            (Rect::new(124, 124, 456, 326), Size::new(332, 202)),
            (Rect::new(40, 40, 376, 580), Size::new(336, 540)),
            (Rect::new(208, 208, 540, 410), Size::new(332, 202)),
            (Rect::new(68, 68, 400, 270), Size::new(332, 202)),
            (Rect::new(96, 96, 257, 339), Size::new(161, 243)),
            (Rect::new(12, 12, 628, 420), Size::new(616, 408)),
        ];
        sizes
            .into_iter()
            .enumerate()
            .map(|(index, (rect, min))| Candidate {
                id: id(index as u64),
                current: rect,
                min_size: Some(min),
                focused: false,
            })
            .collect()
    }

    #[test]
    fn minimums_that_equal_the_current_size_do_not_force_a_cascade() {
        // The bug: `plan` declined to lay anything out because no grid could satisfy
        // every window's self-reported minimum, and piled all eight diagonally
        // instead. A cascade overlaps - each window covers the previous one - so
        // gathering eight windows showed the user one.
        let plan = plan(&desktop_of_eight(), WORK, &LayoutConfig::default());
        assert!(
            !plan.cascaded,
            "a usable arrangement exists and must be used instead of a pile",
        );
        assert_eq!(plan.placements.len(), 8);
        assert!(plan.unplaced.is_empty());

        // Only the two genuinely large windows are squeezed; the rest keep their
        // own minimum.
        let squeezed = plan.placements.iter().filter(|p| p.clamped_to_min).count();
        assert!(squeezed <= 3, "squeezed {squeezed} of eight windows");
    }

    #[test]
    fn a_desktop_with_more_windows_than_the_floor_can_hold_still_cascades() {
        // The cascade keeps its purpose: when the work area cannot hold this many
        // windows even at the planner floor, placing them at their own sizes and
        // letting the user sort it out beats pretending they fit.
        let many = candidates(40);
        let plan = plan(&many, WORK, &LayoutConfig::default());
        assert!(
            plan.cascaded,
            "40 windows cannot tile a 1920x1040 work area"
        );
    }

    #[test]
    fn plan_is_deterministic() {
        let input = candidates(11);
        let a = plan(&input, WORK, &LayoutConfig::default());
        let b = plan(&input, WORK, &LayoutConfig::default());
        assert_eq!(a.placements, b.placements);
        assert_eq!(a.mode, b.mode);
    }

    #[test]
    fn degenerate_work_area_yields_no_placements() {
        let zero = Rect::new(0, 0, 0, 0);
        let plan = plan(&candidates(3), zero, &LayoutConfig::default());
        assert!(plan.placements.is_empty());
        assert_eq!(plan.unplaced.len(), 3);
    }

    #[test]
    fn negative_coordinates_are_handled() {
        let work = Rect::new(-1920, -100, 0, 940);
        let plan = plan(&candidates(4), work, &LayoutConfig::default());
        for p in &plan.placements {
            assert!(p.rect.reachable_in(&work, 64, 32));
            assert!(p.rect.left >= work.left);
            assert!(p.rect.right <= work.right);
        }
    }

    #[test]
    fn portrait_work_area_still_tiles() {
        let portrait = Rect::new(0, 0, 1080, 1920);
        let plan = plan(&candidates(6), portrait, &LayoutConfig::default());
        assert!(!plan.cascaded);
        for p in &plan.placements {
            assert!(p.rect.reachable_in(&portrait, 64, 32));
        }
    }
}

#[cfg(test)]
mod column_choice_tests {
    //! The preview exposed a real defect: 5 windows in a 1920x1032 area were laid
    //! out as 5 vertical slivers because "zero empty cells" outranked "usable
    //! aspect ratio". These cases pin the intuitive result for the small counts
    //! users actually hit.

    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1032);

    fn windows(count: usize) -> Vec<Candidate> {
        (0..count)
            .map(|i| Candidate {
                id: WindowId {
                    native_id: i as u64,
                    process_id: 1,
                    process_start: 1,
                },
                // A typical desktop window is landscape.
                current: Rect::new(0, 0, 1200, 800),
                min_size: None,
                focused: false,
            })
            .collect()
    }

    fn columns(plan: &Plan) -> usize {
        let mut lefts: Vec<i32> = plan.placements.iter().map(|p| p.rect.left).collect();
        lefts.sort_unstable();
        lefts.dedup();
        lefts.len()
    }

    #[test]
    fn five_windows_use_three_columns_not_five() {
        let plan = plan(&windows(5), WORK, &LayoutConfig::default());
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
        let plan = plan(&windows(4), WORK, &LayoutConfig::default());
        assert_eq!(columns(&plan), 2);
    }

    #[test]
    fn six_windows_use_three_columns() {
        let plan = plan(&windows(6), WORK, &LayoutConfig::default());
        assert_eq!(columns(&plan), 3);
    }

    #[test]
    fn nine_windows_use_three_columns() {
        let plan = plan(&windows(9), WORK, &LayoutConfig::default());
        assert_eq!(columns(&plan), 3);
    }
}
