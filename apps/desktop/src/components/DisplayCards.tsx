import { t } from "../i18n";
import type { DisplayDto, DisplayThumbnailDto, TargetDto } from "../api";

/**
 * Display picker with a real screenshot per monitor.
 *
 * A list of resolutions cannot answer "which one is the remote showing?" — two
 * identical monitors produce two identical rows. The thumbnail is the actual
 * screen content, and the number drawn on it matches the on-screen badge that
 * `identify_displays` flashes, so the user can look at the remote window, read a
 * number, and pick the matching card.
 *
 * The cards are laid out where the displays actually are. A plain grid says
 * nothing about whether the second screen is to the right or above, and that is
 * exactly the mental model the user is confirming when they choose a target.
 *
 * If capture is unavailable (locked session, unsupported platform) the component
 * falls back to a neutral placeholder with the same number, so the layout and
 * selection behaviour never change.
 */
export function DisplayCards({
  displays,
  target,
  thumbnails,
  onSelect,
  disabled,
}: {
  displays: DisplayDto[];
  target: TargetDto | null;
  thumbnails: Record<string, DisplayThumbnailDto>;
  onSelect: (nativeId: string) => void;
  disabled?: boolean;
}) {
  if (displays.length === 0) {
    return (
      <p className="px-1 py-6 text-center text-xs text-[var(--color-muted)]">
        {t("未检测到显示器。")}
      </p>
    );
  }

  // The backend already returns them in positional order with a `number` that
  // matches the on-screen badge. Sorting here as well was what let the two pages
  // disagree: this component sorted, the overview indexed the raw list.
  const ordered = displays;
  const layout = relativeLayout(ordered);

  return (
    <div
      className="w-full"
      style={{
        display: "grid",
        // One column per distinct x-origin, one row per distinct y-origin, sized by
        // how much of the desktop each track covers. A pair of 16:9 monitors side by
        // side therefore gets two equal columns and *one* row, and the cards agree
        // in height by construction rather than by a shared rem guess.
        gridTemplateColumns: layout.columnTemplate,
        gridTemplateRows: layout.rowTemplate,
        gap: "0.5rem",
      }}
    >
      {ordered.map((display, index) => {
        const selected = target?.native_id === display.native_id;
        const shot = thumbnails[display.native_id];
        const cell = layout.cells[index]!;

        return (
          <div
            key={display.native_id}
            className="min-w-0"
            style={{ gridColumn: cell.column, gridRow: cell.row }}
          >
            <button
              type="button"
              onClick={() => onSelect(display.native_id)}
              disabled={disabled}
              aria-pressed={selected}
              aria-label={
                t(display.primary ? "显示器 {number}（主）" : "显示器 {number}", {
                  number: display.number,
                }) +
                t("，") +
                `${display.width}×${display.height}`
              }
              className={`flex size-full flex-col overflow-hidden rounded-[var(--radius-control)] border text-left transition-colors disabled:cursor-not-allowed ${
                selected
                  ? "border-[var(--color-accent)] ring-1 ring-[var(--color-accent)]"
                  : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"
              }`}
            >
              {/* The screenshot box is the display's own shape. The card is as wide
                  as the grid track and this box sets its height, so a 16:9 monitor is
                  shown at 16:9 with no letterboxing and nothing cropped. */}
              <div
                className="relative w-full bg-[var(--color-surface-sunken)]"
                style={{
                  aspectRatio: `${display.width} / ${Math.max(1, display.height)}`,
                }}
              >
                {shot ? (
                  <>
                    {/* `object-contain`, not `cover`: the box is already the
                        display's ratio, so this fills it exactly, and if a capture
                        ever comes back at another ratio it is letterboxed rather than
                        silently cropped. Cropping was what made the picture disagree
                        with the resolution printed underneath it. */}
                    <img
                      src={shot.data_url}
                      alt=""
                      className="size-full object-contain"
                      draggable={false}
                    />
                  </>
                ) : (
                  <div className="flex size-full items-center justify-center text-[11px] text-[var(--color-muted)]">
                    {t("暂时无法获取预览，仍可按编号选择")}
                  </div>
                )}
                {/* The number matches the badge flashed on the physical screen.
                    Its colour is fixed rather than themed: it sits on a
                    screenshot, whose brightness has nothing to do with the app
                    theme. */}
                <span
                  className={`absolute left-1.5 top-1.5 flex size-5 items-center justify-center rounded-full text-[11px] font-semibold ${
                    selected
                      ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                      : "bg-[var(--color-overlay)] text-[var(--color-overlay-fg)]"
                  }`}
                >
                  {display.number}
                </span>
              </div>
              <div className="flex shrink-0 items-center justify-between gap-2 px-2.5 py-1.5">
                <div className="min-w-0">
                  <p className="truncate text-xs font-medium">
                    {t(display.primary ? "显示器 {number}（主）" : "显示器 {number}", { number: display.number })}
                  </p>
                  <p className="truncate text-[11px] tabular-nums text-[var(--color-muted)]">
                    {display.width}×{display.height}
                    {display.scale !== 1
                      ? ` · ${Math.round(display.scale * 100)}%`
                      : ""}
                  </p>
                </div>
                {selected ? (
                  <span className="shrink-0 text-[11px] font-medium text-[var(--color-fg-soft)]">
                    {t(target?.confirmed ? "已选择" : "需要确认")}
                  </span>
                ) : null}
              </div>
            </button>
          </div>
        );
      })}
    </div>
  );
}

