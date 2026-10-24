import { t } from "../i18n";
import type { PreviewDto, RectDto } from "../api";
import { skipLabel } from "../api";

/**
 * Plan preview.
 *
 * Draws the target work area and every planned rectangle inside it, at the real
 * proportions. This is the only chance the user gets to see what will happen
 * before windows move, so it must not idealise the layout: if the planner had to
 * cascade because the screen is too small, the preview has to show overlapping
 * rectangles rather than a tidy grid.
 */
/**
 * Cells smaller than this, in work-area pixels, do not influence the label size.
 *
 * It is a first filter, not the final rule: the median of the surviving cells sets one
 * font size for the drawing, and each cell is then checked against that size. A cell
 * below this threshold could not hold even the 10 px floor, so letting it vote on the
 * size would drag every label down with it.
 */
const MIN_LABEL_CELL = 96;

export function PlanPreview({ preview }: { preview: PreviewDto }) {
  const work = preview.target_work;
  const placed = preview.placements.length;
  // One size for every label in the drawing. Sizing each label from its own cell made
  // `python` (a large window) print at twice the size of `Notepad #4` next to it, which
  // reads as a rendering fault rather than as emphasis. The smallest cell that still
  // gets a label sets the size for all of them.
  // Cells that overlap their neighbours get no labels at all. In a cascade every cell is
  // full size - they are merely offset - so a size test passes for all of them and the
  // labels print on top of each other into an unreadable block. Overlap is the condition
  // that actually decides whether a label can be read, not area.
  // "Overlap" means a real intersection, not a shared edge. Tiled cells touch on every
  // side, and the naive `a.left < b.right` test counts a shared border as overlap - which
  // stripped the labels from a perfectly ordinary six-window grid. A cell must intrude by
  // at least a few pixels to count.
  const OVERLAP_TOLERANCE = 4;
  const overlaps = (
    a: { left: number; top: number; width: number; height: number },
    b: { left: number; top: number; width: number; height: number },
  ) =>
    a.left < b.left + b.width - OVERLAP_TOLERANCE &&
    b.left < a.left + a.width - OVERLAP_TOLERANCE &&
    a.top < b.top + b.height - OVERLAP_TOLERANCE &&
    b.top < a.top + a.height - OVERLAP_TOLERANCE;
  // Which cells collide with a neighbour. One overlapping window should not silence the
  // labels on a grid that is otherwise clean; the drawing only drops labels wholesale when
  // most cells overlap (a cascade), where no per-cell rule could help.
  const collides = preview.placements.map((a, i) =>
    preview.placements.some((b, j) => i !== j && overlaps(a.rect, b.rect)),
  );
  // Only a cascade silences the whole drawing: there every cell is full-size and merely
  // offset, so all of them collide. A grid with one oversized window makes a handful of
  // cells collide, and that is not a reason to strip the labels from the other four.
  const overlapping = placed > 1 && collides.every(Boolean);
  const labelCells = overlapping
    ? []
    : preview.placements.filter(
        (p) => p.rect.width >= MIN_LABEL_CELL && p.rect.height >= MIN_LABEL_CELL,
      );
  // The median labelled cell sets the size, not the smallest: taking the minimum let a
  // single short cell shrink every label on the drawing to the floor. A cell that cannot
  // hold the chosen size simply goes unlabelled - that check is per cell below.
  //
  // The floor is 32 work-area units, not 10. The SVG is drawn at its real proportions and
  // then scaled down to the card: a 1920-wide viewBox inside roughly 600 px of card is a
  // factor of ~0.31, so 10 units renders at 3 px. 32 units renders at ~10 px, which is
  // the smallest size worth printing; the earlier "10" was legible in the DOM and
  // invisible on screen.
  const sizes = labelCells
    .map((p) => Math.min(p.rect.width, p.rect.height))
    .sort((a, b) => a - b);
  const median = sizes.length > 0 ? sizes[Math.floor(sizes.length / 2)]! : 0;
  const labelSize = median > 0 ? Math.max(32, Math.min(median * 0.3, 48)) : 32;
  const squeezed = preview.placements.filter((p) => p.clamped).length;

  // Two windows of the same application are indistinguishable in the drawing
  // otherwise, and "which explorer window goes where" is exactly the question a
  // user looking at two identical rectangles will ask.
  const duplicates = new Map<string, number>();
  for (const placement of preview.placements) {
    if (!placement.app_name) continue;
    duplicates.set(placement.app_name, (duplicates.get(placement.app_name) ?? 0) + 1);
  }

  if (placed === 0 && preview.skipped.length === 0) {
    return (
      <p className="rounded-[var(--radius-control)] border border-dashed border-[var(--color-border-strong)] px-4 py-6 text-center text-xs text-[var(--color-muted)]">
        {t("没有可整理的窗口")}
      </p>
    );
  }

  return (
    <div className="space-y-3">
      <div className="relative overflow-hidden rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)]">
        <svg
          viewBox={`${work.left} ${work.top} ${work.width} ${work.height}`}
          // Fill the card and let the intrinsic aspect ratio decide the height,
          // capped so a tall target does not push the rest of the page below the
          // fold. Fixed pixel dimensions made the drawing narrower than its
          // container, which read as unexplained margins inside the work area.
          className="mx-auto block h-auto w-full"
          style={{
            aspectRatio: `${work.width} / ${work.height}`,
            maxHeight: 300,
          }}
          role="img"
          aria-label={t("布局预览：{placed} 个窗口排入 {width}×{height} 的区域", {
            placed,
            width: work.width,
            height: work.height,
          })}
          preserveAspectRatio="xMidYMid meet"
        >
          {/* Work-area boundary, so the user can see the margin is respected. */}
          <rect
            x={work.left}
            y={work.top}
            width={work.width}
            height={work.height}
            fill="none"
            stroke="var(--color-border-strong)"
            strokeWidth={Math.max(2, work.width * 0.002)}
          />
          {preview.placements.map((placement, index) => (
            <g key={placement.window_id}>
              {/* SVG has no `title` attribute; a `<title>` child is the standard way
                  to give an element a tooltip, and the label inside the box is
                  shortened on purpose. Hovering reveals the full name and, for a
                  squeezed window, why it is drawn dashed. */}
              <title>{`${placement.app_name || t("窗口 {number}", { number: index + 1 })}${
                placement.clamped
                  ? t("：比它的格子需要更大，会压到相邻窗口")
                  : ""
              }`}</title>
              <rect
                x={placement.rect.left}
                y={placement.rect.top}
                width={placement.rect.width}
                height={placement.rect.height}
                rx={work.width * 0.004}
                fill="var(--color-accent)"
                fillOpacity={placement.clamped ? 0.06 : 0.12}
                stroke="var(--color-accent)"
                strokeOpacity={placement.clamped ? 0.5 : 1}
                strokeWidth={Math.max(2, work.width * 0.0025)}
                strokeDasharray={placement.clamped ? "8 6" : undefined}
              />
              {/* The label is sized against the cell, not the work area. It used to
                  use `work.width * 0.028`, which is 54 px on a 1920-wide screen
                  regardless of how many windows there are - so fifty-seven cells drew
                  fifty-seven 54 px labels into a few hundred pixels and the preview
                  became a grey smear. Below a legible threshold the labels are dropped
                  and the rectangles speak for themselves; the `<title>` still names
                  every window on hover. */}
              {!overlapping &&
              !collides[index] &&
              placement.rect.height >= labelSize * 2.2 &&
              placement.rect.width >= labelSize * 3 ? (
              <text
                x={placement.rect.left + placement.rect.width / 2}
                // A squeezed window is drawn larger than its cell and therefore
                // covers its neighbours. Keeping its label in the middle printed it
                // on top of the label underneath, and both became unreadable - the
                // drawing then showed a smudge exactly where the user needs to see
                // which application is the problem. Sitting near the bottom edge
                // separates the two, because the overlap grows upwards from the
                // cell's top-left corner.
                y={
                  placement.clamped
                    ? placement.rect.top + placement.rect.height - Math.max(work.width * 0.022, 8)
                    : placement.rect.top + placement.rect.height / 2
                }
                textAnchor="middle"
                dominantBaseline={placement.clamped ? "auto" : "central"}
                fontSize={labelSize}
                fill="var(--color-fg-soft)"
                fontFamily="inherit"
                // A squeezed rectangle is drawn over its neighbours, so two labels
                // can land on the same few pixels no matter where each is anchored -
                // moving the squeezed one to the bottom fixed one collision and left
                // another. A thin outline in the card's own colour keeps whichever
                // label is on top legible instead of relying on the geometry to
                // separate them.
                stroke="var(--color-surface)"
                strokeWidth={Math.max(work.width * 0.006, 2)}
                paintOrder="stroke"
                strokeLinejoin="round"
              >
                {/* The application name is what makes the drawing useful: a grid
                    of numbers tells the user nothing about where their work will
                    land. Long names are truncated by characters rather than with
                    SVG clipping, because a clipped word looks like a rendering
                    bug. */}
                {label(placement.app_name, index, duplicates)}
              </text>
              ) : null}
            </g>
          ))}
        </svg>
      </div>

      {preview.target_present ? null : (
        <p className="rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] px-3 py-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">
          {t("这份预览的目标显示器已经不可用，请重新选择后再整理。")}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[var(--color-muted)]">
        <span>
          {t("将移动 {count} 个窗口", { count: placed })}
        </span>
        {preview.cascaded ? (
          <span className="text-[var(--color-warning)]">
            {t("空间不足，窗口将叠放")}
          </span>
        ) : null}
        {preview.unplaced.length > 0 ? (
          <span className="text-[var(--color-warning)]">
            {t("{count} 个窗口放不下，保持原位", { count: preview.unplaced.length })}
          </span>
        ) : null}
        {squeezed > 0 ? (
          <span>
            {/* Dashed cells already meant this; without a legend the drawing just
                looked wrong. The count matters: "one of eight will overlap" is a
                different decision from "most of them will". */}
            {t("虚线框的窗口比格子需要更大，会略微压到相邻窗口（{count} 个）", {
              count: squeezed,
            })}
          </span>
        ) : null}
      </div>

      {preview.skipped.length > 0 ? <SkippedList skipped={preview.skipped} /> : null}
    </div>
  );
}

/**
 * What to draw inside a rectangle.
 *
 * A short application name, with a `#2` suffix when the same application owns
 * several windows in this plan. Falls back to the plan position when the platform
 * could not read a name (elevated processes deny the query).
 */
function label(appName: string, index: number, duplicates: Map<string, number>): string {
  if (!appName) return t("窗口 {number}", { number: index + 1 });
  const name = shortName(appName);
  return (duplicates.get(appName) ?? 0) > 1 ? `${name} #${index + 1}` : name;
}

/**
 * Executable name without its extension, trimmed to fit a small rectangle.
 *
 * `WindowsTerminal.exe` is the one the user recognises in the taskbar, but it
 * does not fit inside a 60 px wide box at any readable size; `WindowsT…` does,
 * and the numbered list underneath always carries the full name.
 */
function shortName(appName: string): string {
  if (!appName) return "";
  const base = appName.replace(/\.exe$/i, "");
  return base.length > 10 ? `${base.slice(0, 9)}…` : base;
}

function SkippedList({ skipped }: { skipped: PreviewDto["skipped"] }) {
  // Group by reason and name the applications, because "已排除此应用" without the
  // name leaves the user unable to confirm the rule matched what they expected.
  // Reasons that are not about a specific application stay as a count.
  const byReason = new Map<string, Map<string, number>>();
  for (const item of skipped) {
    const apps = byReason.get(item.reason) ?? new Map<string, number>();
    if (item.app_name) {
      apps.set(item.app_name, (apps.get(item.app_name) ?? 0) + 1);
    }
    byReason.set(item.reason, apps);
  }

  const entries = [...byReason.entries()].sort(
    (a, b) =>
      countOf(b[1]) - countOf(a[1]) || a[0].localeCompare(b[0]),
  );

  return (
    <details className="group text-xs text-[var(--color-muted)]">
      <summary className="cursor-pointer select-none list-none rounded-[var(--radius-control)] px-1 py-1 hover:text-[var(--color-fg)]">
        <span className="inline-block transition-transform group-open:rotate-90">
          ›
        </span>{" "}
        {t("已跳过 {count} 个窗口", { count: skipped.length })}
      </summary>
      <ul className="mt-1 space-y-1 pl-4">
        {entries.map(([reason, apps]) => (
          <li key={reason}>
            {t("{reason} · {count} 个窗口", {
              reason: skipLabel(reason),
              count: countOf(apps),
            })}
            {apps.size > 0 ? (
              <span className="text-[var(--color-muted)]/70">
                {" "}
                {t("（{apps}）", { apps: appNames(apps) })}
              </span>
            ) : null}
          </li>
        ))}
      </ul>
    </details>
  );
}

/**
 * The application names beside a skip reason.
 *
 * Both the separator and the "and more" marker are locale-specific, so neither is
 * written inline: English needs ", " where Chinese needs "、".
 */
function appNames(apps: Map<string, number>): string {
  const shown = [...apps.keys()].slice(0, 4);
  const list = shown.join(t("、"));
  return apps.size > shown.length ? t("{list} 等", { list }) : list;
}

function countOf(apps: Map<string, number>): number {
  let total = 0;
  for (const count of apps.values()) total += count;
  return total;
}

/** Convert a rect to a CSS-friendly string; used by tests and future overlays. */
export function rectLabel(rect: RectDto): string {
  return `${rect.width}×${rect.height} @ ${rect.left},${rect.top}`;
}
