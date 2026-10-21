import type { DisplayDto, SettingsDto, TargetDto } from "../api";
import { DisplayCards } from "../components/DisplayCards";
import { Button, Card, Segmented, SettingRow } from "../components/ui";

/**
 * Displays page.
 *
 * The picker shows a live screenshot of each monitor: a resolution list cannot
 * distinguish two identical panels, and the whole task is "which screen is the
 * remote showing". Nothing on this page moves a window — choosing a target only
 * persists an identity.
 */
export function DisplaysPage({
  displays,
  settings,
  target,
  thumbnails,
  busy,
  onSelect,
  onIdentify,
  onSave,
}: {
  displays: DisplayDto[];
  settings: SettingsDto | null;
  target: TargetDto | null;
  thumbnails: Record<string, string>;
  busy: boolean;
  onSelect: (nativeId: string) => void;
  onIdentify: () => void;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  const weak = displays.find(
    (d) => d.native_id === target?.native_id && !d.restorable,
  );

  return (
    <>
      <Card
        title="目标显示器"
        description="选择远程端能看到的那块屏。更换屏幕后需要重新选择。"
        aside={
          <Button size="sm" onClick={onIdentify} disabled={busy}>
            在屏幕上显示编号
          </Button>
        }
      >
        <DisplayCards
          displays={displays}
          target={target}
          thumbnails={thumbnails}
          onSelect={onSelect}
          disabled={busy}
        />
        <p className="mt-2.5 text-[11px] leading-relaxed text-[var(--color-muted)]">
          卡片上的数字与「在屏幕上显示编号」时出现的一致，用远程画面里的数字对照即可。
        </p>
        {weak ? (
          <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-warning)]">
            这块显示器的身份只能靠分辨率识别，恢复时会放回可见区域，而不是精确的原位置。
          </p>
        ) : null}
      </Card>

      <Card
        title="布局"
        description="窗口如何排列在目标屏上。空间不足时自动降级，不会压成不可用的小格。"
      >
        <SettingRow label="排列方式" hint={layoutDescription(settings.layout_mode)}>
          <Segmented
            label="布局方式"
            value={settings.layout_mode}
            disabled={busy}
            onChange={(layout_mode) => onSave({ layout_mode })}
            options={[
              { value: "grid", label: "网格" },
              { value: "primary_side", label: "主次" },
              { value: "preserve_order", label: "顺序" },
            ]}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label="边距" hint="窗口与屏幕边缘的距离">
          <NumberInput
            value={settings.margin}
            min={0}
            max={200}
            suffix="像素"
            disabled={busy}
            onCommit={(margin) => onSave({ margin })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label="窗口间距" hint="相邻窗口之间的距离">
          <NumberInput
            value={settings.gap}
            min={0}
            max={200}
            suffix="像素"
            disabled={busy}
            onCommit={(gap) => onSave({ gap })}
          />
        </SettingRow>
      </Card>
    </>
  );
}

function layoutDescription(mode: SettingsDto["layout_mode"]): string {
  switch (mode) {
    case "primary_side":
      return "焦点窗口占主区域，其余在侧栏排列。";
    case "preserve_order":
      return "按原屏幕上的位置顺序依次放入。";
    default:
      return "等分排列，列数按窗口数量自动选择。";
  }
}

function NumberInput({
  value,
  min,
  max,
  suffix,
  disabled,
  onCommit,
}: {
  value: number;
  min: number;
  max: number;
  suffix: string;
  disabled: boolean;
  onCommit: (value: number) => void;
}) {
  // Commit on blur, not per keystroke: each save rewrites the file, so typing "20"
  // would otherwise persist "2" first.
  return (
    <div className="flex items-center gap-1.5">
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
        className="h-7 w-16 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 text-right text-xs tabular-nums outline-none focus:border-[var(--color-accent)]"
      />
      <span className="text-[11px] text-[var(--color-muted)]">{suffix}</span>
    </div>
  );
}
