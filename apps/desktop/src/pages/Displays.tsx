import { t } from "../i18n";
import { targetState } from "../lib/target";
import { commitNumber } from "../lib/numeric";
import type {
  DisplayThumbnailDto, DisplayDto, SettingsDto, TargetDto } from "../api";
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
  onCycle,
  onSave,
}: {
  displays: DisplayDto[];
  settings: SettingsDto | null;
  target: TargetDto | null;
  thumbnails: Record<string, DisplayThumbnailDto>;
  busy: boolean;
  onSelect: (nativeId: string) => void;
  onIdentify: () => void;
  /** Advance the target to the next display, keeping the original baseline. */
  onCycle: () => void;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">{t("正在读取设置…")}</p>;
  }

  // Both facts come from one place: the overview and this page used to test the
  // "target is gone" condition separately, and only one of them did it.
  const { gone: targetGone, weak: targetWeak } = targetState(target, displays);

  return (
    <>
      <Card
        title={t("目标显示器")}
        description={t("选择远程端能看到的那块屏。更换屏幕后需要重新选择。")}
        aside={
          <div className="flex gap-1.5">
            {displays.length > 1 ? (
              <Button size="sm" onClick={onCycle} disabled={busy}>{t("切换到下一块屏")}</Button>
            ) : null}
            <Button size="sm" onClick={onIdentify} disabled={busy}>{t("在屏幕上显示编号")}</Button>
          </div>
        }
      >
        <DisplayCards
          displays={displays}
          target={target}
          thumbnails={thumbnails}
          onSelect={onSelect}
          disabled={busy}
        />
        <p className="mt-2.5 text-[11px] leading-relaxed text-[var(--color-muted)]">{t("卡片上的数字与「在屏幕上显示编号」时出现的一致，用远程画面里的数字对照即可。")}</p>
        {targetGone ? (
          <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-warning)]">
            {t("保存的目标显示器已断开，整理时会改用主显示器。在下面重新选择一块屏即可恢复。")}
          </p>
        ) : null}
        {targetWeak ? (
          <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-warning)]">{t("这块显示器的身份只能靠分辨率识别，恢复时会放回可见区域，而不是精确的原位置。")}</p>
        ) : null}
      </Card>

      <Card
        title={t("布局")}
        description={t("窗口如何排列在目标屏上。空间不足时自动降级，不会压成不可用的小格。")}
      >
        <SettingRow label={t("排列方式")} hint={layoutDescription(settings.layout_mode)}>
          <Segmented
            label={t("布局方式")}
            value={settings.layout_mode}
            disabled={busy}
            onChange={(layout_mode) => onSave({ layout_mode })}
            options={[
              { value: "default", label: t("移到目标屏") },
              { value: "grid", label: t("网格") },
            ]}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label={t("边距")} hint={t("窗口与屏幕边缘的距离")}>
          <NumberInput
            value={settings.margin}
            min={0}
            max={200}
            suffix={t("像素")}
            disabled={busy}
            onCommit={(margin) => onSave({ margin })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label={t("窗口间距")} hint={t("相邻窗口之间的距离")}>
          <NumberInput
            value={settings.gap}
            min={0}
            max={200}
            suffix={t("像素")}
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
    case "default":
      // The name now describes the behaviour, and the sentence adds the two facts a
      // user cannot infer: their sizes survive, and windows already on the target
      // are left alone rather than being pulled across.
      return t("保留窗口大小和最大化状态，把其他屏的窗口移过来。目标屏上已有的窗口保持原样。");
    default:
      return t("等分排列，列数按窗口数量自动选择；焦点窗口放第一格。");
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
  //
  // The committed value is clamped, so the field has to show the clamped number:
  // it used to keep whatever was typed, and typing 999 left "999" on screen while
  // the setting held 200. Nothing else corrected it until the page was reloaded, so
  // the number the user was looking at was not the number in effect.
  return (
    <div className="flex items-center gap-1.5">
      <input
        type="number"
        defaultValue={value}
        min={min}
        max={max}
        disabled={disabled}
        onBlur={(event) => {
          const commit = commitNumber(event.currentTarget.value, value, min, max);
          // Always write back, even when nothing changed: an empty or malformed
          // field would otherwise stay blank while a real value is in effect.
          event.currentTarget.value = commit.display;
          if (commit.changed) onCommit(commit.value);
        }}
        className="h-7 w-16 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 text-right text-xs tabular-nums outline-none focus:border-[var(--color-accent)]"
      />
      <span className="text-[11px] text-[var(--color-muted)]">{suffix}</span>
    </div>
  );
}
