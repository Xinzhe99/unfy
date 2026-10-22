import { useState } from "react";

import { t } from "../i18n";
import { renderMessage, renderNotice, stateLabels } from "../api";
import type {
  ActivityEntryDto,
  DisplayDto,
  TargetDto,
  PreviewDto,
  SettingsDto,
  StatusDto,
} from "../api";
import { PlanPreview } from "../components/PlanPreview";
import { Button, Card, Detail, EmptyState, Status } from "../components/ui";
import { IconArrowRight, IconEye, IconInfo, IconLayout } from "../components/icons";

/**
 * Overview.
 *
 * One question per block, in the order a user asks it:
 *
 *  1. what is the current state and where will windows go;
 *  2. the single action that applies right now;
 *  3. what happened last time;
 *  4. the maintenance actions nobody needs daily.
 */
export function OverviewPage({
  status,
  settings,
  preview,
  displays,
  activity,
  progress,
  busy,
  onPreview,
  onMigrate,
  onRestore,
  onPause,
  onRescue,
  onIdentify,
  onExport,
  onPreviewDiagnostics,
  diagnosticsPreview,
  onOpenConfigDir,
  onOpenDisplays,
  onExclude,
  onKeepLayout,
  onDismissError,
  onToggleRemote,
  pendingApps,
}: {
  status: StatusDto | null;
  settings: SettingsDto | null;
  preview: PreviewDto | null;
  displays: DisplayDto[];
  activity: ActivityEntryDto[];
  /** Live count while a gather runs, or null. */
  progress: { done: number; total: number } | null;
  busy: boolean;
  onPreview: () => void;
  onMigrate: () => void;
  onRestore: () => void;
  /** Applications whose windows the last restore could not put back. */
  pendingApps: string[];
  onPause: (paused: boolean) => void;
  onRescue: () => void;
  onIdentify: () => void;
  onExport: () => void;
  /** Show the payload without writing a file (spec P0-15-A). */
  onPreviewDiagnostics: () => void;
  /** The payload, or null when not requested. */
  diagnosticsPreview: string | null;
  /** Reveal the config directory, which also holds the logs and diagnostics. */
  onOpenConfigDir: () => void;
  onOpenDisplays: () => void;
  /**
   * Add applications to the exclusion rules, from a report row.
   *
   * Takes a list even for the single-row case: calling it once per row would
   * issue one save per name, each built from the settings snapshot the caller
   * read before the first save landed, so only the last rule would survive.
   */
  onExclude: (appNames: string[]) => void;
  /** Accept the current positions and drop the journal. */
  onKeepLayout: () => void;
  /** Clear the "last operation failed" line. */
  onDismissError: () => void;
  /** Declare or end a remote session by hand. */
  onToggleRemote: () => void;
}) {
  // The full history is reference material; three rows answer "what just happened".
  const [expanded, setExpanded] = useState(false);
  const supported = status?.supported ?? false;
  const state = status?.state ?? "idle";
  const paused = status?.paused ?? false;
  const captured = status?.captured_windows ?? 0;
  // Windows in an unfinished transaction, including one the engine adopted from
  // a previous run. A user looking at "ready" while five windows are displaced
  // is the failure this row prevents.
  const pending = status?.pending_windows ?? 0;
  const target = status?.target;

  // `degraded` means the journal is still open: some windows are captured, some
  // could not be restored. Treating it as "idle" (the previous behaviour) showed
  // "就绪" and offered "整理到目标屏", which then failed with a transaction error.
  // Anything with pending work must offer restore first.
  const hasWork =
    state === "active" || state === "degraded" || state === "restoring";
  const busyState = state === "migrating" || state === "restoring";
  const needsAttention = state === "degraded" || state === "error";
  // In `Error` the engine refuses every new gather by design: the journal is not
  // durable, so writing another one would put the user's windows at risk. The
  // actions that still work are rescue and restore, and they must be what the
  // page offers - previously it showed "gather", the one button that cannot run.
  const blocked = state === "error";
  // With one display there is nowhere to gather to. Saying nothing leaves the
  // user pressing a button that reports "moved 0 windows", which reads as broken.
  const singleDisplay = (status?.display_count ?? 0) < 2;
  // Applications that refused the last gather, minus those already excluded.
  // Showing an application whose rule already exists would offer an action that
  // does nothing.
  const lastRefused = (settings?.last_refused_apps ?? []).filter(
    (app) => !settings?.excluded_apps.some((rule) => rule.toLowerCase() === app.toLowerCase()),
  );

  return (
    <>
      <Card
        title={t("当前状态")}
        aside={
          paused ? (
            <Status tone="warning">{t("已暂停")}</Status>
          ) : state === "error" ? (
            <Status tone="danger">{t("需要处理")}</Status>
          ) : state === "degraded" ? (
            <Status tone="warning">{t("部分待恢复")}</Status>
          ) : busyState || busy ? (
            // `busy` covers the window between the click and the engine's first
            // state change, which on a busy desktop can be a second of silence.
            <Status tone="live">
              {t(stateLabels[state === "idle" ? "migrating" : state])}
            </Status>
          ) : hasWork ? (
            <Status tone="live">{t("布局已启用")}</Status>
          ) : status === null ? (
            // Before the first status arrives, claiming the engine is ready is a
            // statement the page cannot make: it has not asked yet. The comment above
            // records the earlier version showing ready while a journal was still
            // open; this is the same mistake at the other end of the lifecycle.
            // Normally invisible, because the answer arrives within a frame - but on a
            // slow disk or a scanned executable it is the first thing the user reads.
            <Status tone="neutral">{t("正在载入…")}</Status>
          ) : (
            <Status tone="success">{t("就绪")}</Status>
          )
        }
      >
        <div className="space-y-0.5">
          <Detail
            label={t("目标显示器")}
            value={targetDisplayName(displays, target)}
            muted={!target}
          />
          {/* Only when it has something to say. An em dash on an idle engine was a
              row of pure noise between two rows that carry facts. */}
          {captured > 0 ? (
            <Detail label={t("已收拢窗口")} value={String(captured)} />
          ) : null}
          {pending > 0 ? (
            <Detail
              label={t("待恢复窗口")}
              value={t("{count} 个", { count: pending })}
            />
          ) : null}
          <Detail label={t("布局")} value={layoutLabel(settings?.layout_mode)} />
          <Detail
            label={t("远程检测")}
            value={
              settings?.remote_active
                ? settings.remote_evidence
                  ? renderMessage(settings.remote_evidence)
                  : t("已检测到")
                : detectStatusLabel(settings)
            }
            muted={!settings?.remote_active}
          />
        </div>


        {target && !target.present ? (
          <div className="mt-2.5 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] px-3 py-2">
            <p className="text-xs leading-relaxed text-[var(--color-fg-soft)]">
              {t("保存的目标显示器（{display}）已断开，整理时会改用主显示器。", {
                display: target.number
                  ? t("显示器 {number}", { number: target.number })
                  : t("已保存的显示器"),
              })}
            </p>
            <button
              type="button"
              onClick={onOpenDisplays}
              className="mt-1.5 inline-flex items-center gap-1 text-xs font-medium text-[var(--color-fg)] hover:underline"
            >{t("重新选择目标显示器")}<IconArrowRight className="size-3" />
            </button>
          </div>
        ) : null}

        {status?.settings_recovered ? (
          <div className="mt-2 flex items-start gap-2 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] px-3 py-2">
            <p className="min-w-0 flex-1 text-xs leading-relaxed text-[var(--color-fg-soft)]">
              {t("读取设置文件失败，已改用默认设置。原文件保留为 settings.json.invalid，可以手工修复后重启。")}
            </p>
          </div>
        ) : null}

        {status?.last_error ? (
          <div className="mt-2 flex items-start gap-2 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-danger)] px-3 py-2">
            <p className="min-w-0 flex-1 text-xs leading-relaxed text-[var(--color-fg-soft)]">
              {t("上次操作失败：{message}", {
                message: renderNotice(status.last_error),
              })}
            </p>
            <button
              type="button"
              onClick={onDismissError}
              className="shrink-0 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >{t("知道了")}</button>
          </div>
        ) : null}

        {needsAttention ? (
          <div className="mt-2 space-y-1.5">
            <p className="text-xs leading-relaxed text-[var(--color-fg-soft)]">
              {t(
                state === "error"
                  ? "出现了无法自动处理的问题。恢复与救援仍然可用；如果反复失败，请导出诊断包。"
                  : "有窗口没能恢复到原位。可以再试一次，或先关闭这些窗口。",
              )}
            </p>
            {/* Naming the application is the difference between an explanation and a
                puzzle: "a window did not come back" leaves the user hunting through
                the taskbar, and the backend already knows which one it was. */}
            {pendingApps.length > 0 ? (
              <div className="flex flex-wrap items-center gap-1.5 text-xs">
                <span className="text-[var(--color-muted)]">
                  {t("仍在原处：")}
                </span>
                {pendingApps.map((name) => (
                  <span
                    key={name}
                    className="rounded-[var(--radius-control)] border border-[var(--color-border)] px-1.5 py-0.5 font-mono text-[11px]"
                  >
                    {name}
                  </span>
                ))}
                <button
                  type="button"
                  onClick={() => onExclude(pendingApps)}
                  disabled={busy}
                  // No opacity change when `busy`: muted text on the card measures 5.03:1, and
                  // any fade drops it under the 4.5:1 floor (0.9 alone gives 4.34:1).
                  // The button is disabled for a few milliseconds to stop a double
                  // submit, not to signal that the action is unavailable.
                  className="text-[11px] font-medium text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                >
                  {t("排除这些应用")}
                </button>
              </div>
            ) : null}
          </div>
        ) : null}

        {!settings?.hotkey_gather &&
        settings?.auto_detect === "off" &&
        !hasWork ? (
          <p className="mt-2 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-border-strong)] px-3 py-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">{t("现在只能从本窗口或托盘触发整理。设置一个快捷键、或打开远程检测，之后就不必先切回这个窗口。")}</p>
        ) : null}


        <div className="mt-3 flex flex-wrap items-center gap-1.5">
          {blocked ? (
            <>
              <Button onClick={onRestore} disabled={busy}>{t("尝试恢复")}</Button>
              <Button onClick={onRescue} disabled={busy || !supported}>{t("救援越界窗口")}</Button>
              <Button
                variant="quiet"
                size="sm"
                onClick={onExport}
                disabled={busy}
              >{t("导出诊断包")}</Button>
            </>
          ) : hasWork ? (
            <>
              <Button
                variant="primary"
                onClick={onRestore}
                disabled={busy || busyState}
                loading={busy || busyState}
              >
                {busy || busyState ? t("正在恢复…") : t("恢复本次布局")}
              </Button>
              <Button
                onClick={() => onPause(!paused)}
                disabled={busy || busyState}
              >
                {t(paused ? "取消暂停" : "暂停")}
              </Button>
              {/* The only way out when restoring is impossible: the user decides
                  the current arrangement is good enough. Without it the exit
                  barrier refuses to quit, forever. */}
              <Button
                variant="quiet"
                size="sm"
                onClick={onKeepLayout}
                disabled={busy || busyState}
              >{t("保留当前布局")}</Button>
            </>
          ) : (
            <Button
              variant="primary"
              onClick={onMigrate}
              disabled={busy || !supported || singleDisplay}
              loading={busy}
            >
              {progress
                ? t("正在整理 {done}/{total}…", { done: progress.done, total: progress.total })
                : busy
                  ? t("正在整理…")
                  : t("整理到目标屏")}
            </Button>
          )}
          {/* Read-only, so it is offered whatever the engine is doing: the plan is
              the only way to see where the windows would land, and hiding it while a
              layout is active left the user with no way to look before acting. */}
          <Button
            variant={blocked || hasWork ? "primary" : "secondary"}
            size="sm"
            onClick={onPreview}
            disabled={busy || !supported}
            icon={<IconEye className="size-3.5" />}
          >{t("预览")}</Button>
          <Button
            variant="quiet"
            size="sm"
            onClick={onIdentify}
            disabled={busy}
          >{t("识别屏幕编号")}</Button>
          {/* A tray icon can be hidden by the shell and there is no keyboard route
              into a tray menu, so this is the only way to tell the app a session
              started. It used to be a text link under the status rows, which read as
              a footnote to a control nobody could find. */}
          <Button
            variant="quiet"
            size="sm"
            onClick={onToggleRemote}
            disabled={busy}
          >
            {t(settings?.remote_active ? "结束远程会话标记" : "标记为远程会话")}
          </Button>
        </div>
        {/* Directly under the controls it explains. The gather button is disabled
            with one display, and the sentence that says why used to sit between two
            detail rows above - easy to miss, and easy to read as part of the layout
            description rather than as the reason the button does nothing. */}
        {singleDisplay && !hasWork ? (
          <p className="mt-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">
            {t("当前只检测到一块显示器，没有需要整理的窗口。接上第二块屏、或远程端开始投送多屏后再来。")}
          </p>
        ) : null}
        <p className="mt-1.5 text-[11px] text-[var(--color-muted)]">{t("识别编号会在每块屏幕中央短暂显示数字，用来对照远程画面确认是哪一块。")}</p>
      </Card>

      {preview ? (
        <Card
          title={t("布局预览")}
          description={t("预览不会移动任何窗口。窗口有变化时会自动更新。")}
          aside={
            // A plain count, not a `Status`: the dot in that component means "state
            // of the engine", and a grey dot next to a window count reads as
            // "nothing is happening" rather than "this is how many there are".
            <span className="inline-flex items-center gap-1.5 text-xs tabular-nums text-[var(--color-muted)]">
              <IconLayout className="size-3.5" />
              {t("{count} 个窗口", { count: preview.placements.length })}
            </span>
          }
        >
          <PlanPreview preview={preview} />
        </Card>
      ) : null}

      {lastRefused.length > 0 ? (
        <Card
          title={t("没有接受位置调整的应用")}
          // Two things cause this, and the copy used to assert only one of them:
          // an application that snaps its own windows back, and a window whose
          // minimum size is larger than the cell it was given. The second is common
          // in terminals and IDEs, and telling those users "it corrects itself" is
          // simply wrong. Both lead to the same offer: a rule stops the retry.
          description={t("这些窗口没有接受新的位置：有的会自己纠正，有的大到放不进格子。排除它们可以避免每次都重试。")}
          aside={
            lastRefused.length > 1 ? (
              <Button
                size="sm"
                variant="quiet"
                disabled={busy}
                onClick={() => onExclude(lastRefused)}
              >{t("全部排除")}</Button>
            ) : null
          }
        >
          <ul className="space-y-1">
            {lastRefused.map((app) => (
              <li
                key={app}
                className="flex items-center justify-between gap-3 py-1"
              >
                <span className="min-w-0 truncate font-mono text-xs">
                  {app}
                </span>
                <Button
                  size="sm"
                  variant="quiet"
                  disabled={busy}
                  onClick={() => onExclude([app])}
                >{t("排除")}</Button>
              </li>
            ))}
          </ul>
        </Card>
      ) : null}

      <Card
        title={t("最近活动")}
        aside={
          activity.length > 0 ? (
            <span className="text-[11px] tabular-nums text-[var(--color-muted)]">
              {t("最近 {count} 次", { count: activity.length })}
            </span>
          ) : null
        }
      >
        {activity.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">{t("还没有执行过操作。整理或恢复之后，这里会记录每次的结果。")}</p>
        ) : (
          // Three rows by default, the rest behind a disclosure. The list is the
          // answer to "what did it just do", which three rows already give; the full
          // history is there for anyone who wants it.
          //
          // "All" means all. The button promises `activity.length` rows and the list
          // used to stop at 40, so a user with 58 entries pressed "show all 58" and
          // silently got 40 - eighteen records they could never reach from the
          // window. The store is capped at 200 entries, which the scroll container
          // handles comfortably.
          <>
          <ul>
            {activity.slice(0, expanded ? activity.length : 3).map((entry) => (
              <li
                key={`${entry.at}-${entry.kind}-${entry.affected}`}
                className="flex items-baseline justify-between gap-3 border-t border-[var(--color-border)] py-1.5 first:border-t-0 first:pt-0"
              >
                <div className="min-w-0">
                  <span className="text-[13px]">{activityLabel(entry)}</span>
                  {entry.unsettled > 0 ? (
                    <span className="ml-1.5 text-[11px] text-[var(--color-warning)]">
                      {t("{count} 个未处理", { count: entry.unsettled })}
                    </span>
                  ) : null}
                  {entry.note ? (
                    <span className="ml-1.5 text-[11px] text-[var(--color-muted)]">
                      {renderMessage(entry.note)}
                    </span>
                  ) : null}
                </div>
                <span className="shrink-0 text-[11px] tabular-nums text-[var(--color-muted)]">
                  {relativeTime(entry.at)}
                </span>
              </li>
            ))}
          </ul>
          {activity.length > 3 ? (
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              className="mt-1 text-[11px] font-medium text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              {expanded
                ? t("收起")
                : t("显示全部 {count} 条", { count: activity.length })}
            </button>
          ) : null}
          </>
        )}
      </Card>

      <Card title={t("工具")}>
        {supported ? null : (
          <EmptyState title={t("当前平台尚未实现窗口控制")}>
            {t("界面可以浏览，但不会移动任何窗口。")}
          </EmptyState>
        )}
        <div className="flex flex-wrap gap-1.5">
          <Button size="sm" onClick={onRescue} disabled={busy || !supported}>{t("救援越界窗口")}</Button>
          <Button size="sm" variant="quiet" onClick={onPreviewDiagnostics} disabled={busy}>{t("查看诊断内容")}</Button>
          <Button size="sm" onClick={onExport} disabled={busy}>{t("导出诊断包")}</Button>
          <Button size="sm" variant="quiet" onClick={onOpenConfigDir} disabled={busy}>{t("打开配置目录")}</Button>
        </div>
        <p className="mt-2 flex items-start gap-1.5 text-[11px] leading-relaxed text-[var(--color-muted)]">
          <IconInfo className="mt-px size-3 shrink-0" />
          {t("「救援」把停留在不存在坐标上的窗口移回可见区域。诊断包只含版本、显示器分辨率与设置项，不含窗口标题、文件路径或屏幕画面。")}
        </p>

        {diagnosticsPreview ? (
          <details className="mt-2 rounded-[var(--radius-control)] border border-[var(--color-border)]">
            <summary className="cursor-pointer select-none px-3 py-2 text-xs text-[var(--color-fg-soft)]">{t("诊断包内容（导出时就是这个文件）")}</summary>
            <pre className="max-h-64 overflow-auto border-t border-[var(--color-border)] px-3 py-2 font-mono text-[11px] leading-relaxed text-[var(--color-fg-soft)]">
              {diagnosticsPreview}
            </pre>
          </details>
        ) : null}
      </Card>
    </>
  );
}

