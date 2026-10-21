import { type ReactElement, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  api,
  errorMessage,
  type ActivityEntryDto,
  type HealthItemDto,
  formatBytes,
  type AppDto,
  type DisplayDto,
  type EngineStateName,
  type HotkeyStatusDto,
  type PendingWorkDto,
  type PreviewDto,
  type SettingsDto,
  type StatusDto,
  type UpdateStatusDto,
} from "./api";
import { ExclusionsPage } from "./pages/Exclusions";
import { GeneralPage } from "./pages/General";
import { completeOnboarding, Onboarding } from "./pages/Onboarding";
import { OverviewPage } from "./pages/Overview";
import { DisplaysPage } from "./pages/Displays";
import {
  IconDisplays,
  IconHome,
  IconInfo,
  IconSettings,
  IconShield,
} from "./components/icons";
import { Status } from "./components/ui";

export type Page = "overview" | "displays" | "exclusions" | "general";

const navigation: {
  id: Page;
  label: string;
  icon: (props: { className?: string }) => ReactElement;
  group: string;
}[] = [
  { id: "overview", label: "概览", icon: IconHome, group: "使用" },
  { id: "displays", label: "显示器", icon: IconDisplays, group: "使用" },
  { id: "exclusions", label: "排除的应用", icon: IconShield, group: "使用" },
  { id: "general", label: "设置", icon: IconSettings, group: "设置" },
];

const stateTone: Record<
  EngineStateName,
  "neutral" | "accent" | "success" | "warning" | "danger"
> = {
  idle: "neutral",
  planning: "accent",
  migrating: "accent",
  active: "accent",
  restoring: "accent",
  degraded: "warning",
  error: "danger",
};

const stateLabel: Record<EngineStateName, string> = {
  idle: "就绪",
  planning: "正在计划",
  migrating: "正在整理",
  active: "布局已启用",
  restoring: "正在恢复",
  degraded: "部分受限",
  error: "需要处理",
};

export interface Notice {
  tone: "neutral" | "success" | "warning" | "danger";
  text: string;
}

