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
 * If capture is unavailable (locked session, unsupported platform) the component
 * falls back to a neutral placeholder with the same number, so the card layout
 * and selection behaviour never change.
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

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
      {displays.map((display, index) => {
        const selected = target?.native_id === display.native_id;
        const shot = thumbnails[display.native_id];

        return (
          <button
            key={display.native_id}
            type="button"
            onClick={() => onSelect(display.native_id)}
            disabled={disabled}
            aria-pressed={selected}
            aria-label={`显示器 ${index + 1}${display.primary ? "，主显示器" : ""}，${display.width}×${display.height}`}
            className={`group overflow-hidden rounded-[var(--radius-control)] border text-left transition-colors disabled:cursor-not-allowed ${
              selected
                ? "border-[var(--color-accent)] ring-1 ring-[var(--color-accent)]"
                : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"
            }`}
          >
            <div className="relative aspect-video w-full bg-[var(--color-surface-sunken)]">
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
                  Its colour is fixed rather than themed: it sits on a screenshot,
                  whose brightness has nothing to do with the app theme. */}
              <span
                className={`absolute left-1.5 top-1.5 flex size-5 items-center justify-center rounded-full text-[11px] font-semibold ${
                  selected
                    ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                    : "bg-[var(--color-overlay)] text-[var(--color-overlay-fg)]"
                }`}
              >
                {index + 1}
              </span>
            </div>
            <div className="flex items-center justify-between gap-2 px-2.5 py-2">
              <div className="min-w-0">
                <p className="truncate text-xs font-medium">
                  {display.primary ? "主显示器" : `显示器 ${index + 1}`}
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
        );
      })}
    </div>
  );
}
