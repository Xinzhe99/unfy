import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  api,
  errorMessage,
  stateLabels,
  type DisplayDto,
  type EngineStateName,
  type MigrationDto,
  type PendingWorkDto,
  type PreviewDto,
  type RestoreDto,
  type SettingsDto,
  type StatusDto,
} from "./api";
import { PermissionBanner } from "./components/PermissionBanner";
import { DisplaysPage } from "./pages/Displays";
import { GeneralPage } from "./pages/General";
import { OverviewPage } from "./pages/Overview";

export type Page = "overview" | "displays" | "general";

const pages: { id: Page; label: string }[] = [
  { id: "overview", label: "概览" },
  { id: "displays", label: "显示器与布局" },
  { id: "general", label: "通用" },
];

const stateTone: Record<EngineStateName, string> = {
  idle: "text-[var(--color-muted)]",
  planning: "text-[var(--color-accent)]",
  migrating: "text-[var(--color-accent)]",
  active: "text-[var(--color-success)]",
  restoring: "text-[var(--color-accent)]",
  degraded: "text-[var(--color-warning)]",
  error: "text-[var(--color-danger)]",
};

export default function App() {
  const [page, setPage] = useState<Page>("overview");
  const [status, setStatus] = useState<StatusDto | null>(null);
  const [displays, setDisplays] = useState<DisplayDto[]>([]);
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [preview, setPreview] = useState<PreviewDto | null>(null);
  const [pending, setPending] = useState<PendingWorkDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastMigration, setLastMigration] = useState<MigrationDto | null>(null);
  const [lastRestore, setLastRestore] = useState<RestoreDto | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextDisplays, nextSettings, nextPending] =
        await Promise.all([
          api.getStatus(),
          api.listDisplays(),
          api.getSettings(),
          api.pendingWork(),
        ]);
      setStatus(nextStatus);
      setDisplays(nextDisplays);
      setSettings(nextSettings);
      setPending(nextPending);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // The theme is a class on <html> so it applies before React paints a new
  // colour and so the CSS can use plain custom properties.
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
    const unlisten = listen("conflux://action", () => {
      void refresh();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const run = useCallback(
    async <T,>(action: () => Promise<T>, success?: (value: T) => string | null) => {
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        const value = await action();
        const message = success?.(value);
        if (message) setNotice(message);
        return value;
      } catch (e) {
        setError(errorMessage(e));
        return null;
      } finally {
        setBusy(false);
        void refresh();
      }
    },
    [refresh],
  );

  const state = status?.state ?? "idle";
  const supported = status?.supported ?? false;

  const onRestore = () =>
    run(
      async () => {
        const report = await api.restore();
        setLastRestore(report);
        return report;
      },
      (report) =>
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
    );

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-[var(--color-border)] px-5 py-3">
        <div className="flex items-center gap-3">
          <h1 className="text-sm font-semibold tracking-tight">Conflux</h1>
          <span
            className={`inline-flex items-center gap-1.5 text-xs ${stateTone[state]}`}
          >
            <span
              aria-hidden
              className="inline-block size-1.5 rounded-full bg-current"
            />
            {stateLabels[state]}
          </span>
          {!supported ? (
            <span className="rounded-full bg-[var(--color-warning)]/14 px-2 py-0.5 text-[10px] font-medium text-[var(--color-warning)]">
              只读演示模式
            </span>
          ) : null}
        </div>
        <nav className="flex gap-1" aria-label="设置页面">
          {pages.map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => setPage(item.id)}
              aria-current={page === item.id ? "page" : undefined}
              className={`rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors ${
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

      <main className="flex-1 overflow-y-auto p-5">
        {error ? (
          <div
            role="alert"
            className="mb-4 rounded-lg border border-[var(--color-danger)]/40 bg-[var(--color-danger)]/8 p-3 text-xs text-[var(--color-danger)]"
          >
            {error}
          </div>
        ) : null}
        {notice ? (
          <div
            role="status"
            className="mb-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-3 text-xs"
          >
            {notice}
          </div>
        ) : null}

        {status ? (
          <PermissionBanner
            status={status}
            busy={busy}
            onRequest={() =>
              void run(() => api.requestPermission(), () =>
                "已打开系统授权面板。授权后请点击「重新检查」。",
              )
            }
            onRecheck={() => void refresh()}
          />
        ) : null}

        {pending?.present ? (
          <div className="mb-4 rounded-lg border border-[var(--color-warning)]/40 bg-[var(--color-warning)]/8 p-4">
            <p className="text-sm font-medium">上次整理未完成</p>
            <p className="mt-1 text-xs text-[var(--color-muted)]">
              有 {pending.windows} 个窗口的快照仍待处理
              {pending.may_have_moved ? "，其中部分可能已被移动" : ""}。
            </p>
            <div className="mt-3 flex gap-2">
              <button
                type="button"
                onClick={() => void onRestore()}
                className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-xs font-medium text-white"
              >
                尝试恢复
              </button>
              <button
                type="button"
                onClick={() =>
                  void run(() => api.dismissPendingWork(), () => "已保留当前布局。")
                }
                className="rounded-md px-3 py-1.5 text-xs font-medium text-[var(--color-muted)] hover:text-[var(--color-fg)]"
              >
                保留当前布局
              </button>
            </div>
          </div>
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
              void run(async () => {
                const next = await api.preview();
                setPreview(next);
                return next;
              })
            }
            onMigrate={() =>
              void run(
                async () => {
                  const result = await api.migrate();
                  setLastMigration(result);
                  setPreview(null);
                  return result;
                },
                (result) =>
                  `已整理 ${result.applied} 个窗口` +
                  (result.rejected > 0
                    ? `，${result.rejected} 个应用未接受`
                    : "") +
                  (result.gone > 0 ? `，${result.gone} 个已关闭` : "") +
                  (result.skipped > 0 ? `，跳过 ${result.skipped} 个` : "") +
                  "。",
              )
            }
            onRestore={() => void onRestore()}
            onRescue={() =>
              void run(
                () => api.rescueOffscreen(),
                (count) =>
                  count === 0
                    ? "没有发现越界窗口。"
                    : `已把 ${count} 个窗口移回可见区域。`,
              )
            }
            onExport={() =>
              void run(
                () => api.exportDiagnostics(),
                (result) => `诊断包已写入 ${result.path}（${result.bytes} 字节）。`,
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
                () => "目标显示器已保存。",
              )
            }
            onSave={(patch) =>
              void run(
                async () => {
                  const next = await api.saveSettings(patch);
                  setSettings(next);
                  return next;
                },
                () => "设置已保存。",
              )
            }
          />
        ) : null}

        {page === "general" ? (
          <GeneralPage
            settings={settings}
            busy={busy}
            onSave={(patch) =>
              void run(
                async () => {
                  const next = await api.saveSettings(patch);
                  setSettings(next);
                  return next;
                },
                () => "设置已保存。",
              )
            }
          />
        ) : null}
      </main>
    </div>
  );
}
