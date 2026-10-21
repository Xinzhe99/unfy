import type { DisplayDto, TargetDto } from "../api";

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
  thumbnails: Record<string, string>;
  onSelect: (nativeId: string) => void;
  disabled?: boolean;
}) {
  if (displays.length === 0) {
    return (
      <p className="px-1 py-6 text-center text-xs text-[var(--color-muted)]">
        未检测到显示器。
      </p>
    );
  }

  // The backend already returns them in positional order with a `number` that
  // matches the on-screen badge. Sorting here as well was what let the two pages
  // disagree: this component sorted, the overview indexed the raw list.
  const ordered = displays;
  const layout = relativeLayout(ordered);

  // Each row gets a fixed height, and the box is that many rows tall. Deriving
  // the height from a percentage of the arrangement's aspect ratio produced a
  // 40 px card for a side-by-side pair, because two 16:9 monitors in one box
  // leave only a quarter of the height each. A pixel budget per row cannot
  // collapse: the layout above it decides only horizontal placement.
  const ROW_HEIGHT_REM = 9;
  return (
    <div
      className="relative w-full"
      style={{ height: `${ROW_HEIGHT_REM * layout.rows}rem` }}
    >
      {ordered.map((display, index) => {
        const selected = target?.native_id === display.native_id;
        const shot = thumbnails[display.native_id];
        const cell = layout.cells[index]!;

        return (
          <div
            key={display.native_id}
            className="absolute p-1"
            style={{
              left: `${cell.left * 100}%`,
              top: `${cell.top * 100}%`,
              width: `${cell.width * 100}%`,
              // Height stays proportional to the real display when there is more
              // than one row, but never below a readable minimum.
              height: `${Math.max(78, cell.height * 100)}%`,
            }}
          >
            <button
              type="button"
              onClick={() => onSelect(display.native_id)}
              disabled={disabled}
              aria-pressed={selected}
              aria-label={`显示器 ${display.number}${display.primary ? "，主显示器" : ""}，${display.width}×${display.height}`}
              className={`flex size-full flex-col overflow-hidden rounded-[var(--radius-control)] border text-left transition-colors disabled:cursor-not-allowed ${
                selected
                  ? "border-[var(--color-accent)] ring-1 ring-[var(--color-accent)]"
                  : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"
              }`}
            >
              <div className="relative min-h-0 flex-1 bg-[var(--color-surface-sunken)]">
                {shot ? (
                  <img
                    src={shot}
                    alt=""
                    className="size-full object-cover"
                    draggable={false}
                  />
                ) : (
                  <div className="flex size-full items-center justify-center text-[11px] text-[var(--color-muted)]">
                    无法截取画面
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
                    {display.primary ? `显示器 ${display.number}（主）` : `显示器 ${display.number}`}
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
                    使用中
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
function relativeLayout(displays: DisplayDto[]) {
  const minX = Math.min(...displays.map((d) => d.origin_x));
  const minY = Math.min(...displays.map((d) => d.origin_y));
  const maxX = Math.max(...displays.map((d) => d.origin_x + d.width));
  const maxY = Math.max(...displays.map((d) => d.origin_y + d.height));

  // One denominator for both axes keeps the aspect ratio honest: dividing each
  // axis by its own extent would stretch a stacked pair into a wide box.
  const extent = Math.max(1, maxX - minX, maxY - minY);

  // Vertical span measured in "rows" of the tallest display, so the caller can
  // size the container for a single row of cards or a stacked pair.
  const tallest = Math.max(...displays.map((d) => d.height));
  const rows = Math.max(1, Math.ceil((maxY - minY) / Math.max(1, tallest)));

  return {
    rows,
    boxWidth: (maxX - minX) / extent,
    boxHeight: (maxY - minY) / extent,
    cells: displays.map((d) => ({
      left: (d.origin_x - minX) / extent,
      top: (d.origin_y - minY) / extent,
      width: d.width / extent,
      height: d.height / extent,
    })),
  };
}
