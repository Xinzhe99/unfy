import { useEffect, useState } from "react";

import { t } from "../i18n";
import { targetState } from "../lib/target";
import { renderMessage, renderNotice, skipLabel, stateLabels } from "../api";
import type {
  ActivityEntryDto,
  DisplayDto,
  TargetDto,
  PreviewDto,
  SettingsDto,
  StatusDto,
  WindowResultDto,
} from "../api";
import { PlanPreview } from "../components/PlanPreview";
import { Button, Card, Detail, EmptyState, Status } from "../components/ui";
import { IconArrowRight, IconEye, IconLayout } from "../components/icons";

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
  onOpenDisplays,
  onExclude,
  onKeepLayout,
  onDismissError,
  pendingApps,
  refusedApps,
  recoveryPending = false,
  onRecheck,
  latestResults,
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
  /**
   * Applications that refused the last gather, from its own report.
   *
   * Not read from settings: a persisted copy went stale as soon as a rule changed,
   * and the only list worth showing is the one from the run the user just watched.
   */
  refusedApps: string[];
  onPause: (paused: boolean) => void;
  onRescue: () => void;
  onIdentify: () => void;
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
  recoveryPending?: boolean;
  onRecheck?: () => void;
  latestResults?: WindowResultDto[] | null;
}) {
  // The full history is reference material; three rows answer "what just happened".
  const [expanded, setExpanded] = useState(false);
  const supported = status?.supported ?? false;
  const state = status?.state ?? "idle";
  const paused = status?.paused ?? false;
  // Captured windows are the whole transaction; `pending_windows` is what still
  // owes a restore. They are different numbers and the page must not use one as
  // the other: a finished restore has captured > 0 and pending 0 for a moment.
  const captured = status?.captured_windows ?? 0;
  // Windows in an unfinished transaction, including one the engine adopted from
  // a previous run. A user looking at "ready" while five windows are displaced
  // is the failure this row prevents.
  const pending = status?.pending_windows ?? 0;
  const target = status?.target;

  const hasWork = pending > 0 || captured > 0 || recoveryPending;
  const restorable = hasWork;
  const busyState = state === "migrating" || state === "restoring";
  const blocked = (status?.recovery_blockers ?? 0) > 0 || state === "error";
  const needsAttention = blocked || state === "degraded";
  // With one display there is nowhere to gather to. Saying nothing leaves the
  // user pressing a button that reports "moved 0 windows", which reads as broken.
  // One display is not, by itself, a reason to hide the gather action: windows can
  // still be outside the work area after a resolution change. The copy explains
  // what will happen; the button stays available so rescue is reachable.
  const singleDisplay = (status?.display_count ?? 0) < 2;
  // "Keep current layout" discards the recovery record. It is the one action on
  // this page that destroys the ability to undo, so it takes two clicks: the same
  // in-page pattern the settings reset uses, because the native dialog does not
  // render reliably inside the Tauri webview.
  const [keepArmed, setKeepArmed] = useState(false);
  useEffect(() => { setKeepArmed(false); }, [captured, pending]);
  // Applications that refused the last gather, minus those already excluded.
  // Showing an application whose rule already exists would offer an action that
  // does nothing.
  const targetIsGone = targetState(target, displays).gone;
  const lastRefused = refusedApps.filter(
    (app) => !settings?.excluded_apps.some((rule) => rule.toLowerCase() === app.toLowerCase()),
  );

  return (
    <>
      <Card
        title={t("当前状态")}
        aside={
          blocked ? (
            <Status tone="danger">{t("需要处理")}</Status>
          ) : state === "degraded" ? (
            <Status tone="warning">{t("仍有窗口待处理")}</Status>
          ) : busyState ? (
            <Status tone="live">{t(stateLabels[state])}</Status>
          ) : state === "active" ? (
            // The windows really are gathered and really do owe a restore. Calling
            // that "recovery records still need attention" made a successful gather
            // read as a fault, which is the same confusion the crash banner had.
            <Status tone="live">{t("布局已启用")}</Status>
          ) : hasWork ? (
            <Status tone="live">{t("恢复记录仍待处理")}</Status>
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
            <Detail label={t("已保存恢复位置")} value={String(captured)} />
          ) : null}
          {pending > 0 ? (
            <Detail
              label={t("待恢复窗口")}
              value={t("{count} 个", { count: pending })}
            />
          ) : null}
          <Detail label={t("布局")} value={layoutLabel(settings?.layout_mode)} />
          {paused ? <Status tone="warning">{t("自动整理已暂停")}</Status> : null}
          <Detail
            label={t("远程检测")}
            value={
              settings?.remote_unknown ? t("连接状态未知，自动操作已停止") : settings?.remote_active
                ? settings.remote_evidence
                  ? renderMessage(settings.remote_evidence)
                  : t("已检测到")
                : detectStatusLabel(settings, paused)
            }
            muted={!settings?.remote_active}
          />
        </div>


        {target?.present && !target.confirmed ? (
          <div role="status" className="mt-2 text-xs">
            <p>{t("这块显示器需要在当前桌面重新确认。请点击对应卡片；确认前不会整理窗口。")}</p>
            <Button size="sm" onClick={onOpenDisplays}>{t("重新选择目标显示器")}</Button>
          </div>
        ) : null}
        {targetIsGone && target ? (
          <div className="mt-2.5 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] px-3 py-2">
            <p className="text-xs leading-relaxed text-[var(--color-fg-soft)]">
              {t("保存的目标显示器（{display}）已断开。请重新选择，Unfy 不会改用到主显示器。", {
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
              {t("设置文件无法读取，原文件已保留。请查看诊断并修复后重启；自动操作已停止。")}
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
                  ? "恢复记录需要处理。请重试检查或恢复；记录会继续保留。"
                  : "有窗口仍需处理。可以重试恢复，或查看最近一次结果。",
              )}
            </p>
            {/* Naming the application is the difference between an explanation and a
                puzzle: "a window did not come back" leaves the user hunting through
                the taskbar, and the backend already knows which one it was.
                No exclusion offer here: excluding an application only changes what
                the *next* gather touches, and it does nothing for windows that are
                still out of place. Offering it as the main way out of a failure
                implied it was the fix. */}
            {pendingApps.length > 0 ? (
              <div className="flex flex-wrap items-center gap-1.5 text-xs">
                <span className="text-[var(--color-muted)]">
                  {t("仍待恢复：")}
                </span>
                {pendingApps.map((name) => (
                  <span
                    key={name}
                    className="rounded-[var(--radius-control)] border border-[var(--color-border)] px-1.5 py-0.5 font-mono text-[11px]"
                  >
                    {name}
                  </span>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}

        {!settings?.hotkey_gather &&
        settings?.auto_detect === "off" &&
        !restorable ? (
          <p className="mt-2 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-border-strong)] px-3 py-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">{t("现在只能从本窗口或托盘触发整理。设置一个快捷键、或打开远程检测，之后就不必先切回这个窗口。")}</p>
        ) : null}


        <div className="mt-3 flex flex-wrap items-center gap-1.5">
          {restorable || blocked ? (
            <>
              <Button variant="primary" onClick={onRestore} disabled={busy || busyState || !supported || !status?.has_permission} loading={state === "restoring"}>
                {state === "restoring" ? t("正在恢复…") : t("恢复本次布局")}
              </Button>
              {blocked ? <Button onClick={onRecheck} disabled={busy}>{t("重新检查")}</Button> : null}
              {!blocked ? (
                <>
                  <Button onClick={onMigrate} disabled={busy || busyState || !target?.confirmed || targetIsGone || !supported || !status?.has_permission}>{t("整理新窗口")}</Button>
                  <p className="w-full text-[11px] leading-relaxed text-[var(--color-muted)]">
                    {t("当前布局仍在生效：把之后打开或移出目标屏的窗口也整理过去，已整理的窗口保持不动。恢复记录会继续保留。")}
                  </p>
                </>
              ) : null}
            </>
          ) : !target?.confirmed || targetIsGone ? (
            <Button variant="primary" onClick={onOpenDisplays}>{t("选择目标显示器")}</Button>
          ) : (
            <Button variant="primary" onClick={onMigrate} disabled={busy || busyState || !supported || !status?.has_permission} loading={state === "migrating"}>
              {progress ? t("正在整理 {done}/{total}…", { done: progress.done, total: progress.total }) : state === "migrating" ? t("正在整理…") : t("整理到目标屏")}
            </Button>
          )}
          {/* Pause belongs here, not behind the collapsed "More" section. It is the
              one control a user reaches for when automation does something they did
              not expect, and hiding it made the app look like it could not be
              stopped at all. */}
          {settings?.auto_detect !== "off" ? (
            <Button onClick={() => onPause(!paused)} disabled={busy || busyState}>{t(paused ? "取消暂停" : "暂停")}</Button>
          ) : null}
          {/* Read-only, so it is offered whatever the engine is doing: the plan is
              the only way to see where the windows would land, and hiding it while a
              layout is active left the user with no way to look before acting. */}
          <Button
            variant="secondary"
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
        </div>
        {/* Directly under the controls it explains. The gather button is disabled
            with one display, and the sentence that says why used to sit between two
            detail rows above - easy to miss, and easy to read as part of the layout
            description rather than as the reason the button does nothing. */}
        {singleDisplay && !restorable ? (
          <p className="mt-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">
            {t("当前只有一块显示器；仍可找回越界窗口。")}
          </p>
        ) : null}
        <p className="mt-1.5 text-[11px] text-[var(--color-muted)]">{t("识别编号会在每块屏幕中央短暂显示数字，用来对照远程画面确认是哪一块。")}</p>
      </Card>

      {latestResults ? (
        <Card title={t("最近一次实际结果")}>
          <ul className="space-y-2 text-xs">
            {latestResults.map((result, index) => (
              <li key={`${result.window_id}:${index}`} className="space-y-1 border-b border-[var(--color-border)] pb-2 last:border-0">
                <p className="break-all" title={result.app_name ?? result.window_id}>{result.app_name ?? result.window_id}</p>
                <p>{t({ applied: "已移入", restored: "已恢复原位", kept_user_change: "保留外部改动", fallback_visible: "已安全可见，原屏恢复待处理", rejected: "窗口未接受位置调整，可重试", gone: "窗口已关闭，无需重试", skipped: "本次跳过", unknown: "无法确认结果，恢复记录保留" }[result.outcome] ?? result.outcome)}</p>
                {result.reason ? <p className="text-[var(--color-muted)]">{skipLabel(result.reason)}</p> : null}
              </li>
            ))}
          </ul>
        </Card>
      ) : null}
      {preview ? (
        <Card
          title={t("布局预览")}
          description={t("预览不会移动窗口；目标或设置变化后请重新预览。")}
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
          // simply wrong.
          //
          // Exclusion is described for what it does - it changes the next gather -
          // because it does not undo anything. A rule added here leaves the windows
          // that already moved exactly where they are, and they can still be
          // restored; saying otherwise would turn a housekeeping rule into a false
          // promise about the user's windows.
          description={t("这些窗口没有接受新的位置：有的会自己纠正，有的大到放不进格子。以后整理时跳过某个应用可以避免每次都重试；这只影响之后的整理，已经整理的窗口仍然可以恢复。")}
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
                {/* Per application only, and as a quiet secondary action: the user
                    names the one program they want left alone. There is no bulk
                    version, because "exclude everything that failed" was being
                    offered as the way out of a failure it cannot fix. */}
                <Button
                  size="sm"
                  variant="quiet"
                  disabled={busy}
                  onClick={() => onExclude([app])}
                >{t("不再整理此应用")}</Button>
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
            {activity.slice(0, expanded ? activity.length : 1).map((entry) => (
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
          {activity.length > 1 ? (
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

      <details>
        <summary className="cursor-pointer text-xs">{t("更多")}</summary>
      <Card title={t("工具")}>
        {supported ? null : (
          <EmptyState title={t("当前平台尚未实现窗口控制")}>
            {t("界面可以浏览，但不会移动任何窗口。")}
          </EmptyState>
        )}
        <div className="flex flex-wrap gap-1.5">
          {captured > 0 ? keepArmed ? (
            <div role="alertdialog" aria-label={t("放弃恢复记录")} className="space-y-2">
              <p className="text-xs">{t("这次整理的恢复位置将被放弃，之后不能用本次记录恢复。")}</p>
              <p className="text-xs">{t("影响 {count} 个窗口", { count: captured })}</p>
              <Button autoFocus onClick={() => setKeepArmed(false)}>{t("取消")}</Button>
              <Button variant="danger" disabled={busy || busyState} onClick={() => { setKeepArmed(false); onKeepLayout(); }}>{t("放弃恢复记录")}</Button>
            </div>
          ) : <Button size="sm" variant="quiet" disabled={busy || busyState} onClick={() => setKeepArmed(true)}>{t("保留当前位置并结束本次整理")}</Button> : null}
          <Button size="sm" onClick={onRescue} disabled={busy || !supported}>{t("救援越界窗口")}</Button>
        </div>
      </Card>
      </details>
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
  if (!target) return t("尚未选择目标显示器");
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

function detectStatusLabel(settings: SettingsDto | null, paused: boolean): string {
  if (!settings) return "—";
  if (settings.auto_detect === "off") return t("已关闭");
  // Paused stops the automation, not the detector: the row would otherwise claim
  // it is acting while the badge above says paused, which reads as two different
  // answers to the same question.
  if (paused) return t("监视中（已暂停，不会自动操作）");
  // There is only one level now: verified connection evidence. Naming it would be
  // a choice the user no longer has.
  if (!settings.auto_gather)
    return t("监视中（未启用自动操作）");
  return t("监视中");
}

function layoutLabel(mode: SettingsDto["layout_mode"] | undefined): string {
  // The default is not called "默认": that named a setting rather than describing
  // what happens to the windows. It moves the arrangement onto the target screen.
  switch (mode) {
    case "grid":
      return t("网格");
    default:
      return t("移到目标屏");
  }
}
