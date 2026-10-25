import { useEffect, useId, useState } from "react";
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
 * Selecting a target during a gather requires confirmation because it moves
 * already gathered windows while keeping their original recovery positions.
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
  hasRecovery = false,
  onRefreshThumbnails,
}: {
  displays: DisplayDto[];
  settings: SettingsDto | null;
  target: TargetDto | null;
  thumbnails: Record<string, DisplayThumbnailDto>;
  busy: boolean;
  onSelect: (nativeId: string) => void;
  onIdentify: () => void;
  /** Advance the target to the next display, keeping the original baseline. */
  onSave: (patch: Partial<SettingsDto>) => void;
  hasRecovery?: boolean;
  onRefreshThumbnails?: () => void;
}) {
  const [proposedTarget, setProposedTarget] = useState<string | null>(null);
  useEffect(() => { setProposedTarget(null); }, [target?.native_id, hasRecovery]);
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
            <Button size="sm" onClick={onIdentify} disabled={busy}>{t("在屏幕上显示编号")}</Button>
            <Button size="sm" onClick={onRefreshThumbnails} disabled={busy}>{t("刷新画面")}</Button>
          </div>
        }
      >
        <DisplayCards
          displays={displays}
          target={target}
          thumbnails={thumbnails}
          onSelect={(nativeId) => {
            if (nativeId === target?.native_id && target.confirmed) return;
            if (hasRecovery) setProposedTarget(nativeId);
            else onSelect(nativeId);
          }}
          disabled={busy}
        />
        {proposedTarget ? (
          <div role="alertdialog" aria-label={t("更换目标显示器")} className="mt-3 space-y-2">
            <p className="text-xs">{t("将已整理的窗口移到显示器 {number}，并保留最初的恢复位置。", { number: displays.find((display) => display.native_id === proposedTarget)?.number ?? "—" })}</p>
            <Button autoFocus onClick={() => setProposedTarget(null)}>{t("取消")}</Button>
            <Button disabled={busy || !displays.some((display) => display.native_id === proposedTarget)} onClick={() => { onSelect(proposedTarget); setProposedTarget(null); }}>{t("移到这块屏")}</Button>
          </div>
        ) : null}
        <p className="mt-2.5 text-xs text-[var(--color-muted)]">{t("卡片显示 Unfy 编号，与屏幕上的编号一致。截图不可用时仍可按编号选择。")}</p>
        {targetGone ? (
          <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-warning)]">
            {t("保存的目标显示器已断开。在下面重新选择一块屏；确认之前 Unfy 不会搬动窗口。")}
          </p>
        ) : null}
        {targetWeak && !target?.confirmed ? (
          <p className="mt-2 text-xs text-[var(--color-warning)]">{t("这块显示器需要在当前桌面重新确认。请点击对应卡片；确认前不会整理窗口。")}</p>
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

        {/* Margin and gap only exist for the explicit grid. Showing them in the
            default mode offered two settings that changed nothing. */}
        {settings.layout_mode === "grid" ? (
          <>
            <div className="border-t border-[var(--color-border)]" />

            <SettingRow label={t("边距")} hint={t("窗口与屏幕边缘的距离")}>
              <NumberInput
                label={t("边距")}
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
                label={t("窗口间距")}
                value={settings.gap}
                min={0}
                max={200}
                suffix={t("像素")}
                disabled={busy}
                onCommit={(gap) => onSave({ gap })}
              />
            </SettingRow>
          </>
        ) : null}
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

export function NumberInput({
  value,
  label,
  min,
  max,
  suffix,
  disabled,
  onCommit,
}: {
  value: number;
  label: string;
  min: number;
  max: number;
  suffix: string;
  disabled: boolean;
  onCommit: (value: number) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState(String(value));
  useEffect(() => { setDraft(String(value)); }, [value]);
  return (
    <div className="flex items-center gap-1.5">
      <label htmlFor={id} className="sr-only">{label}</label>
      <input
        type="number"
        id={id}
        aria-label={label}
        value={draft}
        onChange={(event) => setDraft(event.currentTarget.value)}
        min={min}
        max={max}
        disabled={disabled}
        onBlur={(event) => {
          const commit = commitNumber(event.currentTarget.value, value, min, max);
          // Always write back, even when nothing changed: an empty or malformed
          // field would otherwise stay blank while a real value is in effect.
          setDraft(String(value));
          if (commit.changed) onCommit(commit.value);
        }}
        className="h-7 w-16 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 text-right text-xs tabular-nums outline-none focus:border-[var(--color-accent)]"
      />
      <span className="text-[11px] text-[var(--color-muted)]">{suffix}</span>
    </div>
  );
}
