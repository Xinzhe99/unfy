import type { DisplayDto, SettingsDto, TargetDto } from "../api";
import { DisplayMap } from "../components/DisplayMap";

/**
 * Displays page: pick the target, choose a layout, tune spacing.
 *
 * Nothing here moves a window. Choosing a target only persists an identity; the
 * actual move happens from the overview page or the tray.
 */
export function DisplaysPage({
  displays,
  settings,
  target,
  busy,
  onSelect,
  onSave,
}: {
  displays: DisplayDto[];
  settings: SettingsDto | null;
  target: TargetDto | null;
  busy: boolean;
  onSelect: (nativeId: string) => void;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  const weakTarget = displays.find(
    (d) => d.native_id === target?.native_id && !d.restorable,
  );

  return (
    <div className="space-y-4">
      <section className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-5">
        <h2 className="text-sm font-semibold">目标显示器</h2>
        <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
          选择远程端能看到的那块屏。换了一块屏后需要重新选一次；应用不会凭猜测自动切换。
        </p>
        <div className="mt-4">
          <DisplayMap
            displays={displays}
            target={target}
            onSelect={onSelect}
            disabled={busy}
          />
        </div>
        {weakTarget ? (
          <p className="mt-4 rounded-lg border border-[var(--color-warning)]/40 bg-[var(--color-warning)]/8 p-3 text-xs text-[var(--color-warning)]">
            这块显示器的身份只能靠分辨率识别（可能被换成了同款）。恢复时会把窗口放回可见区域，但不会假装知道它原来是哪一块。
          </p>
        ) : null}
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">布局</h2>
        <div className="mt-3 grid gap-2" role="radiogroup" aria-label="布局方式">
          {(
            [
              {
                id: "grid" as const,
                title: "网格",
                detail: "等分排列，列数按窗口数量和目标屏比例自动选择。",
              },
              {
                id: "primary_side" as const,
                title: "主次分区",
                detail: "聚焦窗口占主区域，其余窗口排在侧栏。",
              },
              {
                id: "preserve_order" as const,
                title: "保持顺序",
                detail: "按窗口原来的位置顺序依次放入槽位，尽量保持阅读顺序。",
              },
            ] satisfies {
              id: SettingsDto["layout_mode"];
              title: string;
              detail: string;
            }[]
          ).map((option) => {
            const active = settings.layout_mode === option.id;
            return (
              <button
                key={option.id}
                type="button"
                role="radio"
                aria-checked={active}
                disabled={busy}
                onClick={() => onSave({ layout_mode: option.id })}
                className={`rounded-lg border p-3 text-left transition-colors disabled:opacity-50 ${
                  active
                    ? "border-[var(--color-accent)] bg-[var(--color-accent-soft)]"
                    : "border-[var(--color-border)] hover:border-[var(--color-accent)]/50"
                }`}
              >
                <span className="block text-xs font-medium">{option.title}</span>
                <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
                  {option.detail}
                </span>
              </button>
            );
          })}
        </div>

        <div className="mt-4 grid grid-cols-2 gap-4">
          <NumberField
            label="边距"
            hint="窗口与屏幕边缘的距离（像素）"
            value={settings.margin}
            min={0}
            max={200}
            disabled={busy}
            onCommit={(margin) => onSave({ margin })}
          />
          <NumberField
            label="窗口间距"
            hint="相邻窗口之间的距离（像素）"
            value={settings.gap}
            min={0}
            max={200}
            disabled={busy}
            onCommit={(gap) => onSave({ gap })}
          />
        </div>
      </section>
    </div>
  );
}

function NumberField({
  label,
  hint,
  value,
  min,
  max,
  disabled,
  onCommit,
}: {
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  disabled: boolean;
  onCommit: (value: number) => void;
}) {
  // Commit on blur rather than on every keystroke: each save rewrites the file,
  // and typing "20" would otherwise save "2" and "20".
  return (
    <label className="block">
      <span className="text-xs font-medium">{label}</span>
      <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
        {hint}
      </span>
      <input
        type="number"
        defaultValue={value}
        min={min}
        max={max}
        disabled={disabled}
        onBlur={(event) => {
          const next = Number(event.currentTarget.value);
          if (Number.isFinite(next) && next !== value) {
            onCommit(Math.min(max, Math.max(min, Math.round(next))));
          }
        }}
        className="mt-1.5 w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-xs"
      />
    </label>
  );
}
