import type { PreviewDto } from "../api";
import { skipLabel } from "../api";

/**
 * Plan preview.
 *
 * Draws every planned rectangle inside a proportional work-area canvas. This is
 * the user's only chance to see what will happen before windows move, so the
 * canvas must reflect the real coordinates rather than an idealised grid.
 */
export function PlanPreview({ preview }: { preview: PreviewDto }) {
  if (preview.placements.length === 0) {
    return (
      <p className="text-xs text-[var(--color-muted)]">
        没有可整理的窗口。部分窗口可能已最小化、全屏或被排除。
      </p>
    );
  }

  const bounds = preview.placements.reduce(
    (acc, p) => ({
      minX: Math.min(acc.minX, p.rect.left),
      minY: Math.min(acc.minY, p.rect.top),
      maxX: Math.max(acc.maxX, p.rect.left + p.rect.width),
      maxY: Math.max(acc.maxY, p.rect.top + p.rect.height),
    }),
    {
      minX: Number.POSITIVE_INFINITY,
      minY: Number.POSITIVE_INFINITY,
      maxX: Number.NEGATIVE_INFINITY,
      maxY: Number.NEGATIVE_INFINITY,
    },
  );

  const width = Math.max(bounds.maxX - bounds.minX, 1);
  const height = Math.max(bounds.maxY - bounds.minY, 1);
  const viewBox = `${bounds.minX} ${bounds.minY} ${width} ${height}`;

  return (
    <div className="space-y-2">
      <svg
        viewBox={viewBox}
        className="h-40 w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
        role="img"
        aria-label={`布局预览，共 ${preview.placements.length} 个窗口`}
        preserveAspectRatio="xMidYMid meet"
      >
        {preview.placements.map((placement, index) => (
          <g key={placement.window_id}>
            <rect
              x={placement.rect.left}
              y={placement.rect.top}
              width={placement.rect.width}
              height={placement.rect.height}
              rx={Math.max(4, width * 0.006)}
              fill="var(--color-accent)"
              fillOpacity={0.12}
              stroke="var(--color-accent)"
              strokeWidth={Math.max(1, width * 0.002)}
            />
            <text
              x={placement.rect.left + placement.rect.width / 2}
              y={placement.rect.top + placement.rect.height / 2}
              textAnchor="middle"
              dominantBaseline="middle"
              fontSize={Math.max(8, width * 0.028)}
              fill="var(--color-fg)"
            >
              {index + 1}
            </text>
          </g>
        ))}
      </svg>
      {preview.cascaded ? (
        <p className="text-xs text-[var(--color-warning)]">
          目标屏空间不足，窗口将以叠放方式排布，标题栏保持可点击。
        </p>
      ) : null}
      {preview.unplaced.length > 0 ? (
        <p className="text-xs text-[var(--color-warning)]">
          另有 {preview.unplaced.length} 个窗口无法排入，将保持原位。
        </p>
      ) : null}
      {preview.skipped.length > 0 ? (
        <details className="text-xs text-[var(--color-muted)]">
          <summary className="cursor-pointer select-none">
            已跳过 {preview.skipped.length} 个窗口
          </summary>
          <ul className="mt-1 space-y-0.5 pl-4">
            {preview.skipped.map((skip) => (
              <li key={skip.window_id}>
                {skip.window_id} · {skipLabel(skip.reason)}
              </li>
            ))}
          </ul>
        </details>
      ) : null}
    </div>
  );
}