export default function App() {
  const [page, setPage] = useState<Page>("overview");
  const [status, setStatus] = useState<StatusDto | null>(null);
  const [displays, setDisplays] = useState<DisplayDto[]>([]);
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [preview, setPreview] = useState<PreviewDto | null>(null);
  const [pending, setPending] = useState<PendingWorkDto | null>(null);
  const [hotkeys, setHotkeys] = useState<HotkeyStatusDto | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatusDto | null>(null);
  const [thumbnails, setThumbnails] = useState<Record<string, string>>({});
  const [runningApps, setRunningApps] = useState<AppDto[]>([]);
  const [activity, setActivity] = useState<ActivityEntryDto[]>([]);
  const [health, setHealth] = useState<HealthItemDto[]>([]);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);
  const noticeTimer = useRef<number | null>(null);

  const flash = useCallback((next: Notice) => {
    setNotice(next);
    if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 6000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [
        nextStatus,
        nextDisplays,
        nextSettings,
        nextPending,
        nextHotkeys,
        nextUpdate,
        nextActivity,
        nextHealth,
      ] = await Promise.all([
        api.getStatus(),
        api.listDisplays(),
        api.getSettings(),
        api.pendingWork(),
        api.hotkeyStatus(),
        api.updateStatus(),
        api.activityLog(),
        api.healthCheck(),
      ]);
      setActivity(nextActivity);
      setHealth(nextHealth);
      setStatus(nextStatus);
      setDisplays(nextDisplays);
      setSettings(nextSettings);
      setPending(nextPending);
      setHotkeys(nextHotkeys);
      setUpdateStatus(nextUpdate);
    } catch (e) {
      flash({ tone: "danger", text: errorMessage(e) });
    }
  }, [flash]);

  useEffect(() => {
    const theme = settings?.theme ?? "system";
    const root = document.documentElement;
    root.classList.remove("theme-system", "theme-light", "theme-dark");
    root.classList.add(`theme-${theme}`);
  }, [settings?.theme]);

  // Screenshots are only fetched when a surface that shows them is visible: each
  // one is a full-screen capture and a PNG encode, and most sessions never open
  // those pages. The guide counts as such a surface — it is where a new user
  // picks the target display.
  const needsThumbnails =
    page === "displays" ||
    page === "exclusions" ||
    (settings !== null && !settings.onboarding_done);

  useEffect(() => {
    if (!needsThumbnails) return;
    let cancelled = false;
    void (async () => {
      const shots = await api.displayThumbnails();
      if (cancelled) return;
      setThumbnails(
        Object.fromEntries(shots.map((s) => [s.native_id, s.data_url])),
      );
    })();
    return () => {
      cancelled = true;
    };
  }, [needsThumbnails, displays.length]);

  // The list is what the user is looking at while they switch to another app and
  // come back. Fetching only on navigation meant the newly opened app was missing
  // until the page was left and re-entered a second time.
  useEffect(() => {
    if (page !== "exclusions") return;
    let cancelled = false;
    const load = async () => {
      const apps = await api.runningApplications();
      if (!cancelled) setRunningApps(apps);
    };
    void load();
    const timer = window.setInterval(() => void load(), 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [page, settings?.excluded_apps]);

  useEffect(() => {
    void refresh();

    const unlistenAction = listen<{ action: string; ok: boolean; message?: string }>(
      "conflux://action",
      (event) => {
        void refresh();
        if (!event.payload.ok && event.payload.message) {
          flash({ tone: "danger", text: errorMessage(event.payload.message) });
        }
      },
    );
    const unlistenRemote = listen<{
      active: boolean;
      evidence?: string;
      acted?: boolean;
      applied?: number;
      restored?: number;
      error?: string;
    }>("conflux://remote", (event) => {
      void refresh();
      const { active, evidence, acted, applied, restored, error } = event.payload;
      if (error) {
        flash({
          tone: "warning",
          text: `自动处理失败：${error}`,
        });
        return;
      }
      if (active && acted) {
        flash({
          tone: "success",
          text: `检测到${evidence ?? "远程会话"}，已整理 ${applied ?? 0} 个窗口。`,
        });
        return;
      }
      if (active && evidence) {
        flash({
          tone: "neutral",
          text: `检测到${evidence}。自动整理已关闭，可点「整理到目标屏」。`,
        });
        return;
      }
      if (!active && acted) {
        flash({
          tone: "success",
          text: `远程会话已结束，已恢复 ${restored ?? 0} 个窗口。`,
        });
      }
    });
    const unlistenExit = listen("conflux://exit-blocked", () => {
      // The old message mentioned "keep the current layout" as an option while
      // no such control existed anywhere in the app.
      flash({
        tone: "warning",
        text: "还有窗口没有恢复，已取消退出。在概览页可以再试一次恢复，或点「保留当前布局」。",
      });
      setPage("overview");
    });
    const unlistenProgress = listen<{
      phase: string;
      seconds?: number;
      done?: number;
      total?: number;
    }>(
      "conflux://progress",
      (event) => {
        const phase = event.payload.phase;
        setBusy(phase !== "done" && phase !== "waiting");
        // A count once the engine reports one. Without it the button only says
        // "working", which is indistinguishable from "stuck" when a single
        // application holds the batch for a few seconds.
        if (phase === "migrating" && (event.payload.total ?? 0) > 0) {
          setProgress({
            done: event.payload.done ?? 0,
            total: event.payload.total ?? 0,
          });
        }
        if (phase === "done") setProgress(null);
        // A delayed restore is not "busy": the window is still usable and the
        // user can cancel it by gathering. Saying "正在恢复" during the wait
        // would be a lie, and hiding the countdown leaves the button looking dead.
        if (phase === "waiting") {
          const seconds = event.payload.seconds ?? 0;
          setNotice({
            tone: "neutral",
            text: `${seconds} 秒后恢复布局。现在点「整理到目标屏」可以取消。`,
          });
          return;
        }
        if (phase === "done") void refresh();
      },
    );
    const unlistenTopology = listen<{
      rescued: number;
      needs_regather?: boolean;
      captured?: number;
    }>(
      "conflux://topology-changed",
      (event) => {
        void refresh();
        const { rescued, needs_regather } = event.payload;
        if (rescued > 0) {
          flash({
            tone: "warning",
            text: `显示器连接发生变化，已把 ${rescued} 个窗口移回可见区域。`,
          });
          return;
        }
        if (needs_regather) {
          // The layout survived but the display list it was planned against did
          // not. Saying nothing leaves windows arranged for a monitor that is no
          // longer where the plan assumed.
          flash({
            tone: "neutral",
            text: "显示器连接发生变化。当前布局可能已不适用，建议重新点「整理到目标屏」。",
          });
        }
      },
    );
    const unlistenUpdate = listen<{ outcome: string; version?: string }>(
      "conflux://update",
      (event) => {
        void refresh();
        if (event.payload.outcome === "deferred") {
          flash({
            tone: "neutral",
            text: `新版本 ${event.payload.version ?? ""} 已下载，将在窗口全部恢复后安装。`,
          });
        }
        if (event.payload.outcome === "installed") {
          flash({ tone: "neutral", text: "更新已安装，应用即将重启。" });
        }
      },
    );

    const timer = window.setInterval(() => void refresh(), 4000);

    return () => {
      void unlistenAction.then((fn) => fn());
      void unlistenRemote.then((fn) => fn());
      void unlistenExit.then((fn) => fn());
      void unlistenProgress.then((fn) => fn());
      void unlistenTopology.then((fn) => fn());
      void unlistenUpdate.then((fn) => fn());
      window.clearInterval(timer);
    };
  }, [refresh, flash]);

  const run = useCallback(
    async <T,>(
      action: () => Promise<T>,
      onSuccess?: (value: T) => Notice | null,
    ): Promise<T | null> => {
      setBusy(true);
      try {
        const value = await action();
        const message = onSuccess?.(value);
        if (message) flash(message);
        return value;
      } catch (e) {
        // Cancellation is the user's own doing, not a failure: a red banner for
        // "you started a new gather" reads like something broke.
        const cancelled = String(e).includes("restore_cancelled");
        flash({
          tone: cancelled ? "neutral" : "danger",
          text: errorMessage(e),
        });
        return null;
      } finally {
        setBusy(false);
        void refresh();
      }
    },
    [flash, refresh],
  );

  const saveSettings = (patch: Partial<SettingsDto>) =>
    run(async () => {
      const next = await api.saveSettings(patch);
      setSettings(next);
      return next;
    });

  const onRestore = () =>
    run(
      async () => {
        return api.restore();
      },
      (report) => ({
        tone: report.pending > 0 ? "warning" : "success",
        text:
          [
            `已恢复 ${report.restored} 个窗口`,
            report.kept_user_change > 0
              ? `保留你的改动 ${report.kept_user_change} 个`
              : null,
            report.fallback_visible > 0
              ? `原屏不可用，回退到可见区域 ${report.fallback_visible} 个`
              : null,
            report.pending > 0 ? `仍有 ${report.pending} 个待处理` : null,
          ]
            .filter(Boolean)
            .join("，") + "。",
      }),
    );

  if (settings && !settings.onboarding_done) {
    return (
      <Onboarding
        displays={displays}
        settings={settings}
        thumbnails={thumbnails}
        busy={busy}
        onSelectTarget={async (nativeId) => {
          await run(() => api.setTargetDisplay(nativeId));
        }}
        onIdentify={() =>
          void run(() => api.identifyDisplays(4), (shown) => ({
            tone: shown ? "neutral" : "warning",
            text: shown
              ? "已在每块屏幕上显示编号。"
              : "无法显示编号，请检查屏幕状态。",
          }))
        }
        onSetDetect={async (level, auto) => {
          const next = await api.saveSettings({
            auto_detect: level,
            auto_gather: auto,
            auto_restore: auto,
          });
          setSettings(next);
        }}
        onFinish={() =>
          void run(
            async () => {
              await completeOnboarding();
              const next = await api.getSettings();
              setSettings(next);
              return next;
            },
            () => null,
          )
        }
      />
    );
  }

  const state = status?.state ?? "idle";
  const supported = status?.supported ?? false;
  const groups = [...new Set(navigation.map((item) => item.group))];

  return (
    <div className="flex h-full">
      <aside className="flex w-48 shrink-0 flex-col border-r border-[var(--color-border)] bg-[var(--color-sidebar)]">
        <div className="flex h-12 items-center px-4">
          <span className="text-[13px] font-semibold tracking-tight">Conflux</span>
        </div>

        <nav
          className="flex-1 px-2"
          aria-label="主导航"
          // Arrow keys move between pages, the way a real sidebar behaves. Tab
          // alone would walk through every item to reach the content, which is
          // four extra stops on every visit.
          onKeyDown={(event) => {
            if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
            const index = navigation.findIndex((item) => item.id === page);
            const delta = event.key === "ArrowDown" ? 1 : -1;
            const next = navigation[(index + delta + navigation.length) % navigation.length];
            if (!next) return;
            event.preventDefault();
            setPage(next.id);
            // Move focus with the selection so the next key press continues from
            // the new item instead of the old one.
            const target = event.currentTarget.querySelector<HTMLButtonElement>(
              `[data-page="${next.id}"]`,
            );
            target?.focus();
          }}
        >
          {groups.map((group) => (
            <div key={group} className="mb-3">
              <p className="px-2 pb-1 text-[11px] font-medium uppercase tracking-wide text-[var(--color-muted)]">
                {group}
              </p>
              {navigation
                .filter((item) => item.group === group)
                .map((item) => {
                  const active = page === item.id;
                  const Icon = item.icon;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      data-page={item.id}
                      tabIndex={active ? 0 : -1}
                      onClick={() => setPage(item.id)}
                      aria-current={active ? "page" : undefined}
                      className={`mb-0.5 flex w-full items-center gap-2 rounded-[var(--radius-control)] px-2 py-1.5 text-left text-[13px] transition-colors ${
                        active
                          ? "bg-[var(--color-nav-active)] font-medium text-[var(--color-fg)] shadow-[var(--shadow-card)]"
                          : "text-[var(--color-fg-soft)] hover:bg-[var(--color-surface)]/60"
                      }`}
                    >
                      <Icon className="size-4 shrink-0 opacity-70" />
                      {item.label}
                    </button>
                  );
                })}
            </div>
          ))}
        </nav>

        <div className="border-t border-[var(--color-border)] px-3 py-2.5">
          <Status tone={stateTone[state]}>
            {stateLabel[state]}
            {status?.paused ? " · 已暂停" : ""}
          </Status>
          {!supported ? (
            <p className="mt-1 text-[11px] text-[var(--color-muted)]">
              当前平台未实现窗口控制
            </p>
          ) : null}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-5">
          <h1 className="text-[13px] font-semibold">
            {navigation.find((item) => item.id === page)?.label}
          </h1>
          <div className="flex items-center gap-3">
            <span className="text-[11px] tabular-nums text-[var(--color-muted)]">
              v{updateStatus?.current_version ?? "—"}
            </span>
          </div>
        </header>

        <main className="flex-1 overflow-y-auto">
          <div className="mx-auto flex max-w-2xl flex-col gap-3 px-5 py-4">
            {notice ? (
              <div
                role={notice.tone === "danger" ? "alert" : "status"}
                className={`animate-in flex items-start gap-2 rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 bg-[var(--color-surface)] px-3.5 py-2.5 text-xs ${
                  notice.tone === "danger"
                    ? "border-l-[var(--color-danger)]"
                    : notice.tone === "warning"
                      ? "border-l-[var(--color-warning)]"
                      : notice.tone === "success"
                        ? "border-l-[var(--color-success)]"
                        : "border-l-[var(--color-accent)]"
                }`}
              >
                <IconInfo className="mt-px size-3.5 shrink-0 text-[var(--color-muted)]" />
                <span className="text-[var(--color-fg-soft)]">{notice.text}</span>
              </div>
            ) : null}

            {status && !status.has_permission ? (
              <PermissionPrompt
                status={status}
                busy={busy}
                onRequest={() =>
                  void run(() => api.requestPermission(), () => ({
                    tone: "neutral",
                    text: "已请求授权。授权后请点「重新检查」。",
                  }))
                }
                onSettings={() => void api.openPermissionSettings()}
                onRecheck={() => void refresh()}
              />
            ) : null}

            {pending?.present ? (
              <PendingCard
                pending={pending}
                busy={busy}
                onRestore={() => void onRestore()}
                onDismiss={() =>
                  void run(
                    () => api.dismissPendingWork(),
                    () => ({ tone: "neutral", text: "已隐藏提示，恢复记录仍然保留。" }),
                  )
                }
              />
            ) : null}

            {page === "overview" ? (
              <OverviewPage
                status={status}
                settings={settings}
                preview={preview}
                displays={displays}
                activity={activity}
                progress={progress}
                busy={busy}
                onPreview={() =>
                  void run(
                    async () => {
                      const next = await api.preview();
                      setPreview(next);
                      return next;
                    },
                    (next) =>
                      next.placements.length === 0
                        ? { tone: "neutral", text: "没有可整理的窗口。" }
                        : null,
                  )
                }
                onMigrate={() =>
                  void run(
                    async () => {
                      const result = await api.migrate();
                      setPreview(null);
                      return result;
                    },
                    (result) => {
                      // Naming the applications is what makes the next step
                      // possible: a count of refusals cannot be acted on, but
                      // "msedge.exe did not accept" tells the user which rule to
                      // add. The names are executable base names only.
                      const refused =
                        result.refused_apps.length > 0
                          ? `；${result.refused_apps.join("、")} 没有接受位置调整`
                          : "";
                      return {
                        tone: result.rejected > 0 ? "warning" : "success",
                        text:
                          `已整理 ${result.applied} 个窗口` +
                          (result.gone > 0 ? `，${result.gone} 个已关闭` : "") +
                          (result.skipped > 0 ? `，跳过 ${result.skipped} 个` : "") +
                          `${refused}。`,
                      };
                    },
                  )
                }
                onRestore={() => void onRestore()}
                onPause={(paused) =>
                  void run(
                    () => api.setPaused(paused),
                    (result) => ({
                      tone: "neutral",
                      text: result.paused
                        ? "已暂停自动整理，当前布局保留。"
                        : result.restored_deferred
                          ? // The session ended while paused, so resuming also
                            // finished the cleanup. Saying only "resumed" would
                            // leave the user wondering why their windows moved.
                            "已恢复自动整理，并补做了暂停期间欠下的恢复。"
                          : "已恢复自动整理。",
                    }),
                  )
                }
                onRescue={() =>
                  void run(
                    () => api.rescueOffscreen(),
                    (count) => ({
                      tone: count === 0 ? "neutral" : "success",
                      text:
                        count === 0
                          ? "没有发现越界窗口。"
                          : `已把 ${count} 个窗口移回可见区域。`,
                    }),
                  )
                }
                onIdentify={() =>
                  void run(() => api.identifyDisplays(4), (shown) => ({
                    tone: shown ? "neutral" : "warning",
                    text: shown
                      ? "已在每块屏幕上显示编号，4 秒后消失。"
                      : "无法显示编号，请检查屏幕状态。",
                  }))
                }
                onExport={() =>
                  void run(
                    () => api.exportDiagnostics(),
                    (result) => ({
                      tone: "neutral",
                      text: `诊断包已写入 ${result.path}（${formatBytes(result.bytes)}）。`,
                    }),
                  )
                }
                onOpenDisplays={() => setPage("displays")}
                onDismissError={() =>
                  void run(() => api.dismissLastError())
                }
                onKeepLayout={() =>
                  void run(
                    () => api.keepCurrentLayout(),
                    () => ({
                      tone: "neutral",
                      text: "已保留当前布局，之后可以重新整理到目标屏。",
                    }),
                  )
                }
                onExclude={(appNames) => {
                  // Merge in one pass: each name is compared against the current
                  // rules and against the names added earlier in this batch, so a
                  // batch of three cannot produce duplicate rules.
                  const next = [...(settings?.excluded_apps ?? [])];
                  for (const name of appNames) {
                    if (
                      !next.some(
                        (rule) => rule.toLowerCase() === name.toLowerCase(),
                      )
                    ) {
                      next.push(name);
                    }
                  }
                  void saveSettings({ excluded_apps: next });
                }}
              />
            ) : null}

            {page === "displays" ? (
              <DisplaysPage
                displays={displays}
                settings={settings}
                target={status?.target ?? null}
                thumbnails={thumbnails}
                busy={busy}
                onSelect={(nativeId) =>
                  void run(
                    () => api.setTargetDisplay(nativeId),
                    () => null,
                  )
                }
                onIdentify={() =>
                  void run(() => api.identifyDisplays(4), (shown) => ({
                    tone: shown ? "neutral" : "warning",
                    text: shown
                      ? "已在每块屏幕上显示编号。"
                      : "无法显示编号，请检查屏幕状态。",
                  }))
                }
                onSave={(patch) => void saveSettings(patch)}
              />
            ) : null}

            {page === "exclusions" ? (
              <ExclusionsPage
                settings={settings}
                runningApps={runningApps}
                busy={busy}
                onSave={(patch) => void saveSettings(patch)}
              />
            ) : null}

            {page === "general" ? (
              <GeneralPage
                settings={settings}
                hotkeys={hotkeys}
                health={health}
                updateStatus={updateStatus}
                busy={busy}
                onSave={(patch) => void saveSettings(patch)}
                onCheckUpdate={() =>
                  void run(
                    () => api.checkUpdate(),
                    (result) => ({
                      tone:
                        result.outcome === "up_to_date"
                          ? "success"
                          : result.outcome === "failed"
                            ? "danger"
                            : "neutral",
                      text:
                        result.outcome === "up_to_date"
                          ? "当前已是最新版本。"
                          : result.outcome === "deferred"
                            ? "更新已下载，将在窗口全部恢复后安装。"
                            : result.outcome === "installed"
                              ? "更新已安装，应用即将重启。"
                              : `检查更新失败：${result.detail || "未知原因"}`,
                    }),
                  )
                }
              />
            ) : null}
          </div>
        </main>
      </div>
    </div>
  );
}

