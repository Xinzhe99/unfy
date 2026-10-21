import { useState } from "react";

import type {
  MigrationDto,
  PreviewDto,
  RestoreDto,
  SettingsDto,
  StatusDto,
} from "../api";
import { PlanPreview } from "../components/PlanPreview";

/**
 * Overview page: the whole workflow in one screen — preview, gather, restore,
 * rescue. Everything here is reachable from the tray as well, so the page is a
 * convenience rather than a requirement.
 */
export function OverviewPage({
  status,
  settings,
  preview,
  lastMigration,
  lastRestore,
  busy,
  onPreview,
  onMigrate,
  onRestore,
  onRescue,
  onExport,
}: {
  status: StatusDto | null;
  settings: SettingsDto | null;
  preview: PreviewDto | null;
  lastMigration: MigrationDto | null;
  lastRestore: RestoreDto | null;
  busy: boolean;
  onPreview: () => void;
  onMigrate: () => void;
  onRestore: () => void;
  onRescue: () => void;
  onExport: () => void;
}) {
  const supported = status?.supported ?? false;
  const [showSkipped, setShowSkipped] = useState(false);
  const target = status?.target;

  return (
    <div className="space-y-4">
      <section className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">把窗口收拢到一块屏</h2>
            <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
              {target
                ? `目标：${target.label}${target.present ? "" : "（当前不可用，请重新选择）"}`
                : "尚未选择目标显示器，将使用主显示器。"}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={onPreview}
              disabled={busy || !supported}
              className="rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-medium disabled:opacity-50"
            >
              预览
            </button>
            <button
              type="button"
              onClick={onMigrate}
              disabled={busy || !supported}
              className="rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
            >
              整理到目标屏
            </button>
            <button
              type="button"
              onClick={onRestore}
              disabled={busy}
              className="rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-medium disabled:opacity-50"
            >
              恢复本次布局
            </button>
          </div>
        </div>

        {preview ? (
          <div className="mt-4 space-y-2">
            <PlanPreview preview={preview} />
            <p className="text-xs text-[var(--color-muted)]">
              将移动 {preview.placements.length} 个窗口。预览不会改变任何窗口。
            </p>
          </div>
        ) : null}

        {lastRestore ? (
          <p className="mt-3 text-xs text-[var(--color-muted)]">
            已恢复 {lastRestore.restored} 个窗口
            {lastRestore.kept_user_change > 0
              ? `，保留你的改动 ${lastRestore.kept_user_change} 个`
              : ""}
            {lastRestore.fallback_visible > 0
              ? `，原屏不可用回退 ${lastRestore.fallback_visible} 个`
              : ""}
            {lastRestore.pending > 0
              ? `，仍有 ${lastRestore.pending} 个待处理`
              : ""}
            。
          </p>
        ) : null}

        {lastMigration && lastMigration.refused_pids.length > 0 ? (
          <p className="mt-3 text-xs text-[var(--color-warning)]">
            {lastMigration.refused_pids.length} 个应用没有接受位置调整，已停止重试：
            进程 {lastMigration.refused_pids.join("、")}。这些窗口会保持原位，不影响其他窗口。
          </p>
        ) : null}

        {lastMigration && lastMigration.skipped > 0 ? (
          <button
            type="button"
            onClick={() => setShowSkipped((v) => !v)}
            className="mt-3 text-xs text-[var(--color-muted)] underline-offset-2 hover:underline"
          >
            {showSkipped ? "隐藏" : "查看"}被跳过的 {lastMigration.skipped} 个窗口
          </button>
        ) : null}
      </section>

      <section className="grid grid-cols-3 gap-3">
        <Stat label="检测到的显示器" value={String(status?.display_count ?? 0)} />
        <Stat label="布局" value={layoutLabel(settings?.layout_mode)} />
        <Stat
          label="快捷键"
          value={settings?.hotkey_gather ? settings.hotkey_gather : "未设置"}
        />
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-xs font-semibold">救援与诊断</h2>
        <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
          如果窗口被留在了不存在的坐标上（例如拔掉显示器、或上次未正常退出），可以把它们移回可见区域。这个操作不需要恢复记录，也不会覆盖你的布局。
        </p>
        <div className="mt-3 flex gap-2">
          <button
            type="button"
            onClick={onRescue}
            disabled={busy || !supported}
            className="rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-medium disabled:opacity-50"
          >
            救援越界窗口
          </button>
          <button
            type="button"
            onClick={onExport}
            className="rounded-lg px-3 py-1.5 text-xs font-medium text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            导出诊断包
          </button>
        </div>
        <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-muted)]">
          诊断包包含版本、显示器数量与设置项名称，不包含窗口标题、屏幕画面或文件路径。
        </p>
      </section>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-4">
      <p className="text-xs text-[var(--color-muted)]">{label}</p>
      <p className="mt-1 truncate text-sm font-semibold" title={value}>
        {value}
      </p>
    </div>
  );
}

export function layoutLabel(mode: SettingsDto["layout_mode"] | undefined): string {
  switch (mode) {
    case "primary_side":
      return "主次分区";
    case "preserve_order":
      return "保持顺序";
    default:
      return "网格";
  }
}
