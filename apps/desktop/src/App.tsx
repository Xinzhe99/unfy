import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  api,
  errorMessage,
  formatBytes,
  stateLabels,
  type DisplayDto,
  type EngineStateName,
  type HotkeyStatusDto,
  type MigrationDto,
  type PendingWorkDto,
  type PreviewDto,
  type RestoreDto,
  type SettingsDto,
  type StatusDto,
} from "./api";
import { PermissionBanner } from "./components/PermissionBanner";
import { DisplaysPage } from "./pages/Displays";
import { ExclusionsPage } from "./pages/Exclusions";
import { GeneralPage } from "./pages/General";
import { OverviewPage } from "./pages/Overview";

type Page = "overview" | "displays" | "exclusions" | "general";

const pages: { id: Page; label: string }[] = [
  { id: "overview", label: "概览" },
  { id: "displays", label: "显示器与布局" },
  { id: "exclusions", label: "排除的应用" },
  { id: "general", label: "通用" },
];

/** Status colour per engine state, used by the header dot and the accent bar. */
const stateTone: Record<EngineStateName, "neutral" | "accent" | "success" | "warning" | "danger"> = {
  idle: "neutral",
  planning: "accent",
  migrating: "accent",
  active: "success",
  restoring: "accent",
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
  const [hotkeys, setHotkeys] = useState<HotkeyStatusDto | null>(null);
  const [lastMigration, setLastMigration] = useState<MigrationDto | null>(null);
  const [lastRestore, setLastRestore] = useState<RestoreDto | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);
  const noticeTimer = useRef<number | null>(null);

  const flash = useCallback((next: Notice) => {
    setNotice(next);
    if (noticeTimer.current !== null) {
      window.clearTimeout(noticeTimer.current);
    }
    noticeTimer.current = window.setTimeout(() => setNotice(null), 6000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextDisplays, nextSettings, nextPending, nextHotkeys] =
        await Promise.all([
          api.getStatus(),
          api.listDisplays(),
          api.getSettings(),
          api.pendingWork(),
          api.hotkeyStatus(),
        ]);
      setStatus(nextStatus);
      setDisplays(nextDisplays);
      setSettings(nextSettings);
      setPending(nextPending);
      setHotkeys(nextHotkeys);
    } catch (e) {
      flash({ tone: "danger", text: errorMessage(e) });
    }
  }, [flash]);

  // Theme is a class on <html> so the CSS custom properties switch atomically.
  useEffect(() => {
    const theme = settings?.theme ?? "system";
    const root = document.documentElement;
    root.classList.remove("theme-system", "theme-light", "theme-dark");
    root.classList.add(`theme-${theme}`);
  }, [settings?.theme]);

  useEffect(() => {
    void refresh();

    // Tray and hotkey actions happen outside this window; every one of them
    // emits an event so the UI never shows a stale engine state.
    const unlistenAction = listen<{ action: string; ok: boolean; message?: string }>(
      "conflux://action",
      (event) => {
        void refresh();
        if (!event.payload.ok && event.payload.message) {
          flash({ tone: "danger", text: errorMessage(event.payload.message) });
        }
      },
    );
    const unlistenExit = listen("conflux://exit-blocked", () => {
      flash({
        tone: "warning",
        text: "还有窗口没有恢复，已取消退出。请先恢复布局，或选择保留当前布局。",
      });
    });
    // The engine reports when a long migration starts and finishes. Without it
    // the button looks unresponsive for the second or two the platform calls
    // take, and a user is likely to click again.
    const unlistenProgress = listen<{ phase: string }>(
      "conflux://progress",
      (event) => {
        setBusy(event.payload.phase !== "done");
        if (event.payload.phase === "done") void refresh();
      },
    );
    const unlistenUpdate = listen<{ outcome: string; version?: string }>(
      "conflux://update",
      (event) => {
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

    // The engine can be driven from the tray while this window is open, so poll
    // slowly as a backstop. Events cover the common path; polling covers a missed
    // event and external changes such as a monitor being unplugged.
    const timer = window.setInterval(() => void refresh(), 4000);

    return () => {
      void unlistenAction.then((fn) => fn());
      void unlistenExit.then((fn) => fn());
      void unlistenUpdate.then((fn) => fn());
      void unlistenProgress.then((fn) => fn());
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
        flash({ tone: "danger", text: errorMessage(e) });
        return null;
      } finally {
        setBusy(false);
        void refresh();
      }
    },
    [flash, refresh],
  );

  const state = status?.state ?? "idle";
  const supported = status?.supported ?? false;
  const tone = stateTone[state];

  const onRestore = () =>
    run(
      async () => {
        const report = await api.restore();
        setLastRestore(report);
        return report;
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

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-5">
        <div className="flex items-center gap-3">
          <span className="text-[13px] font-semibold tracking-tight">Conflux</span>
          <span
            className={`inline-flex items-center gap-1.5 text-xs ${
              tone === "success"
                ? "text-[var(--color-success)]"
                : tone === "warning"
                  ? "text-[var(--color-warning)]"
                  : tone === "danger"
                    ? "text-[var(--color-danger)]"
                    : tone === "accent"
                      ? "text-[var(--color-accent)]"
                      : "text-[var(--color-muted)]"
            }`}
          >
            <span aria-hidden className="inline-block size-1.5 rounded-full bg-current" />
            {stateLabels[state]}
            {status?.paused ? " · 已暂停" : ""}
          </span>
          {!supported ? (
            <span className="rounded-full bg-[var(--color-warning-soft)] px-2 py-0.5 text-[10px] font-medium text-[var(--color-warning)]">
              只读演示模式
            </span>
          ) : null}
        </div>
        <nav className="flex gap-0.5" aria-label="页面">
          {pages.map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => setPage(item.id)}
              aria-current={page === item.id ? "page" : undefined}
              className={`rounded-[var(--radius-control)] px-3 py-1.5 text-xs font-medium transition-colors ${
                page === item.id
                  ? "bg-[var(--color-surface-2)] text-[var(--color-fg)]"
                  : "text-[var(--color-muted)] hover:text-[var(--color-fg)]"
              }`}
            >
              {item.label}
            </button>
          ))}
        </nav>
      </header>

      <main className="flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-3xl flex-col gap-4 p-5">
          {notice ? (
            <div
              role={notice.tone === "danger" ? "alert" : "status"}
              className={`animate-in rounded-[var(--radius-control)] border px-4 py-2.5 text-xs ${
                notice.tone === "danger"
                  ? "border-[var(--color-danger)]/35 bg-[var(--color-danger-soft)] text-[var(--color-danger)]"
                  : notice.tone === "warning"
                    ? "border-[var(--color-warning)]/35 bg-[var(--color-warning-soft)] text-[var(--color-warning)]"
                    : notice.tone === "success"
                      ? "border-[var(--color-success)]/35 bg-[var(--color-success-soft)] text-[var(--color-success)]"
                      : "border-[var(--color-border)] bg-[var(--color-surface-2)] text-[var(--color-fg-soft)]"
              }`}
            >
              {notice.text}
            </div>
          ) : null}

          {status ? (
            <PermissionBanner
              status={status}
              busy={busy}
              onRecheck={() => void refresh()}
            />
          ) : null}

          {pending?.present ? (
            <PendingBanner
              pending={pending}
              busy={busy}
              onRestore={() => void onRestore()}
              onDismiss={() =>
                void run(
                  () => api.dismissPendingWork(),
                  () => ({ tone: "neutral", text: "已隐藏提示。恢复记录仍然保留。" }),
                )
              }
            />
          ) : null}

          {page === "overview" ? (
            <OverviewPage
              status={status}
              settings={settings}
              preview={preview}
              lastMigration={lastMigration}
              lastRestore={lastRestore}
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
                    setLastMigration(result);
                    setPreview(null);
                    return result;
                  },
                  (result) => ({
                    tone: result.rejected > 0 ? "warning" : "success",
                    text:
                      `已整理 ${result.applied} 个窗口` +
                      (result.rejected > 0
                        ? `，${result.rejected} 个应用未接受`
                        : "") +
                      (result.gone > 0 ? `，${result.gone} 个已关闭` : "") +
                      (result.skipped > 0 ? `，跳过 ${result.skipped} 个` : "") +
                      "。",
                  }),
                )
              }
              onRestore={() => void onRestore()}
              onPause={(paused) =>
                void run(
                  () => api.setPaused(paused),
                  (result) => ({
                    tone: "neutral",
                    text: result.paused
                      ? "已暂停自动整理。当前布局仍然保留。"
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
                void run(
                  () => api.identifyDisplays(4),
                  (shown) => ({
                    tone: shown ? "neutral" : "warning",
                    text: shown
                      ? "已在每块屏幕上显示编号，4 秒后自动消失。"
                      : "无法显示编号，请检查屏幕状态。",
                  }),
                )
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
            />
          ) : null}

          {page === "displays" ? (
            <DisplaysPage
              displays={displays}
              settings={settings}
              target={status?.target ?? null}
              busy={busy}
              onSelect={(nativeId) =>
                void run(
                  () => api.setTargetDisplay(nativeId),
                  () => ({ tone: "success", text: "目标显示器已保存。" }),
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
              onSave={(patch) =>
                void run(
                  async () => {
                    const next = await api.saveSettings(patch);
                    setSettings(next);
                    return next;
                  },
                  () => null,
                )
              }
            />
          ) : null}

          {page === "exclusions" ? (
            <ExclusionsPage
              settings={settings}
              busy={busy}
              onSave={(patch) =>
                void run(
                  async () => {
                    const next = await api.saveSettings(patch);
                    setSettings(next);
                    return next;
                  },
                  () => null,
                )
              }
            />
          ) : null}

          {page === "general" ? (
            <GeneralPage
              settings={settings}
              hotkeys={hotkeys}
              busy={busy}
              onSave={(patch) =>
                void run(
                  async () => {
                    const next = await api.saveSettings(patch);
                    setSettings(next);
                    return next;
                  },
                  () => null,
                )
              }
            />
          ) : null}
        </div>
      </main>
    </div>
  );
}

function PendingBanner({
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
    <div className="animate-in rounded-[var(--radius-control)] border border-[var(--color-warning)]/35 bg-[var(--color-warning-soft)] px-4 py-3">
      <p className="text-[13px] font-medium">上次整理未完成</p>
      <p className="mt-1 text-xs leading-relaxed text-[var(--color-fg-soft)]">
        有 {pending.windows} 个窗口的快照仍待处理
        {pending.may_have_moved ? "，其中部分可能已被移动" : ""}。
        {pending.truncated_bytes > 0
          ? " 记录尾部有损坏，已单独保存用于诊断。"
          : ""}
      </p>
      <div className="mt-3 flex gap-2">
        <button
          type="button"
          onClick={onRestore}
          disabled={busy}
          className="rounded-[var(--radius-control)] bg-[var(--color-accent)] px-3 py-1.5 text-xs font-medium text-[var(--color-accent-fg)] disabled:opacity-50"
        >
          尝试恢复
        </button>
        <button
          type="button"
          onClick={onDismiss}
          disabled={busy}
          className="rounded-[var(--radius-control)] px-3 py-1.5 text-xs font-medium text-[var(--color-fg-soft)] hover:text-[var(--color-fg)]"
        >
          稍后处理
        </button>
      </div>
    </div>
  );
}
