import type {
  MigrationDto,
  PreviewDto,
  RestoreDto,
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
  onOpenDisplays,
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
  onOpenDisplays: () => void;
}) {
  const supported = status?.supported ?? false;
  const state = status?.state ?? "idle";
  const active = state === "active";
  const paused = status?.paused ?? false;
  const captured = status?.captured_windows ?? 0;
  const target = status?.target;

  return (
    <>
      <Card
        title="当前状态"
        aside={
          paused ? (
            <Status tone="warning">已暂停</Status>
          ) : active ? (
            <Status tone="accent">布局已启用</Status>
          ) : (
            <Status tone="neutral">就绪</Status>
          )
        }
      >
        <div className="space-y-0.5">
          <Detail
            label="目标显示器"
            value={target?.label ?? "主显示器"}
            muted={!target}
          />
          <Detail
            label="已收拢窗口"
            value={captured > 0 ? captured : "—"}
            muted={captured === 0}
          />
          <Detail label="布局" value={layoutLabel(settings?.layout_mode)} />
        </div>

        {target && !target.present ? (
          <button
            type="button"
            onClick={onOpenDisplays}
            className="mt-2.5 flex w-full items-center justify-between gap-2 rounded-[var(--radius-control)] border border-[var(--color-border)] px-3 py-2 text-left text-xs text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-hover)]"
          >
            <span>已保存的目标显示器当前不可用，将回退到主显示器。</span>
            <IconArrowRight className="size-3.5 shrink-0 opacity-50" />
          </button>
        ) : null}

        <div className="mt-3 flex flex-wrap items-center gap-1.5">
          {active ? (
            <>
              <Button variant="primary" onClick={onRestore} disabled={busy}>
                恢复本次布局
              </Button>
              <Button onClick={() => onPause(!paused)} disabled={busy}>
                {paused ? "取消暂停" : "暂停"}
              </Button>
            </>
          ) : (
            <>
              <Button
                variant="primary"
                onClick={onMigrate}
                disabled={busy || !supported}
              >
                整理到目标屏
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

      {lastMigration ? (
        <Card title="上次整理">
          <Counts
            items={[
              { label: "已移动", value: lastMigration.applied },
              { label: "应用拒绝", value: lastMigration.rejected },
              { label: "已关闭", value: lastMigration.gone },
              { label: "跳过", value: lastMigration.skipped },
              { label: "放不下", value: lastMigration.unplaced },
            ]}
          />
          {lastMigration.refused_apps.length > 0 ? (
            <p className="mt-2 text-xs leading-relaxed text-[var(--color-muted)]">
              {lastMigration.refused_apps.join("、")} 没有接受位置调整，已停止重试。
            </p>
          ) : null}
        </Card>
      ) : null}

      {lastRestore ? (
        <Card title="上次恢复">
          <Counts
            items={[
              { label: "精确恢复", value: lastRestore.restored },
              { label: "保留改动", value: lastRestore.kept_user_change },
              { label: "回退可见", value: lastRestore.fallback_visible },
              { label: "待处理", value: lastRestore.pending },
            ]}
          />
          {lastRestore.pending_apps.length > 0 ? (
            <p className="mt-2 text-xs leading-relaxed text-[var(--color-muted)]">
              {lastRestore.pending_apps.join("、")} 的窗口仍未恢复，可以重试。
            </p>
          ) : null}
        </Card>
      ) : null}

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
          「救援」把停留在不存在坐标上的窗口移回可见区域。诊断包只含版本、显示器数量与设置项名称。
        </p>
      </Card>
    </>
  );
}

function Counts({
  items,
}: {
  items: { label: string; value: number }[];
}) {
  return (
    <div className="flex flex-wrap gap-x-5 gap-y-1.5">
      {items.map((item) => (
        <div key={item.label} className="flex items-baseline gap-1.5">
          <span
            className={`text-[15px] font-semibold tabular-nums ${
              item.value === 0
                ? "text-[var(--color-muted)]"
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