/**
 * Map real display coordinates onto container fractions.
 *
 * Each cell keeps its display's proportions and relative position, shrunk to fit
 * one box. Coordinates are physical pixels and may be negative (a display to the
 * left of the primary has a negative x), so the origin is translated to the
 * top-left of the bounding box first.
 */
/**
 * Place the displays on a grid that mirrors where they physically are.
 *
 * Each distinct x-origin becomes a column and each distinct y-origin becomes a row,
 * weighted by the span that origin covers. Displays are assigned to the track their
 * origin falls in, not to the index of that origin in a sorted list, because a
 * monitor can sit at a negative offset (a left-hand display often does) and the
 * tracks then start before zero.
 *
 * The alternative - absolute percentages inside a container with a guessed height -
 * made a card's shape depend on that guess: the screenshot was cropped to it, so a
 * 16:9 monitor appeared at some other ratio.
 */
function relativeLayout(displays: DisplayDto[]) {
  // Rows and columns are found by overlap, not by exact origin. Two monitors side by
  // side rarely share a y-origin: one is 78 px lower here because its taskbar sits
  // somewhere else. Treating every distinct origin as its own track turned a
  // left/right pair into a diagonal, which is a picture of a desktop that does not
  // exist.
  const spans = displays.map((d) => ({
    id: d.native_id,
    left: d.origin_x,
    right: d.origin_x + d.width,
    top: d.origin_y,
    bottom: d.origin_y + d.height,
  }));

  const bands = (from: (s: (typeof spans)[number]) => number, to: (s: (typeof spans)[number]) => number) => {
    const sorted = [...spans].sort((a, b) => from(a) - from(b));
    const out: { start: number; end: number }[] = [];
    for (const span of sorted) {
      const last = out[out.length - 1];
      // Overlapping or touching: the same band.
      if (last && from(span) < last.end) {
        last.end = Math.max(last.end, to(span));
      } else {
        out.push({ start: from(span), end: to(span) });
      }
    }
    return out;
  };

  const columns = bands((s) => s.left, (s) => s.right);
  const rows = bands((s) => s.top, (s) => s.bottom);

  const trackOf = (bandList: { start: number; end: number }[], at: number) =>
    bandList.findIndex((b) => at >= b.start && at < b.end);

  // `minmax(0, Nfr)`, not bare `Nfr`: a bare `fr` is `minmax(auto, Nfr)`, whose
  // automatic minimum is the track's min-content size. A caption that happens to be
  // wider - "显示器 2（主）" plus the "使用中" label on the same row - then steals
  // width from its neighbour, so two identical monitors rendered at different sizes.
  const template = (bandList: { start: number; end: number }[]) =>
    bandList
      .map((b) => `minmax(0, ${Math.max(1, Math.round(b.end - b.start))}fr)`)
      .join(" ");

  return {
    columnTemplate: template(columns),
    rowTemplate: template(rows),
    cells: spans.map((span) => ({
      column: trackOf(columns, span.left) + 1,
      row: trackOf(rows, span.top) + 1,
    })),
  };
}
