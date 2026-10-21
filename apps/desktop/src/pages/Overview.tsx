import { stateLabels } from "../api";
import type {
  ActivityEntryDto,
  DisplayDto,
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
  onOpenDisplays,
  onExclude,
  onKeepLayout,
  onDismissError,
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
  onPause: (paused: boolean) => void;
  onRescue: () => void;
  onIdentify: () => void;
  onExport: () => void;
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
}) {
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
        title="当前状态"
        aside={
          paused ? (
            <Status tone="warning">已暂停</Status>
          ) : state === "error" ? (
            <Status tone="danger">需要处理</Status>
          ) : state === "degraded" ? (
            <Status tone="warning">部分待恢复</Status>
          ) : busyState || busy ? (
            // `busy` covers the window between the click and the engine's first
            // state change, which on a busy desktop can be a second of silence.
            <Status tone="accent">
              {stateLabels[state === "idle" ? "migrating" : state]}
            </Status>
          ) : hasWork ? (
            <Status tone="accent">布局已启用</Status>
          ) : (
            <Status tone="neutral">就绪</Status>
          )
        }
      >
        <div className="space-y-0.5">
          <Detail
            label="目标显示器"
            value={targetDisplayName(displays, target)}
            muted={!target}
          />
          <Detail
            label="已收拢窗口"
            value={captured > 0 ? captured : "—"}
            muted={captured === 0}
          />
          {pending > 0 ? (
            <Detail
              label="待恢复窗口"
              value={`${pending} 个`}
            />
          ) : null}
          <Detail label="布局" value={layoutLabel(settings?.layout_mode)} />
          <Detail
            label="远程检测"
            value={
              settings?.remote_active
                ? (settings.remote_evidence ?? "已检测到")
                : detectStatusLabel(settings)
            }
            muted={!settings?.remote_active}
          />
        </div>

        {target && !target.present ? (
          <div className="mt-2.5 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] px-3 py-2">
            <p className="text-xs leading-relaxed text-[var(--color-fg-soft)]">
              保存的目标显示器（{target.label}）已断开，整理时会改用主显示器。
            </p>
            <button
              type="button"
              onClick={onOpenDisplays}
              className="mt-1.5 inline-flex items-center gap-1 text-xs font-medium text-[var(--color-fg)] hover:underline"
            >
              重新选择目标显示器
              <IconArrowRight className="size-3" />
            </button>
          </div>
        ) : null}

        {status?.last_error ? (
          <div className="mt-2 flex items-start gap-2 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-danger)] px-3 py-2">
            <p className="min-w-0 flex-1 text-xs leading-relaxed text-[var(--color-fg-soft)]">
              上次操作失败：{humanError(status.last_error)}
            </p>
            <button
              type="button"
              onClick={onDismissError}
              className="shrink-0 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              知道了
            </button>
          </div>
        ) : null}

        {needsAttention ? (
          <p className="mt-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">
            {state === "error"
              ? "出现了无法自动处理的问题。恢复与救援仍然可用；如果反复失败，请导出诊断包。"
              : "有窗口没能恢复到原位。可以再试一次，或先关闭这些窗口。"}
          </p>
        ) : null}

        {singleDisplay && !hasWork ? (
          <p className="mt-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">
            当前只检测到一块显示器，没有需要整理的窗口。接上第二块屏、或远程端开始投送多屏后再来。
          </p>
        ) : null}

        <div className="mt-3 flex flex-wrap items-center gap-1.5">
          {blocked ? (
            <>
              <Button onClick={onRestore} disabled={busy}>
                尝试恢复
              </Button>
              <Button onClick={onRescue} disabled={busy || !supported}>
                救援越界窗口
              </Button>
              <Button
                variant="quiet"
                size="sm"
                onClick={onExport}
                disabled={busy}
              >
                导出诊断包
              </Button>
            </>
          ) : hasWork ? (
            <>
              <Button
                variant="primary"
                onClick={onRestore}
                disabled={busy || busyState}
                loading={busy || busyState}
              >
                {busy || busyState ? "正在恢复…" : "恢复本次布局"}
              </Button>
              <Button
                onClick={() => onPause(!paused)}
                disabled={busy || busyState}
              >
                {paused ? "取消暂停" : "暂停"}
              </Button>
              {/* The only way out when restoring is impossible: the user decides
                  the current arrangement is good enough. Without it the exit
                  barrier refuses to quit, forever. */}
              <Button
                variant="quiet"
                size="sm"
                onClick={onKeepLayout}
                disabled={busy || busyState}
              >
                保留当前布局
              </Button>
            </>
          ) : (
            <>
              <Button
                variant="primary"
                onClick={onMigrate}
                disabled={busy || !supported || singleDisplay}
                loading={busy}
              >
                {progress
                  ? `正在整理 ${progress.done}/${progress.total}…`
                  : busy
                    ? "正在整理…"
                    : "整理到目标屏"}
              </Button>
              <Button
                onClick={onPreview}
                disabled={busy || !supported}
                icon={<IconEye className="size-3.5" />}
              >
                预览
              </Button>
            </>
          )}
          <Button
            variant="quiet"
            size="sm"
            onClick={onIdentify}
            disabled={busy}
          >
            识别屏幕编号
          </Button>
        </div>
        <p className="mt-1.5 text-[11px] text-[var(--color-muted)]">
          识别编号会在每块屏幕中央短暂显示数字，用来对照远程画面确认是哪一块。
        </p>
      </Card>

      {preview ? (
        <Card
          title="布局预览"
          description="预览不会移动任何窗口。"
          aside={
            <Status tone="neutral">
              <IconLayout className="size-3" />
              {preview.placements.length} 个窗口
            </Status>
          }
        >
          <PlanPreview preview={preview} />
        </Card>
      ) : null}

      {lastRefused.length > 0 ? (
        <Card
          title="没有接受位置调整的应用"
          description="这些应用会自己纠正窗口位置。排除它们可以避免每次都重试。"
          aside={
            lastRefused.length > 1 ? (
              <Button
                size="sm"
                variant="quiet"
                disabled={busy}
                onClick={() => onExclude(lastRefused)}
              >
                全部排除
              </Button>
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
                >
                  排除
                </Button>
              </li>
            ))}
          </ul>
        </Card>
      ) : null}

      <Card
        title="最近活动"
        aside={
          activity.length > 0 ? (
            <span className="text-[11px] tabular-nums text-[var(--color-muted)]">
              最近 {activity.length} 次
            </span>
          ) : null
        }
      >
        {activity.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">
            还没有执行过操作。整理或恢复之后，这里会记录每次的结果。
          </p>
        ) : (
          <ul>
            {activity.slice(0, 8).map((entry) => (
              <li
                key={`${entry.at}-${entry.kind}-${entry.affected}`}
                className="flex items-baseline justify-between gap-3 border-t border-[var(--color-border)] py-1.5 first:border-t-0 first:pt-0"
              >
                <div className="min-w-0">
                  <span className="text-[13px]">{activityLabel(entry)}</span>
                  {entry.unsettled > 0 ? (
                    <span className="ml-1.5 text-[11px] text-[var(--color-warning)]">
                      {entry.unsettled} 个未处理
                    </span>
                  ) : null}
                  {entry.note ? (
                    <span className="ml-1.5 text-[11px] text-[var(--color-muted)]">
                      {entry.note}
                    </span>
                  ) : null}
                </div>
                <span className="shrink-0 text-[11px] tabular-nums text-[var(--color-muted)]">
                  {relativeTime(entry.at)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card title="工具">
        {supported ? null : (
          <EmptyState title="当前平台尚未实现窗口控制">
            界面可以浏览，但不会移动任何窗口。
          </EmptyState>
        )}
        <div className="flex flex-wrap gap-1.5">
          <Button size="sm" onClick={onRescue} disabled={busy || !supported}>
            救援越界窗口
          </Button>
          <Button size="sm" onClick={onExport} disabled={busy}>
            导出诊断包
          </Button>
        </div>
        <p className="mt-2 flex items-start gap-1.5 text-[11px] leading-relaxed text-[var(--color-muted)]">
          <IconInfo className="mt-px size-3 shrink-0" />
          「救援」把停留在不存在坐标上的窗口移回可见区域。诊断包只含版本、显示器分辨率与设置项，
          不含窗口标题、文件路径或屏幕画面。
        </p>
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
function targetDisplayName(
  displays: DisplayDto[],
  target: { native_id: string; present: boolean } | null | undefined,
): string {
  if (!target) return "跟随主显示器";
  const index = displays.findIndex((d) => d.native_id === target.native_id);
  if (index === -1) return target.present ? "已选择" : "不可用";
  const display = displays[index]!;
  return display.primary ? `显示器 ${index + 1}（主）` : `显示器 ${index + 1}`;
}

/**
 * Translate a command error into something a user can act on.
 *
 * The raw values are internal codes (`already_active`, `no_displays`); showing
 * them verbatim is the difference between "the app told me what to do" and "the
 * app printed a developer string".
 */
function humanError(raw: string): string {
  const incomplete = raw.match(/restore_incomplete:(\d+)/);
  if (incomplete) return `还有 ${incomplete[1]} 个窗口没能恢复原位`;
  // Journal failures are the one path that leaves the engine unable to act at
  // all, and the raw text is an English OS error. Say what it means and what the
  // user can do; the exact reason is in the diagnostics bundle.
  if (raw.includes("journal write failed") || raw.includes("journal is corrupt")) {
    return "无法写入恢复记录，为避免窗口位置丢失，整理已停止。请确认磁盘空间与目录权限，然后重启应用。";
  }
  switch (raw) {
    case "already_active":
      return "还有未完成的整理，请先恢复";
    case "paused":
      return "自动整理已暂停";
    case "no_displays":
      return "没有可用的显示器";
    case "restore_cancelled":
      return "恢复已被新的整理请求取消";
    case "no_displays_found":
      return "没有检测到显示器";
    default:
      return raw;
  }
}

/** One line per history entry: what happened and how many windows it touched. */
function activityLabel(entry: ActivityEntryDto): string {
  const name = {
    gather: "整理",
    restore: "恢复",
    rescue: "救援越界窗口",
    auto_gather: "自动整理",
    auto_restore: "自动恢复",
    pause: "暂停状态变更",
    keep_layout: "保留当前布局",
  }[entry.kind];
  const trigger =
    entry.trigger === "hotkey"
      ? "（快捷键）"
      : entry.trigger === "tray"
        ? "（托盘）"
        : "";
  if (entry.kind === "pause" || entry.kind === "keep_layout") {
    return `${name}${trigger}`;
  }
  return `${name}${trigger} · ${entry.affected} 个窗口`;
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
  if (sameDay) return `今天 ${time}`;
  return `${then.getMonth() + 1}月${then.getDate()}日 ${time}`;
}

function detectStatusLabel(settings: SettingsDto | null): string {
  if (!settings) return "—";
  if (settings.auto_detect === "off") return "已关闭";
  const level =
    settings.auto_detect === "conservative"
      ? "保守"
      : settings.auto_detect === "aggressive"
        ? "激进"
        : "均衡";
  if (!settings.auto_gather && !settings.auto_restore) return `监视中（${level}，未启用自动操作）`;
  return `监视中（${level}）`;
}

function layoutLabel(mode: SettingsDto["layout_mode"] | undefined): string {
  switch (mode) {
    case "primary_side":
      return "主次分区";
    case "preserve_order":
      return "保持顺序";
    default:
      return "网格";
  }
}
