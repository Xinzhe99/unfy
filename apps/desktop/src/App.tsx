import { type ReactElement, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  api,
  errorMessage,
  type ActivityEntryDto,
  type HealthItemDto,
  formatBytes,
  type AppDto,
  type DisplayDto,
  type DisplayThumbnailDto,
  type EngineStateName,
  type HotkeyStatusDto,
  type PendingWorkDto,
  type PreviewDto,
  type SettingsDto,
  type StatusDto,
  type UpdateStatusDto,
} from "./api";
import { stateLabels } from "./api";
import { applyLocale, rememberLocale, resolveLocale, t } from "./i18n";
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
import { Status, type StatusTone } from "./components/ui";
import { PendingCard, PermissionPrompt } from "./components/prompts";

export type Page = "overview" | "displays" | "exclusions" | "general";

/**
 * Exported for the translation test: these labels are translated at render time
 * (see the sidebar), so nothing else can prove they have English entries.
 */
export const navigation: {
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

/**
 * State to dot colour.
 *
 * `idle` is `success`, not `neutral`: an idle engine with nothing pending is the
 * healthy state, and a grey dot next to "就绪" reads as "disabled" or "unknown".
 * `active` and the working states are `live` (blue) rather than the monochrome
 * accent, so "a layout is in effect" is visible at a glance instead of looking
 * like ordinary text.
 */
const stateTone: Record<EngineStateName, StatusTone> = {
  idle: "success",
  planning: "live",
  migrating: "live",
  active: "live",
  restoring: "live",
  degraded: "warning",
  error: "danger",
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
  // Applications whose windows the last restore could not put back. The backend
  // already computes this; without it the overview could only say "a window did not
  // come back", leaving the user to guess which application to close.
  const [pendingApps, setPendingApps] = useState<string[]>([]);
  const [hotkeys, setHotkeys] = useState<HotkeyStatusDto | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatusDto | null>(null);
  const [thumbnails, setThumbnails] = useState<Record<string, DisplayThumbnailDto>>({});
  // A ref, not state: the topology listener is registered once and must be able to
  // ask for a fresh capture without re-subscribing every time the setter changes.
  const captureShots = useRef<() => void>(() => {});
  const [runningApps, setRunningApps] = useState<AppDto[]>([]);
  const [activity, setActivity] = useState<ActivityEntryDto[]>([]);
  const [health, setHealth] = useState<HealthItemDto[]>([]);
  const [diagnosticsPreview, setDiagnosticsPreview] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);
  const noticeTimer = useRef<number | null>(null);
  const countdownTimer = useRef<number | null>(null);

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

  // Applied during render: `t()` reads a module-level value, and an effect ran too
  // late - the first paint used the old locale and nothing re-rendered, so
  // switching to English left the interface in Chinese. Safe to mutate here
  // because the value derives from `settings.language` alone.
  const locale = settings ? resolveLocale(settings.language) : null;
  if (locale) applyLocale(locale);
  useEffect(() => {
    if (!locale) return;
    rememberLocale(locale);
    // The tray is built in Rust and cannot read the catalogue, so it is told which
    // language this render used. Failure is not worth a notice: the tray keeps its
    // previous language and the window is already correct.
    void api.setUiLocale(locale).catch(() => undefined);
  }, [locale]);

  useEffect(() => {
    const theme = settings?.theme ?? "system";
    const root = document.documentElement;
    root.classList.remove("theme-system", "theme-light", "theme-dark");
    root.classList.add(`theme-${theme}`);
    // The window frame is drawn by the operating system, not by the stylesheet, so
    // it stays light over a dark page unless it is told which theme to use. `null`
    // hands the decision back to the system, which is what "follow the system"
    // means at this level.
    void getCurrentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => undefined);
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
    const capture = async () => {
      const shots = await api.displayThumbnails();
      if (cancelled) return;
      setThumbnails(Object.fromEntries(shots.map((s) => [s.native_id, s])));
    };
    captureShots.current = () => void capture();
    // Captured on entry, then whenever the desktop says it changed.
    //
    // This used to poll every five seconds. Each poll is a full capture of every
    // display plus a base64 payload over the IPC bridge, which is why the picture
    // lagged behind the screen: it was up to five seconds old, and the round trip
    // added to that. A capture is only stale when the screen changed, and the
    // backend already watches the topology; it says so.
    void capture();
    // Two seconds is a compromise: fast enough that a window opened on the host
    // shows up while the user is still looking at the page, cheap enough that the
    // capture does not compete with the rest of the polling. The topology event
    // below refreshes it immediately when the displays themselves change.
    const timer = window.setInterval(() => void capture(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [needsThumbnails, page, displays.length]);

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
      if (countdownTimer.current !== null) {
        window.clearInterval(countdownTimer.current);
      }
    };
  }, [page, settings?.excluded_apps]);

  // A preview describes the windows as they were when it was computed. Leaving
  // the page means the user did something else in between - switching apps,
  // closing windows, plugging in a monitor - so the picture is stale by the time
  // they come back. It was previously kept forever and silently disagreed with the
  // desktop.
  useEffect(() => {
    if (page !== "overview") setPreview(null);
  }, [page]);

  // While the plan is on screen it is recomputed on the same cadence as the rest of
  // the overview. It is a description of which windows exist right now: leaving it
  // frozen meant a window the user had just closed was still drawn in the picture
  // they were using to decide whether to gather.
  const previewShown = preview !== null;
  useEffect(() => {
    if (!previewShown) return;
    let cancelled = false;
    const timer = window.setInterval(() => {
      void api.preview().then((next) => {
        if (!cancelled) setPreview(next);
      });
    }, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [previewShown]);

  useEffect(() => {
    // The first load is done by `startPolling` below, which runs only when the
    // window is visible; calling `refresh` here as well would double the eight
    // commands on every mount.

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
      suppressed?: string;
    }>("conflux://remote", (event) => {
      void refresh();
      const { active, evidence, acted, applied, restored, error, suppressed } =
        event.payload;
      if (suppressed === "cooldown") {
        // Silence would look like a bug: the session was detected but nothing
        // happened. Say why, and say that the manual button still works.
        flash({
          tone: "neutral",
          text: t("检测到{evidence}，但 30 秒内已经整理过一次，这次跳过。需要时可直接点「整理到目标屏」。", { evidence: evidence ?? t("远程会话") }),
        });
        return;
      }
      if (error) {
        flash({
          tone: "warning",
          text: t("自动处理失败：{message}", { message: error }),
        });
        return;
      }
      if (active && acted) {
        flash({
          tone: "success",
          text: t("检测到{evidence}，已整理 {count} 个窗口。", { evidence: evidence ?? t("远程会话"), count: applied ?? 0 }),
        });
        return;
      }
      if (active && evidence) {
        flash({
          tone: "neutral",
          text: t("检测到{evidence}。自动整理已关闭，可点「整理到目标屏」。", { evidence: evidence ?? "" }),
        });
        return;
      }
      if (!active && acted) {
        flash({
          tone: "success",
          text: t("远程会话已结束，已恢复 {count} 个窗口。", { count: restored ?? 0 }),
        });
      }
    });
    const unlistenExit = listen("conflux://exit-blocked", () => {
      // The old message mentioned "keep the current layout" as an option while
      // no such control existed anywhere in the app.
      flash({
        tone: "warning",
        text: t(
          "还有窗口没有恢复，已取消退出。在概览页可以再试一次恢复，或点「保留当前布局」。",
        ),
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
          // Tick the countdown locally: the backend announces the wait once, and a
          // frozen t("5 秒后") for five seconds looks like the app has stalled.
          const seconds = event.payload.seconds ?? 0;
          if (countdownTimer.current !== null) {
            window.clearInterval(countdownTimer.current);
          }
          // The countdown owns the notice line for its whole life, so a timer
          // left over from the preceding message must be cancelled here: `flash`
          // clears every notice after six seconds, and a twenty-second wait would
          // lose its countdown at the six-second mark and show nothing at all for
          // the remaining fourteen. That is exactly the stretch where the user is
          // wondering whether the app noticed the disconnect.
          if (noticeTimer.current !== null) {
            window.clearTimeout(noticeTimer.current);
            noticeTimer.current = null;
          }
          let remaining = seconds;
          const show = () => {
            setNotice({
              tone: "neutral",
              text:
                remaining > 0
                  ? t("{seconds} 秒后恢复布局。点「保留当前布局」可以取消。", { seconds: remaining })
                  : t("正在恢复布局…"),
            });
          };
          show();
          countdownTimer.current = window.setInterval(() => {
            remaining -= 1;
            if (remaining < 0) {
              if (countdownTimer.current !== null) {
                window.clearInterval(countdownTimer.current);
                countdownTimer.current = null;
              }
              return;
            }
            show();
          }, 1000);
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
        // The displays just changed, so every screenshot is wrong until recaptured.
        captureShots.current();
        const { rescued, needs_regather } = event.payload;
        if (rescued > 0) {
          flash({
            tone: "warning",
            text: t("显示器连接发生变化，已把 {count} 个窗口移回可见区域。", { count: rescued }),
          });
          return;
        }
        if (needs_regather) {
          // The layout survived but the display list it was planned against did
          // not. Saying nothing leaves windows arranged for a monitor that is no
          // longer where the plan assumed.
          flash({
            tone: "neutral",
            text: t("显示器连接发生变化。当前布局可能已不适用，建议重新点「整理到目标屏」。"),
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
            text: t("新版本 {version} 已下载，将在窗口全部恢复后安装。", { version: event.payload.version ?? "" }),
          });
        }
        if (event.payload.outcome === "installed") {
          flash({ tone: "neutral", text: t("更新已安装，应用即将重启。") });
        }
      },
    );

    // Poll only while the window is visible. Closing the window hides it and the
    // process keeps running in the tray, so an unconditional interval meant eight
    // commands - including a journal-directory scan and a disk write probe - every
    // four seconds for the entire life of the machine.
    //
    // Hidden is not stale: the tray shows the same facts from the same engine, and
    // showing the window refreshes immediately.
    let timer: number | null = null;
    const startPolling = () => {
      if (timer !== null) return;
      void refresh();
      timer = window.setInterval(() => void refresh(), 4000);
    };
    const stopPolling = () => {
      if (timer === null) return;
      window.clearInterval(timer);
      timer = null;
    };
    // Tauri hiding the window does not change `document.visibilityState`, so the
    // backend sends an explicit event. Polling resumes on the first focus or when
    // the window is shown again.
    const onVisibility = () => {
      if (document.visibilityState === "visible") startPolling();
      else stopPolling();
    };
    document.addEventListener("visibilitychange", onVisibility);
    const onWindow = () => {
      // Any focus after being hidden means the window is back on screen.
      startPolling();
    };
    window.addEventListener("focus", onWindow);
    const unlistenHidden = listen("conflux://window-hidden", () => {
      stopPolling();
    });
    if (document.visibilityState === "visible") startPolling();

    return () => {
      void unlistenAction.then((fn) => fn());
      void unlistenRemote.then((fn) => fn());
      void unlistenExit.then((fn) => fn());
      void unlistenProgress.then((fn) => fn());
      void unlistenTopology.then((fn) => fn());
      void unlistenUpdate.then((fn) => fn());
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", onWindow);
      void unlistenHidden.then((fn) => fn());
      stopPolling();
      if (countdownTimer.current !== null) {
        window.clearInterval(countdownTimer.current);
      }
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
      (report) => {
        setPendingApps(report.pending_apps);
        return {
        tone: report.pending > 0 ? "warning" : "success",
        text:
          [
            t("已恢复 {count} 个窗口", { count: report.restored }),
            report.kept_user_change > 0
              ? t("保留你的改动 {count} 个", { count: report.kept_user_change })
              : null,
            report.fallback_visible > 0
              ? t("原屏不可用，回退到可见区域 {count} 个", { count: report.fallback_visible })
              : null,
            report.pending > 0 ? t("仍有 {count} 个待处理", { count: report.pending }) : null,
          ]
            .filter(Boolean)
            .join(t("，")) + t("。"),
        };
      },
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
          void run(() => api.identifyDisplays(4), (outcome) => ({
            tone: outcome === "unavailable" ? "warning" : "neutral",
            text:
              outcome === "shown"
                ? t("已在每块屏幕上显示编号。")
                : outcome === "already_showing"
                  ? // Asking twice inside the four seconds the badges live used to
                    // report a failure and send people to check their monitors.
                    t("编号正在显示，请对照屏幕确认。")
                  : t("无法显示编号，请检查屏幕状态。"),
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
          aria-label={t("主导航")}
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
                {t(group)}
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
                      {t(item.label)}
                    </button>
                  );
                })}
            </div>
          ))}
        </nav>

        <div className="border-t border-[var(--color-border)] px-3 py-2.5">
          <Status tone={status ? stateTone[state] : "neutral"}>
            {/* Before the first reply there is no state to name. Showing "ready"
                while the app has not yet read anything is a claim it cannot make. */}
            {status ? t(stateLabels[state]) : t("正在载入…")}
            {status?.paused ? t(" · 已暂停") : ""}
          </Status>
          {!supported ? (
            <p className="mt-1 text-[11px] text-[var(--color-muted)]">{t("当前平台未实现窗口控制")}</p>
          ) : null}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-5">
          <h1 className="text-[13px] font-semibold">
            {t(navigation.find((item) => item.id === page)?.label ?? "")}
          </h1>
          <div className="flex items-center gap-3">
            <span className="text-[11px] tabular-nums text-[var(--color-muted)]">
              v{updateStatus?.current_version ?? "—"}
            </span>
          </div>
        </header>

        {/* `min-h-0` is required, not cosmetic: a flex item's default `min-height:
            auto` refuses to shrink below its content, so with ten settings cards the
            scroll container grew to fit the page and `overflow-y-auto` never engaged.
            The page then had no way to reach anything below the fold - the privacy and
            help cards were simply unreachable at the default window size. */}
        <main className="min-h-0 flex-1 overflow-y-auto">
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
                    text: t("已请求授权。授权后请点「重新检查」。"),
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
                    () => ({ tone: "neutral", text: t("已隐藏提示，恢复记录仍然保留。") }),
                  )
                }
              />
            ) : null}

            {settings === null ? (
              <div className="flex h-40 items-center justify-center text-xs text-[var(--color-muted)]">
                {t("正在载入…")}
              </div>
            ) : page === "overview" ? (
              <OverviewPage
                status={status}
                settings={settings}
                preview={preview}
                displays={displays}
                pendingApps={pendingApps}
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
                        ? { tone: "neutral", text: t("没有可整理的窗口。") }
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
                          ? t("；{apps} 没有接受位置调整", { apps: result.refused_apps.join(", ") })
                          : "";
                      return {
                        // `unplaced` belongs in the same condition as `rejected`: those
                        // windows were left where they were because they did not fit,
                        // and reporting a plain success told the user the gather had
                        // covered everything when it had not.
                        tone:
                          result.rejected > 0 || result.unplaced > 0
                            ? "warning"
                            : "success",
                        text:
                          // Each clause carries its own separator: concatenating the
                          // fragments read "Gathered 5 windowsskipped 7." in English
                          // and "已整理 5 个窗口跳过 7 个。" in Chinese.
                          t("已整理 {count} 个窗口", { count: result.applied }) +
                          (result.gone > 0
                            ? t("，{count} 个已关闭", { count: result.gone })
                            : "") +
                          (result.skipped > 0
                            ? t("，跳过 {count} 个", { count: result.skipped })
                            : "") +
                          (result.unplaced > 0
                            ? t("，{count} 个放不下留在原位", { count: result.unplaced })
                            : "") +
                          refused +
                          t("。"),
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
                        ? t("已暂停自动整理，当前布局保留。")
                        : result.restored_deferred
                          ? // The session ended while paused, so resuming also
                            // finished the cleanup. Saying only "resumed" would
                            // leave the user wondering why their windows moved.
                            t("已恢复自动整理，并补做了暂停期间欠下的恢复。")
                          : t("已恢复自动整理。"),
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
                          ? t("没有发现越界窗口。")
                          : t("已把 {count} 个窗口移回可见区域。", { count }),
                    }),
                  )
                }
                onIdentify={() =>
                  void run(() => api.identifyDisplays(4), (shown) => ({
                    tone: shown ? "neutral" : "warning",
                    text: shown
                      ? t("已在每块屏幕上显示编号，4 秒后消失。")
                      : t("无法显示编号，请检查屏幕状态。"),
                  }))
                }
                diagnosticsPreview={diagnosticsPreview}
                onPreviewDiagnostics={() =>
                  void (async () => {
                    try {
                      const text = await api.previewDiagnostics();
                      setDiagnosticsPreview(text);
                    } catch (e) {
                      flash({ tone: "danger", text: errorMessage(e) });
                    }
                  })()
                }
                onExport={() =>
                  void run(
                    () => api.exportDiagnostics(),
                    (result) => ({
                      tone: "neutral",
                      text: t("诊断包已写入 {path}（{size}）。", {
                path: result.path,
                size: formatBytes(result.bytes),
              }),
                    }),
                  )
                }
                onOpenConfigDir={() =>
                  void run(
                    () => api.openPath(),  // the config directory
                    (opened) => ({
                      tone: opened ? "neutral" : "warning",
                      text: opened
                        ? t("已在文件管理器中打开配置目录。")
                        : t("无法打开配置目录。"),
                    }),
                  )
                }
                onOpenDisplays={() => setPage("displays")}
                onToggleRemote={() =>
                  void run(
                    () => api.toggleRemoteSession(),
                    (nowActive) => ({
                      tone: "neutral",
                      text: nowActive
                        ? t("已标记为远程会话。开启了自动整理的话，窗口会按设置收拢。")
                        : t("已结束远程会话标记。"),
                    }),
                  )
                }
                onDismissError={() =>
                  void run(() => api.dismissLastError())
                }
                onKeepLayout={() =>
                  void run(
                    () => api.keepCurrentLayout(),
                    () => ({
                      tone: "neutral",
                      text: t("已保留当前布局，之后可以重新整理到目标屏。"),
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
                  void run(() => api.identifyDisplays(4), (outcome) => ({
                    tone: outcome === "unavailable" ? "warning" : "neutral",
                    text:
                      outcome === "shown"
                        ? t("已在每块屏幕上显示编号。")
                        : outcome === "already_showing"
                          ? t("编号正在显示，请对照屏幕确认。")
                          : t("无法显示编号，请检查屏幕状态。"),
                  }))
                }
                onCycle={() =>
                  void run(
                    async () => {
                      const next = await api.cycleTarget();
                      // The baseline is preserved by `retarget`, so this is safe to
                      // repeat; say so, because "switch target" sounds destructive.
                      return next;
                    },
                    (next) => ({
                      tone: "neutral",
                      text: t("已切换到显示器 {number}，原有位置记录已保留。", { number: next.number ?? "?" }),
                    }),
                  )
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
                onNotice={(text) => flash({ tone: "neutral", text })}
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
                onClearHistory={() =>
                  void run(
                    () => api.clearActivity(),
                    () => ({ tone: "neutral", text: t("活动历史已清除。") }),
                  )
                }
                onShowGuide={() =>
                  void run(
                    async () => {
                      // Only the guide's own flags: a user re-reading the guide must
                      // not lose their target display or detection settings.
                      const next = await api.saveSettings({
                        onboarding_done: false,
                        onboarding_step: 0,
                      });
                      setSettings(next);
                      return next;
                    },
                    () => null,
                  )
                }
                onReset={() =>
                  void run(
                    async () => {
                      const next = await api.resetSettings();
                      setSettings(next);
                      return next;
                    },
                    () => ({
                      tone: "neutral",
                      text: t("已恢复默认设置，目标显示器已保留。"),
                    }),
                  )
                }
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
                          ? t("当前已是最新版本。")
                          : result.outcome === "deferred"
                            ? t("更新已下载，将在窗口全部恢复后安装。")
                            : result.outcome === "installed"
                              ? t("更新已安装，应用即将重启。")
                              : t("检查更新失败：{detail}", { detail: result.detail || t("未知原因") }),
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
