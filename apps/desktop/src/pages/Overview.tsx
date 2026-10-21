import type {
  MigrationDto,
  PreviewDto,
  RestoreDto,
  SettingsDto,
  StatusDto,
} from "../api";
import { stateHints } from "../api";
import { PlanPreview } from "../components/PlanPreview";
import { Badge, Button, Card } from "../components/ui";

/**
 * Overview: the whole workflow on one screen.
 *
 * Ordering is deliberate and follows what the user needs to know, not what the
 * code happens to expose:
 *
 *  1. what state am I in, and what is the target;
 *  2. the one action that matters right now (gather, or restore);
 *  3. what will happen / what did happen;
 *  4. secondary tools (rescue, identify, diagnostics).
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
  onPause,
  onRescue,
  onIdentify,
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
  onPause: (paused: boolean) => void;
  onRescue: () => void;
  onIdentify: () => void;
  onExport: () => void;
}) {
  const supported = status?.supported ?? false;
  const state = status?.state ?? "idle";
  const active = state === "active";
  const paused = status?.paused ?? false;
  const captured = status?.captured_windows ?? 0;
  const target = status?.target;

  return (
    <div className="flex flex-col gap-4">
      <Card
        title="当前状态"
        description={stateHints[state]}
        aside={
          paused ? <Badge tone="warning">已暂停</Badge> : null
        }
      >
        <dl className="grid grid-cols-3 gap-3">
          <Stat label="目标显示器" value={target?.label ?? "主显示器"} muted={!target} />
          <Stat
            label="已收拢窗口"
            value={captured > 0 ? `${captured} 个` : "—"}
            muted={captured === 0}
          />
          <Stat
            label="布局"
            value={layoutLabel(settings?.layout_mode)}
          />
        </dl>

        {target && !target.present ? (
          <p className="mt-3 rounded-[var(--radius-control)] border border-[var(--color-warning)]/35 bg-[var(--color-warning-soft)] px-3 py-2 text-xs text-[var(--color-warning)]">
            已保存的目标显示器当前不可用，将回退到主显示器。请到「显示器与布局」重新选择。
          </p>
        ) : null}

        <div className="mt-4 flex flex-wrap items-center gap-2">
          {active ? (
            <>
              <Button tone="accent" onClick={onRestore} disabled={busy}>
                恢复本次布局
              </Button>
              <Button
                onClick={() => onPause(!paused)}
                disabled={busy}
              >
                {paused ? "取消暂停" : "暂停自动整理"}
              </Button>
            </>
          ) : (
            <>
              <Button tone="accent" onClick={onMigrate} disabled={busy || !supported}>
                整理到目标屏
              </Button>
              <Button onClick={onPreview} disabled={busy || !supported}>
                预览布局
              </Button>
            </>
          )}
          <Button tone="quiet" size="sm" onClick={onIdentify} disabled={busy}>
            识别屏幕编号
          </Button>
        </div>

        <p className="mt-2 text-[11px] text-[var(--color-muted)]">
          识别编号会在每块屏幕上短暂显示数字，用于确认远程端正在看哪一块。
        </p>
      </Card>

      {preview ? (
        <Card title="布局预览" description="预览不会移动任何窗口。">
          <PlanPreview preview={preview} />
        </Card>
      ) : null}

      {lastMigration ? (
        <Card title="上次整理结果">
          <ResultRow
            items={[
              { label: "已移动", value: lastMigration.applied, tone: "success" },
              { label: "应用拒绝", value: lastMigration.rejected, tone: "warning" },
              { label: "已关闭", value: lastMigration.gone, tone: "neutral" },
              { label: "跳过", value: lastMigration.skipped, tone: "neutral" },
              { label: "放不下", value: lastMigration.unplaced, tone: "warning" },
            ]}
          />
          {lastMigration.refused_apps.length > 0 ? (
            <p className="mt-3 text-xs leading-relaxed text-[var(--color-muted)]">
              {lastMigration.refused_apps.join("、")} 没有接受位置调整，已停止重试。
              这些窗口保持原位，不影响其他窗口。
            </p>
          ) : null}
        </Card>
      ) : null}

      {lastRestore ? (
        <Card title="上次恢复结果">
          <ResultRow
            items={[
              { label: "精确恢复", value: lastRestore.restored, tone: "success" },
              {
                label: "保留改动",
                value: lastRestore.kept_user_change,
                tone: "neutral",
              },
              {
                label: "回退可见",
                value: lastRestore.fallback_visible,
                tone: "neutral",
              },
              { label: "仍待处理", value: lastRestore.pending, tone: "warning" },
            ]}
          />
          {lastRestore.pending_apps.length > 0 ? (
            <p className="mt-3 text-xs leading-relaxed text-[var(--color-muted)]">
              {lastRestore.pending_apps.join("、")} 的窗口仍未恢复。
              可以重试，或在托盘菜单里选择保留当前布局。
            </p>
          ) : null}
        </Card>
      ) : null}

      <Card
        title="工具"
        description="这些操作不需要恢复记录，也不会覆盖你的布局。"
      >
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={onRescue} disabled={busy || !supported}>
            救援越界窗口
          </Button>
          <Button size="sm" onClick={onExport} disabled={busy}>
            导出诊断包
          </Button>
        </div>
        <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-muted)]">
          「救援」把停留在不存在坐标上的窗口移回可见区域。诊断包只含版本、显示器数量与设置项名称，不含窗口标题、屏幕画面或文件路径。
        </p>
      </Card>
    </div>
  );
}

function Stat({
  label,
  value,
  muted,
}: {
  label: string;
  value: string;
  muted?: boolean;
}) {
  return (
    <div className="rounded-[var(--radius-control)] bg-[var(--color-surface)] px-3 py-2">
      <dt className="text-[11px] text-[var(--color-muted)]">{label}</dt>
      <dd
        className={`mt-0.5 truncate text-[13px] font-medium ${muted ? "text-[var(--color-muted)]" : ""}`}
        title={value}
      >
        {value}
      </dd>
    </div>
  );
}

function ResultRow({
  items,
}: {
  items: {
    label: string;
    value: number;
    tone: "success" | "warning" | "neutral";
  }[];
}) {
  return (
    <div className="flex flex-wrap gap-x-6 gap-y-2">
      {items.map((item) => (
        <div key={item.label} className="flex items-baseline gap-1.5">
          <span
            className={`text-lg font-semibold tabular-nums ${
              item.value === 0
                ? "text-[var(--color-muted)]"
                : item.tone === "success"
                  ? "text-[var(--color-success)]"
                  : item.tone === "warning"
                    ? "text-[var(--color-warning)]"
                    : "text-[var(--color-fg)]"
            }`}
          >
            {item.value}
          </span>
          <span className="text-xs text-[var(--color-muted)]">{item.label}</span>
        </div>
      ))}
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