function PermissionPrompt({
  status,
  busy,
  onRequest,
  onSettings,
  onRecheck,
}: {
  status: StatusDto;
  busy: boolean;
  onRequest: () => void;
  onSettings: () => void;
  onRecheck: () => void;
}) {
  return (
    <div className="rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] bg-[var(--color-surface)] px-3.5 py-2.5">
      <p className="text-[13px] font-medium">
        需要「{status.permission_name}」权限才能移动窗口
      </p>
      <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-soft)]">
        预览、查看显示器与导出诊断仍可使用，只是不会移动窗口。
      </p>
      <div className="mt-2.5 flex gap-1.5">
        <button
          type="button"
          onClick={onRequest}
          disabled={busy}
          className="rounded-[var(--radius-control)] bg-[var(--color-accent)] px-2.5 py-1 text-xs font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          请求授权
        </button>
        <button
          type="button"
          onClick={onSettings}
          className="rounded-[var(--radius-control)] border border-[var(--color-border-strong)] px-2.5 py-1 text-xs font-medium"
        >
          前往系统设置
        </button>
        <button
          type="button"
          onClick={onRecheck}
          disabled={busy}
          className="rounded-[var(--radius-control)] px-2.5 py-1 text-xs font-medium text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-sunken)]"
        >
          重新检查
        </button>
      </div>
    </div>
  );
}

function PendingCard({
  pending,
  busy,
  onRestore,
  onDismiss,
}: {
  pending: PendingWorkDto;
  busy: boolean;
  onRestore: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] bg-[var(--color-surface)] px-3.5 py-2.5">
      <p className="text-[13px] font-medium">上次整理未完成</p>
      <p className="mt-0.5 text-xs leading-relaxed text-[var(--color-fg-soft)]">
        有 {pending.windows} 个窗口的快照仍待处理
        {pending.may_have_moved ? "，其中部分可能已被移动" : ""}。
      </p>
      <div className="mt-2.5 flex gap-1.5">
        <button
          type="button"
          onClick={onRestore}
          disabled={busy}
          className="rounded-[var(--radius-control)] bg-[var(--color-accent)] px-2.5 py-1 text-xs font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          尝试恢复
        </button>
        <button
          type="button"
          onClick={onDismiss}
          disabled={busy}
          className="rounded-[var(--radius-control)] px-2.5 py-1 text-xs font-medium text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-sunken)]"
        >
          稍后处理
        </button>
      </div>
    </div>
  );
}
