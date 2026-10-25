import { type ReactElement, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  api,
  errorMessage,
  renderMessage,
  type MessageDto,
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
  type SaveSettingsRequest,
  type StatusDto,
  type UpdateStatusDto,
  type WindowResultDto,
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
  /** Whether elevated windows are reachable from this process. */
  const [elevation, setElevation] = useState<{ elevated: boolean; supported: boolean } | null>(null);
  // Applications whose windows the last restore could not put back. The backend
  // already computes this; without it the overview could only say "a window did not
  // come back", leaving the user to guess which application to close.
  const [pendingApps, setPendingApps] = useState<string[]>([]);
  // Applications that refused the last gather. The settings document no longer
  // remembers this (a persisted copy of a one-off report went stale the moment a
  // rule changed), so the gather result is the only source: it is what the user just
  // saw happen.
  const [refusedApps, setRefusedApps] = useState<string[]>([]);
  const [latestResults, setLatestResults] = useState<WindowResultDto[] | null>(null);
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
  const [windowVisible, setWindowVisible] = useState(false);
  const [restoreCountdown, setRestoreCountdown] = useState<number | null>(null);
  const refreshFlight = useRef<Promise<void> | null>(null);
  const auxiliaryFlight = useRef<Promise<unknown> | null>(null);
  const refreshAgain = useRef(false);
  const mutationEpoch = useRef(0);
  const actionFlight = useRef(false);
  const previewEpoch = useRef(0);

  const clearCountdown = useCallback(() => {
    if (countdownTimer.current !== null) window.clearInterval(countdownTimer.current);
    countdownTimer.current = null;
    setRestoreCountdown(null);
  }, []);

  const invalidatePreview = useCallback(() => {
    previewEpoch.current += 1;
    setPreview(null);
  }, []);

  const flash = useCallback((next: Notice) => {
    setNotice(next);
    if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 6000);
  }, []);

  const refresh = useCallback((): Promise<void> => {
    if (!auxiliaryFlight.current) {
      const epoch = mutationEpoch.current;
      auxiliaryFlight.current = Promise.allSettled([
        api.hotkeyStatus().then((value) => { if (epoch === mutationEpoch.current) setHotkeys(value); }),
        api.updateStatus().then((value) => { if (epoch === mutationEpoch.current) setUpdateStatus(value); }),
        api.activityLog().then((value) => { if (epoch === mutationEpoch.current) setActivity(value); }),
        api.healthCheck().then((value) => { if (epoch === mutationEpoch.current) setHealth(value); }),
      ]).finally(() => { auxiliaryFlight.current = null; });
    }
    if (refreshFlight.current) {
      refreshAgain.current = true;
      return refreshFlight.current;
    }
    const flight = (async () => {
      do {
        refreshAgain.current = false;
        const epoch = mutationEpoch.current;
        const results = await Promise.allSettled([
          api.getStatus(), api.listDisplays(), api.getSettings(), api.pendingWork(),
          api.elevationStatus(),
        ]);
        if (epoch !== mutationEpoch.current) {
          refreshAgain.current = true;
          continue;
        }
        const [nextStatus, nextDisplays, nextSettings, nextPending, nextElevation] = results;
        if (nextStatus.status === "fulfilled") setStatus(nextStatus.value);
        else flash({ tone: "danger", text: errorMessage(nextStatus.reason) });
        if (nextDisplays.status === "fulfilled") setDisplays(nextDisplays.value);
        if (nextSettings.status === "fulfilled") setSettings(nextSettings.value);
        else flash({ tone: "danger", text: errorMessage(nextSettings.reason) });
        if (nextPending.status === "fulfilled") setPending(nextPending.value);
        if (nextElevation.status === "fulfilled") setElevation(nextElevation.value);
      } while (refreshAgain.current);
    })().finally(() => { refreshFlight.current = null; });
    refreshFlight.current = flight;
    return flight;
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
    // Wait for the settings rather than guessing. `?? "system"` used to apply a class
    // the moment React mounted, which removed the one `index.html` had already set from
    // the cache and put light back - so a dark-theme user got the flash twice: once
    // before the page painted, once when this effect ran.
    if (!settings) return;
    const theme = settings.theme;
    const root = document.documentElement;
    root.classList.remove("theme-system", "theme-light", "theme-dark");
    root.classList.add(`theme-${theme}`);
    // Cached so `index.html` can apply it before the first paint; see the comment there.
    try {
      window.localStorage.setItem("unfy-theme", theme);
    } catch {
      // Storage disabled: the inline script keeps the default and this is only a cache.
    }
    // The window frame is drawn by the operating system, not by the stylesheet, so
    // it stays light over a dark page unless it is told which theme to use. `null`
    // hands the decision back to the system, which is what "follow the system"
    // means at this level.
    void getCurrentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => undefined);
  }, [settings?.theme]);

  const needsThumbnails = page === "displays" || (settings !== null && !settings.onboarding_done);
  const topologyKey = displays.map((display) => `${display.native_id}:${display.origin_x}:${display.origin_y}:${display.width}:${display.height}`).join("|");

  useEffect(() => {
    if (!needsThumbnails || !windowVisible) return;
    let cancelled = false;
    let inFlight = false;
    const capture = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const shots = await api.displayThumbnails();
        if (!cancelled) setThumbnails(Object.fromEntries(shots.map((shot) => [shot.native_id, shot])));
      } catch {
        if (!cancelled) setThumbnails({});
      } finally { inFlight = false; }
    };
    captureShots.current = () => void capture();
    void capture();
    return () => { cancelled = true; captureShots.current = () => {}; };
  }, [needsThumbnails, windowVisible, topologyKey]);

  useEffect(() => {
    if (page !== "exclusions" || !windowVisible) return;
    let cancelled = false;
    let inFlight = false;
    const load = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const apps = await api.runningApplications();
        if (!cancelled) setRunningApps(apps);
      } catch { /* Keep the last list when enumeration is temporarily unavailable. */ }
      finally { inFlight = false; }
    };
    void load();
    const timer = window.setInterval(() => void load(), 5000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [page, windowVisible, settings?.excluded_apps.join("|")]);

  useEffect(() => {
    invalidatePreview();
  }, [page, topologyKey, settings?.layout_mode, settings?.margin, settings?.gap, settings?.target?.native_id, status?.target?.confirmed, settings?.excluded_apps.join("|"), invalidatePreview]);

  useEffect(() => {
    // The first load is done by `startPolling` below, which runs only when the
    // window is visible; calling `refresh` here as well would double the eight
    // commands on every mount.

    const unlistenAction = listen<{ action: string; ok: boolean; message?: string }>(
      "unfy://action",
      (event) => {
        mutationEpoch.current += 1;
        invalidatePreview();
        void refresh();
        if (!event.payload.ok && event.payload.message) {
          flash({ tone: "danger", text: errorMessage(event.payload.message) });
        }
      },
    );
    const unlistenRemote = listen<{
      active: boolean;
      candidate?: boolean;
      unknown?: boolean;
      session_id?: number;
      evidence?: MessageDto;
      acted?: boolean;
      rescued?: number;
      error?: string;
    }>("unfy://remote", (event) => {
      mutationEpoch.current += 1;
      void refresh();
      const { active, evidence, acted, rescued, error } = event.payload;
      if (active || acted || error) clearCountdown();
      if (error) {
        flash({ tone: "warning", text: t("自动处理失败：{message}", { message: errorMessage(error) }) });
      } else if (acted) {
        invalidatePreview();
        flash({ tone: "success", text: t("已把 {count} 个窗口移回可见区域。", { count: rescued ?? 0 }) });
      } else if (evidence) {
        flash({ tone: "neutral", text: renderMessage(evidence) });
      }
    });
    const unlistenProgress = listen<{
      phase: string;
      seconds?: number;
      done?: number;
      total?: number;
    }>("unfy://progress", (event) => {
      const { phase, seconds, done, total } = event.payload;
      mutationEpoch.current += 1;
      clearCountdown();
      if (phase === "waiting") {
        const deadline = Date.now() + (seconds ?? 0) * 1000;
        setRestoreCountdown(seconds ?? 0);
        countdownTimer.current = window.setInterval(() => {
          setRestoreCountdown(Math.max(0, Math.ceil((deadline - Date.now()) / 1000)));
        }, 1000);
      } else if (phase === "migrating" || phase === "restoring") {
        invalidatePreview();
        setProgress(total ? { done: done ?? 0, total } : null);
      } else {
        setProgress(null);
      }
      void refresh();
    });
    const unlistenTopology = listen<{
      rescued: number;
      needs_regather?: boolean;
      captured?: number;
    }>(
      "unfy://topology-changed",
      (event) => {
        mutationEpoch.current += 1;
        invalidatePreview();
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
      "unfy://update",
      (event) => {
        void refresh();
        if (event.payload.outcome === "deferred") {
          flash({
            tone: "neutral",
            // The download already happened; the install is what waits. Say that,
            // and keep the restore entry the user needs to unblock it.
            text: t("新版本 {version} 已下载，但当前有未完成的整理，尚未安装。恢复完成后可以再试。", { version: event.payload.version ?? "" }),
          });
        }
        if (event.payload.outcome === "downloaded") {
          flash({ tone: "neutral", text: t("新版本 {version} 已下载并验签，准备安装。", { version: event.payload.version ?? "" }) });
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
    let disposed = false;
    let visibilityVersion = 0;
    const startPolling = () => {
      if (disposed) return;
      visibilityVersion += 1;
      setWindowVisible(true);
      if (timer !== null) return;
      void refresh();
      timer = window.setInterval(() => void refresh(), 4000);
    };
    const stopPolling = () => {
      visibilityVersion += 1;
      setWindowVisible(false);
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
    const unlistenHidden = listen("unfy://window-hidden", () => {
      stopPolling();
    });
    // The explicit pair to the above. `focus` already restarts polling - see `onWindow`
    // - so this is not the only path back, but it is the deterministic one: `set_focus`
    // can be refused when another window holds the foreground, and relying on a focus
    // event that may not arrive is how "the window reopened and never refreshed" would
    // happen. The backend sends it from the one place that shows the window.
    const unlistenShown = listen("unfy://window-shown", () => {
      startPolling();
    });
    // Ask the window, not the page. `document.visibilityState` reports "visible" even
    // when Tauri has hidden the window - measured - so an autostart launch (which hides
    // immediately) began polling eight commands every four seconds for a window nobody
    // had opened. The backend event cannot help here either: it fires during setup,
    // before this listener exists.
    const initialVisibilityVersion = visibilityVersion;
    void api.windowVisible().catch(() => false).then((visible) => {
      if (!disposed && visibilityVersion === initialVisibilityVersion && visible) startPolling();
    });

    return () => {
      disposed = true;
      void unlistenAction.then((fn) => fn());
      void unlistenRemote.then((fn) => fn());
      void unlistenProgress.then((fn) => fn());
      void unlistenShown.then((fn) => fn());
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
  }, [refresh, flash, clearCountdown, invalidatePreview]);

  const run = useCallback(
    async <T,>(
      action: () => Promise<T>,
      onSuccess?: (value: T) => Notice | null,
    ): Promise<T | null> => {
      if (actionFlight.current) return null;
      actionFlight.current = true;
      mutationEpoch.current += 1;
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
        actionFlight.current = false;
        mutationEpoch.current += 1;
        setBusy(false);
        void refresh();
      }
    },
    [flash, refresh],
  );

  const saveSettings = (patch: SaveSettingsRequest) =>
    run(async () => {
      const next = await api.saveSettings(patch);
      setSettings(next);
      return next;
    });

  const onRestore = (transaction?: string) =>
    run(
      async () => {
        clearCountdown();
        invalidatePreview();
        return transaction ? api.restoreRecoveryTransaction(transaction) : api.restore();
      },
      (report) => {
        setPendingApps(report.pending_apps);
        setLatestResults(report.results);
        return {
        // `awaiting_original` counts as unresolved: those windows are visible but
        // their original screen is gone, so they still owe their original position.
        // Reporting a clean success would tell the user everything came back when
        // part of it is still owed.
        tone:
          report.pending > 0 ||
          report.rejected > 0 ||
          report.awaiting_original > 0
            ? "warning"
            : "success",
        text:
          [
            t("已恢复 {count} 个窗口", { count: report.restored }),
            report.kept_user_change > 0
              ? t("保留你的改动 {count} 个", { count: report.kept_user_change })
              : null,
            report.fallback_visible > 0
              ? t("原屏不可用，回退到可见区域 {count} 个", { count: report.fallback_visible })
              : null,
            // `rejected` is not `pending`: these windows refused the move back. Leaving
            // it out of the sentence meant a restore that put most windows home and was
            // refused by the rest reported a clean success.
            report.rejected > 0
              ? t("{count} 个没有接受恢复位置", { count: report.rejected })
              : null,
            // Distinct from `restored`: visible, but still owed the original place.
            report.awaiting_original > 0
              ? t("{count} 个原屏仍未恢复，暂放在可见区域", {
                  count: report.awaiting_original,
                })
              : null,
            report.pending > 0 ? t("仍有 {count} 个待处理", { count: report.pending }) : null,
          ]
            .filter(Boolean)
            .join(t("，")) + t("。"),
        };
      },
    );

  if (settings && !settings.onboarding_done && !settings.migration_notice && status && !pending?.present && status.pending_windows === 0 && status.captured_windows === 0 && status.recovery_blockers === 0) {
    return (
      <Onboarding
        displays={displays}
        settings={settings}
        thumbnails={thumbnails}
        busy={busy}
        onSelectTarget={async (nativeId) => {
          const result = await run(async () => { await api.setTargetDisplay(nativeId); return true; });
          return result === true;
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
        {/* No wordmark here: the title bar already says Unfy, and saying it
            twice stacked vertically read as duplication. */}
        <div className="h-4 shrink-0" />

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
            {status ? t(status.recovery_blockers > 0 ? "需要处理" : (status.pending_windows > 0 && state === "idle") ? "恢复记录仍待处理" : stateLabels[state]) : t("正在载入…")}
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
            {settings?.migration_notice ? (
              <div role="status" className="space-y-2 rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-xs">
                <p>{t("旧版主次布局已改为保留大小移入；旧版自动化已关闭。现有恢复记录保留，不会重新排列当前窗口。可在设置中重新选择已验证的连接触发。")}</p>
                <button type="button" className="min-h-8 px-2" disabled={busy} onClick={() => void saveSettings({ dismiss_migration_notice: true })}>{t("知道了")}</button>
              </div>
            ) : null}
            {restoreCountdown !== null ? (
              <div role="status" className="space-y-2 rounded-[var(--radius-control)] border border-[var(--color-border)] p-3 text-xs">
                <p>{t("远程连接已结束，{seconds} 秒后恢复。取消只停止本次计时，恢复记录会保留。", { seconds: restoreCountdown })}</p>
                <button type="button" className="min-h-8 px-2" disabled={busy} onClick={() => void onRestore()}>{t("立即恢复")}</button>
                <button type="button" className="min-h-8 px-2" disabled={busy} onClick={() => void run(async () => { await api.cancelAutoRestore(); clearCountdown(); })}>{t("取消本次自动恢复")}</button>
              </div>
            ) : null}
            {/* The chip is a warning about records this run did not create. A live
                gather also has unresolved windows - that is the layout on screen -
                and the overview already shows it with a restore button. */}
            {!pending?.live && ((status?.pending_windows ?? 0) > 0 || (status?.recovery_blockers ?? 0) > 0) ? (
              <div role="status" className="flex flex-wrap items-center gap-2 text-xs">
                <span>{t("恢复记录仍待处理")}</span>
                <button type="button" className="min-h-8 px-2" onClick={() => { setPage("overview"); void refresh(); }}>{t("查看恢复记录")}</button>
                <button type="button" className="min-h-8 px-2" disabled={busy} onClick={() => void onRestore()}>{t("尝试恢复")}</button>
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

            {pending && !pending.live && (pending.present || (page === "overview" && ((pending.transactions?.length ?? 0) > 0 || pending.blockers > 0))) ? (
              <PendingCard
                pending={pending}
                busy={busy}
                onRestore={() => void onRestore()}
                onRestoreTransaction={(transaction) => void onRestore(transaction)}
                onRecheck={() => void refresh()}
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
                refusedApps={refusedApps}
                activity={activity}
                progress={progress}
                latestResults={latestResults}
                busy={busy}
                recoveryPending={Boolean(!pending?.live && (pending?.present || pending?.transactions?.length))}
                onRecheck={() => void refresh()}
                onPreview={() =>
                  void run(
                    async () => {
                      const epoch = ++previewEpoch.current;
                      const next = await api.preview();
                      if (epoch === previewEpoch.current) setPreview(next);
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
                      clearCountdown();
                      invalidatePreview();
                      const result = await api.migrate();
                      invalidatePreview();
                      setRefusedApps(result.refused_apps);
                      setLatestResults(result.results);
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
                        // covered everything when it had not. `unchanged` is the
                        // opposite case and must not be read as a failure: those
                        // windows were already on the target and correctly received
                        // no write at all.
                        tone:
                          result.rejected > 0 || result.unplaced > 0
                            ? "warning"
                            : "success",
                        text:
                          // Each clause carries its own separator: concatenating the
                          // fragments read "Gathered 5 windowsskipped 7." in English
                          // and "已整理 5 个窗口跳过 7 个。" in Chinese.
                          t("已整理 {count} 个窗口", { count: result.applied }) +
                          (result.unchanged > 0
                            ? t("，{count} 个已在目标屏", { count: result.unchanged })
                            : "") +
                          (result.gone > 0
                            ? t("，{count} 个已关闭", { count: result.gone })
                            : "") +
                          (result.skipped > 0
                            ? t("，跳过 {count} 个", { count: result.skipped })
                            : "") +
                          (result.unplaced > 0
                            ? t("，{count} 个放不下留在原位", { count: result.unplaced })
                            : "") +
                          (result.not_attempted > 0
                            ? t("，{count} 个未能读取原位置，未移动", { count: result.not_attempted })
                            : "") +
                          refused +
                          t("。"),
                      };
                    },
                  )
                }
                onRestore={() => void onRestore()}
                elevation={elevation}
                onRestartElevated={() =>
                  void run(
                    () => api.restartElevated(),
                    () => ({ tone: "neutral", text: t("正在以管理员身份重启…") }),
                  )
                }
                onPause={(paused) =>
                  void run(
                    () => api.setPaused(paused),
                    (result) => ({
                      tone: "neutral",
                      text: result.paused
                        ? t("已暂停自动整理，当前布局保留。")
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
                  void run(() => api.identifyDisplays(4), (outcome) => ({
                    tone: outcome === "unavailable" ? "warning" : "neutral",
                    text: outcome === "unavailable" ? t("无法显示编号，请检查屏幕状态。") : outcome === "already_showing" ? t("编号正在显示，请对照屏幕确认。") : t("已在每块屏幕上显示编号。"),
                  }))
                }
                onOpenDisplays={() => setPage("displays")}
                onDismissError={() =>
                  void run(() => api.dismissLastError())
                }
                onKeepLayout={() =>
                  void run(
                    async () => { await api.keepCurrentLayout(); clearCountdown(); invalidatePreview(); },
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
                hasRecovery={(status?.captured_windows ?? 0) > 0}
                onRefreshThumbnails={() => captureShots.current()}
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
                platform={status?.platform}
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
                    () => api.openAppFolder("config"),  // the config directory
                    (opened) => ({
                      tone: opened ? "neutral" : "warning",
                      text: opened
                        ? t("已在文件管理器中打开配置目录。")
                        : t("无法打开配置目录。"),
                    }),
                  )
                }
                onInstallUpdate={() => void run(() => api.installUpdate(), (result) => ({
                  tone: result.outcome === "failed" ? "danger" : "neutral",
                  text: result.outcome === "installed" ? t("更新已安装，应用即将重启。") : result.outcome === "deferred" ? t("恢复记录仍待处理") : errorMessage(result.detail),
                }))}
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
                    async () => {
                      const result = await api.checkUpdate();
                      setUpdateStatus(await api.updateStatus().catch(() => updateStatus));
                      return result;
                    },
                    (result) => ({
                      tone:
                        result.outcome === "up_to_date"
                          ? "success"
                          : result.outcome === "failed"
                            ? "danger"
                            : "neutral",
                      text:
                        result.outcome === "available"
                          ? t("发现新版本")
                          : result.outcome === "up_to_date"
                          ? t("当前已是最新版本。")
                          : result.outcome === "deferred"
                            ? t("新版本已下载，但当前有未完成的整理，尚未安装。恢复完成后可以再试。")
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
