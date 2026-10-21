import type { DisplayDto, SettingsDto, TargetDto } from "../api";
import { DisplayMap } from "../components/DisplayMap";
import { Button, Card, Segmented } from "../components/ui";

/**
 * Displays page: pick the target, choose a layout, tune spacing.
 *
 * Nothing here moves a window. Choosing a target only persists an identity; the
 * actual move happens from the overview page or the tray. That separation is
 * stated on the page because "select a display" sounds like it might rearrange
 * things immediately.
 */
export function DisplaysPage({
  displays,
  settings,
  target,
  busy,
  onSelect,
  onIdentify,
  onSave,
}: {
  displays: DisplayDto[];
  settings: SettingsDto | null;
  target: TargetDto | null;
  busy: boolean;
  onSelect: (nativeId: string) => void;
  onIdentify: () => void;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  const weakTarget = displays.find(
    (d) => d.native_id === target?.native_id && !d.restorable,
  );

  return (
    <div className="flex flex-col gap-4">
      <Card
        title="目标显示器"
        description="选择远程端能看到的那块屏。更换屏幕后需要重新选择；应用不会凭猜测自动切换。"
        aside={
          <Button size="sm" onClick={onIdentify} disabled={busy}>
            显示编号
          </Button>
        }
      >
        <DisplayMap
          displays={displays}
          target={target}
          onSelect={onSelect}
          disabled={busy}
        />
        <p className="mt-4 text-[11px] leading-relaxed text-[var(--color-muted)]">
          数字与「显示编号」时屏幕上出现的数字一致，方便对照远程画面确认。
        </p>
        {weakTarget ? (
          <p className="mt-3 rounded-[var(--radius-control)] border border-[var(--color-warning)]/35 bg-[var(--color-warning-soft)] px-3 py-2 text-xs text-[var(--color-warning)]">
            这块显示器的身份只能靠分辨率识别（可能被换成了同款）。恢复时会把窗口放回可见区域，但不会假装知道它原来是哪一块。
          </p>
        ) : null}
      </Card>

      <Card
        title="布局"
        description="窗口如何排列在目标屏上。空间不足时会自动降级，不会把窗口压成不可用的小格。"
      >
        <Segmented
          label="布局方式"
          value={settings.layout_mode}
          disabled={busy}
          onChange={(layout_mode) => onSave({ layout_mode })}
          options={[
            { value: "grid", label: "网格" },
            { value: "primary_side", label: "主次分区" },
            { value: "preserve_order", label: "保持顺序" },
          ]}
        />
        <p className="mt-2 text-xs leading-relaxed text-[var(--color-muted)]">
          {layoutDescription(settings.layout_mode)}
        </p>

        <div className="mt-4 grid grid-cols-2 gap-4">
          <NumberField
            label="边距"
            hint="窗口与屏幕边缘的距离"
            value={settings.margin}
            min={0}
            max={200}
            disabled={busy}
            onCommit={(margin) => onSave({ margin })}
          />
          <NumberField
            label="窗口间距"
            hint="相邻窗口之间的距离"
            value={settings.gap}
            min={0}
            max={200}
            disabled={busy}
            onCommit={(gap) => onSave({ gap })}
          />
        </div>
      </Card>
    </div>
  );
}

function layoutDescription(mode: SettingsDto["layout_mode"]): string {
  switch (mode) {
    case "primary_side":
      return "焦点窗口占据主区域（约 60% 宽度），其余窗口在侧栏按网格排列。";
    case "preserve_order":
      return "按窗口在原来屏幕上的位置顺序依次放入槽位，尽量保持阅读顺序。";
    default:
      return "等分排列，列数根据窗口数量与目标屏比例自动选择。";
  }
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
  // and typing "20" would otherwise save "2" and then "20".
  return (
    <label className="block">
      <span className="text-xs font-medium">{label}</span>
      <span className="mt-0.5 block text-[11px] text-[var(--color-muted)]">
        {hint}
      </span>
      <div className="mt-1.5 flex items-center gap-1.5">
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
          className="h-9 w-20 rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 text-[13px] tabular-nums outline-none focus:border-[var(--color-accent)]"
        />
        <span className="text-xs text-[var(--color-muted)]">像素</span>
      </div>
    </label>
  );
}