/**
 * Friendly name for the target display.
 *
 * The engine's label is the device name (`\\.\DISPLAY1`), which means nothing
 * to a user looking at a physical screen. The picker numbers the same displays,
 * so the same number is used here — that is the one identifier the user can see
 * on their desk.
 */
export function targetDisplayName(
  displays: DisplayDto[],
  target: TargetDto | null | undefined,
): string {
  if (!target) return t("跟随主显示器");
  // Prefer the backend's number for the target itself: it is the live value when
  // the display is present and the saved one when it is not, so the name does not
  // change depending on whether the monitor is plugged in.
  if (target.number) {
    const display = displays.find((d) => d.native_id === target.native_id);
    return t(display?.primary ? "显示器 {number}（主）" : "显示器 {number}", {
      number: target.number,
    });
  }
  const display = displays.find((d) => d.native_id === target.native_id);
  if (!display) return t(target.present ? "已选择" : "不可用");
  return t(display.primary ? "显示器 {number}（主）" : "显示器 {number}", {
    number: display.number,
  });
}

/** One line per history entry: what happened and how many windows it touched. */
/** Human label for each activity kind the backend can send. */
const ACTIVITY_KINDS: Record<string, string> = {
  gather: "整理",
  restore: "恢复",
  rescue: "救援越界窗口",
  auto_gather: "自动整理",
  auto_restore: "自动恢复",
  pause: "暂停状态变更",
  keep_layout: "保留当前布局",
};

