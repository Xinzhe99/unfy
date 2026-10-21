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
    /// Grid, but slot order follows the windows' original spatial order.
    PreserveOrder,
}

impl LayoutMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LayoutMode::Grid => "grid",
            LayoutMode::PrimarySide => "primary_side",
            LayoutMode::PreserveOrder => "preserve_order",
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

    // Deterministic order: focused first, then original spatial order, then id.
    // Spatial order is (top, left) so a horizontal row keeps its left-to-right
    // reading order on every platform.
    ordered.sort_by_key(|c| {
        (
            !c.focused,
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
        return cascade(&ordered, &usable, config, &effective_min);
    }

    match config.mode {
        LayoutMode::Grid => grid(&ordered, &usable, config, &effective_min, LayoutMode::Grid),
        LayoutMode::PreserveOrder => grid(
            &ordered,
            &usable,
            config,
            &effective_min,
            LayoutMode::PreserveOrder,
        ),
        LayoutMode::PrimarySide => primary_side(&ordered, &usable, config, &effective_min),
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

        // Prefer fewer minimum-size violations, then fewer empty cells, then a
        // cell aspect ratio close to the original windows' average.
        let avg_ratio = average_aspect(ordered);
        let cell_ratio = f64::from(cell_w) / f64::from(cell_h);
        let aspect_penalty = ((cell_ratio - avg_ratio).abs() * 100.0) as i64;

        let score = violations * 1_000_000 + empty * 1_000 + aspect_penalty;
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
    fn preserve_order_keeps_spatial_sequence() {
        // Three windows: top, middle, bottom in the source display.
        let input = vec![
            candidate(1, Rect::new(0, 400, 800, 600)),
            candidate(2, Rect::new(0, 0, 800, 200)),
            candidate(3, Rect::new(0, 200, 800, 400)),
        ];
        let config = LayoutConfig {
            mode: LayoutMode::PreserveOrder,
            ..Default::default()
        };
        let plan = plan(&input, WORK, &config);
        // Reading order of slots is row-major, so the top window must come first.
        let order: Vec<u64> = plan.placements.iter().map(|p| p.id.native_id).collect();
        assert_eq!(order, vec![2, 3, 1]);
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
