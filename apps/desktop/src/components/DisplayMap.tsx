import type { DisplayDto, TargetDto } from "../api";

/**
 * Visual display picker.
 *
 * Each display is drawn to scale inside a shared bounding box so the spatial
 * arrangement matches what the user sees in the OS settings. Clicking selects a
 * target; the selected one is highlighted with a labelled border, never colour
 * alone.
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

  const maxWidth = Math.max(...displays.map((d) => d.width), 1);
  const totalHeight = displays.reduce((sum, d) => sum + d.height, 0);
  const canvasWidth = 320;
  const scale = canvasWidth / maxWidth;

  return (
    <div className="flex flex-wrap items-start gap-3">
      {displays.map((display) => {
        const selected = target?.native_id === display.native_id;
        const width = Math.max(64, Math.round(display.width * scale));
        const height = Math.max(
          40,
          Math.round((display.height / Math.max(totalHeight, 1)) * 160),
        );
        return (
          <button
            key={display.native_id}
            type="button"
            onClick={() => onSelect(display.native_id)}
            disabled={disabled}
            aria-pressed={selected}
            className={`group relative flex flex-col items-center justify-center rounded-lg border-2 transition-colors disabled:cursor-not-allowed ${
              selected
                ? "border-[var(--color-accent)] bg-[var(--color-accent-soft)]"
                : "border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]/50"
            }`}
            style={{ width, height }}
            title={`${display.label} · ${display.width}×${display.height}`}
          >
            <span className="text-xs font-medium">{display.label}</span>
            <span className="text-[10px] text-[var(--color-muted)]">
              {display.width}×{display.height} · {Math.round(display.scale * 100)}%
            </span>
            {display.primary ? (
              <span className="mt-0.5 text-[10px] text-[var(--color-muted)]">
                主显示器
              </span>
            ) : null}
            {selected ? (
              <span className="absolute -top-2 left-2 rounded-full bg-[var(--color-accent)] px-1.5 py-0.5 text-[10px] font-medium text-white">
                目标
              </span>
            ) : null}
            {!display.restorable ? (
              <span className="absolute -bottom-2 right-2 rounded-full bg-[var(--color-warning)] px-1.5 py-0.5 text-[10px] font-medium text-white">
                身份弱
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