function activityLabel(entry: ActivityEntryDto): string {
  // `t(undefined)` returns undefined, which React renders as nothing - so a kind
  // added on the Rust side without a line here produced a row reading "· 5 个窗口"
  // with the verb missing. Showing the raw code is ugly but says what happened and
  // names the string that needs a label.
  const name = t(ACTIVITY_KINDS[entry.kind] ?? entry.kind);
  const trigger =
    entry.trigger === "hotkey"
      ? t("（快捷键）")
      : entry.trigger === "tray"
        ? t("（托盘）")
        : "";
  if (entry.kind === "pause" || entry.kind === "keep_layout") {
    return `${name}${trigger}`;
  }
  return t("{name}{trigger} · {count} 个窗口", { name, trigger, count: entry.affected });
}

/**
 * Clock time for today, date for anything older.
 *
 * A raw timestamp is unreadable at a glance, and "3 分钟前" for something that
 * happened yesterday is worse: the user cannot tell whether a restore happened
 * before or after they last touched the machine.
 */
function relativeTime(at: number): string {
  const then = new Date(at * 1000);
  const now = new Date();
  const sameDay =
    then.getFullYear() === now.getFullYear() &&
    then.getMonth() === now.getMonth() &&
    then.getDate() === now.getDate();
  const time = `${String(then.getHours()).padStart(2, "0")}:${String(
    then.getMinutes(),
  ).padStart(2, "0")}`;
  if (sameDay) return t("今天 {time}", { time });
  // The log keeps 200 entries, which on a machine used across a new year can hold
  // dates from both. "10月22日 18:19" then names two different days, so an older
  // year is spelled out. Months and days are compared rather than the year alone,
  // because "last December" is still a different year.
  if (then.getFullYear() !== now.getFullYear()) {
    return t("{year}年{month}月{day}日 {time}", {
      year: then.getFullYear(),
      month: then.getMonth() + 1,
      day: then.getDate(),
      time,
    });
  }
  return t("{month}月{day}日 {time}", {
    month: then.getMonth() + 1,
    day: then.getDate(),
    time,
  });
}

function detectStatusLabel(settings: SettingsDto | null): string {
  if (!settings) return "—";
  if (settings.auto_detect === "off") return t("已关闭");
  const level = t(
    settings.auto_detect === "conservative"
      ? "保守"
      : settings.auto_detect === "aggressive"
        ? "激进"
        : "均衡",
  );
  if (!settings.auto_gather && !settings.auto_restore)
    return t("监视中（{level}，未启用自动操作）", { level });
  return t("监视中（{level}）", { level });
}

function layoutLabel(mode: SettingsDto["layout_mode"] | undefined): string {
  switch (mode) {
    case "primary_side":
      return t("主次");
    case "preserve_order":
      return t("顺序");
    default:
      return t("网格");
  }
}
