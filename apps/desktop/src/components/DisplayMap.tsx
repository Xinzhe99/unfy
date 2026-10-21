import type { DisplayDto, TargetDto } from "../api";

/**
 * Display picker.
 *
 * Draws the displays at their real relative positions and sizes, because the
 * whole task is spatial: the user is looking at one physical screen and needs to
 * pick the matching rectangle. A vertical list of resolutions does not answer
 * "which one is the remote showing?"; a map that matches the OS arrangement does.
 *
 * Displays with no usable identity are marked, since choosing one means restore
 * can only fall back to a visible area rather than the exact original screen.
 */
export function DisplayMap({
  displays,
  target,
  onSelect,
  disabled,
}: {
  displays: DisplayDto[];
  target: TargetDto | null;
  onSelect: (nativeId: string) => void;
  disabled?: boolean;
}) {
  if (displays.length === 0) {
    return (
      <p className="text-xs text-[var(--color-muted)]">未检测到显示器。</p>
    );
  }

  // Normalise into a 0-based box so negative coordinates (a display to the left
  // of the primary) still render in the right relative position.
  const minX = Math.min(...displays.map((d) => d.origin_x));
  const minY = Math.min(...displays.map((d) => d.origin_y));
  const maxX = Math.max(...displays.map((d) => d.origin_x + d.width));
  const maxY = Math.max(...displays.map((d) => d.origin_y + d.height));
  const spanX = Math.max(maxX - minX, 1);
  const spanY = Math.max(maxY - minY, 1);

  const canvasW = 420;
  const canvasH = 190;
  const scale = Math.min(canvasW / spanX, canvasH / spanY);

  return (
    <div
      className="relative mx-auto"
      style={{ width: spanX * scale, height: spanY * scale }}
    >
      {displays.map((display, index) => {
        const selected = target?.native_id === display.native_id;
        const left = (display.origin_x - minX) * scale;
        const top = (display.origin_y - minY) * scale;
        const width = display.width * scale;
        const height = display.height * scale;

        return (
          <button
            key={display.native_id}
            type="button"
            onClick={() => onSelect(display.native_id)}
            disabled={disabled}
            aria-pressed={selected}
            aria-label={`显示器 ${index + 1}${display.primary ? "（主显示器）" : ""}，${display.width}×${display.height}`}
            title={`${display.label} · ${display.width}×${display.height} · ${Math.round(display.scale * 100)}%`}
            className={`absolute flex flex-col items-center justify-center gap-0.5 rounded-md border-2 transition-all disabled:cursor-not-allowed ${
              selected
                ? "border-[var(--color-accent)] bg-[var(--color-accent-soft)]"
                : "border-[var(--color-border-strong)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]/60"
            }`}
            style={{ left, top, width, height }}
          >
            <span
              className={`text-lg font-semibold leading-none ${selected ? "text-[var(--color-accent)]" : "text-[var(--color-fg-soft)]"}`}
            >
              {index + 1}
            </span>
            <span className="text-[10px] leading-tight text-[var(--color-muted)]">
              {display.width}×{display.height}
            </span>
            {display.primary ? (
              <span className="text-[10px] leading-tight text-[var(--color-muted)]">
                主屏
              </span>
            ) : null}
            {!display.restorable ? (
              <span
                aria-label="身份较弱"
                className="absolute right-1 top-1 size-1.5 rounded-full bg-[var(--color-warning)]"
              />
            ) : null}
            {selected ? (
              <span className="absolute -bottom-2 left-1/2 -translate-x-1/2 rounded-full bg-[var(--color-accent)] px-1.5 py-px text-[10px] font-medium text-[var(--color-accent-fg)]">
                目标
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
