import type { PreviewDto, RectDto } from "../api";
import { skipLabel } from "../api";

/**
 * Plan preview.
 *
 * Draws the target work area and every planned rectangle inside it, at the real
 * proportions. This is the only chance the user gets to see what will happen
 * before windows move, so it must not idealise the layout: if the planner had to
 * cascade because the screen is too small, the preview has to show overlapping
 * rectangles rather than a tidy grid.
 */
export function PlanPreview({ preview }: { preview: PreviewDto }) {
  const work = preview.target_work;
  const placed = preview.placements.length;

  // Two windows of the same application are indistinguishable in the drawing
  // otherwise, and "which explorer window goes where" is exactly the question a
  // user looking at two identical rectangles will ask.
  const duplicates = new Map<string, number>();
  for (const placement of preview.placements) {
    if (!placement.app_name) continue;
    duplicates.set(placement.app_name, (duplicates.get(placement.app_name) ?? 0) + 1);
  }

  if (placed === 0 && preview.skipped.length === 0) {
    return (
      <p className="rounded-[var(--radius-control)] border border-dashed border-[var(--color-border-strong)] px-4 py-6 text-center text-xs text-[var(--color-muted)]">
        当前没有可整理的窗口。
      </p>
    );
  }

  return (
    <div className="space-y-3">
      <div className="relative overflow-hidden rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)]">
        <svg
          viewBox={`${work.left} ${work.top} ${work.width} ${work.height}`}
          // Fill the card and let the intrinsic aspect ratio decide the height,
          // capped so a tall target does not push the rest of the page below the
          // fold. Fixed pixel dimensions made the drawing narrower than its
          // container, which read as unexplained margins inside the work area.
          className="mx-auto block h-auto w-full"
          style={{
            aspectRatio: `${work.width} / ${work.height}`,
            maxHeight: 300,
          }}
          role="img"
          aria-label={`布局预览：${placed} 个窗口排入 ${work.width}×${work.height} 的区域`}
          preserveAspectRatio="xMidYMid meet"
        >
          {/* Work-area boundary, so the user can see the margin is respected. */}
          <rect
            x={work.left}
            y={work.top}
            width={work.width}
            height={work.height}
            fill="none"
            stroke="var(--color-border-strong)"
            strokeWidth={Math.max(2, work.width * 0.002)}
          />
          {preview.placements.map((placement, index) => (
            <g key={placement.window_id}>
              <rect
                x={placement.rect.left}
                y={placement.rect.top}
                width={placement.rect.width}
                height={placement.rect.height}
                rx={work.width * 0.004}
                fill="var(--color-accent)"
                fillOpacity={placement.clamped ? 0.06 : 0.12}
                stroke="var(--color-accent)"
                strokeOpacity={placement.clamped ? 0.5 : 1}
                strokeWidth={Math.max(2, work.width * 0.0025)}
                strokeDasharray={placement.clamped ? "8 6" : undefined}
              />
              <text
                x={placement.rect.left + placement.rect.width / 2}
                y={placement.rect.top + placement.rect.height / 2}
                textAnchor="middle"
                dominantBaseline="central"
                fontSize={Math.max(work.width * 0.028, 10)}
                fill="var(--color-fg-soft)"
                fontFamily="inherit"
              >
                {/* The application name is what makes the drawing useful: a grid
                    of numbers tells the user nothing about where their work will
                    land. Long names are truncated by characters rather than with
                    SVG clipping, because a clipped word looks like a rendering
                    bug. */}
                {label(placement.app_name, index, duplicates)}
              </text>
            </g>
          ))}
        </svg>
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[var(--color-muted)]">
        <span>
          将移动 <strong className="font-medium text-[var(--color-fg)]">{placed}</strong> 个窗口
        </span>
        {preview.cascaded ? (
          <span className="text-[var(--color-warning)]">
            空间不足，窗口将叠放
          </span>
        ) : null}
        {preview.unplaced.length > 0 ? (
          <span className="text-[var(--color-warning)]">
            {preview.unplaced.length} 个窗口放不下，保持原位
          </span>
        ) : null}
      </div>

      {preview.skipped.length > 0 ? <SkippedList skipped={preview.skipped} /> : null}
    </div>
  );
}

/**
 * What to draw inside a rectangle.
 *
 * A short application name, with a `#2` suffix when the same application owns
 * several windows in this plan. Falls back to the plan position when the platform
 * could not read a name (elevated processes deny the query).
 */
function label(appName: string, index: number, duplicates: Map<string, number>): string {
  if (!appName) return `窗口 ${index + 1}`;
  const name = shortName(appName);
  return (duplicates.get(appName) ?? 0) > 1 ? `${name} #${index + 1}` : name;
}

/**
 * Executable name without its extension, trimmed to fit a small rectangle.
 *
 * `WindowsTerminal.exe` is the one the user recognises in the taskbar, but it
 * does not fit inside a 60 px wide box at any readable size; `WindowsT…` does,
 * and the numbered list underneath always carries the full name.
 */
function shortName(appName: string): string {
  if (!appName) return "";
  const base = appName.replace(/\.exe$/i, "");
  return base.length > 10 ? `${base.slice(0, 9)}…` : base;
}

function SkippedList({ skipped }: { skipped: PreviewDto["skipped"] }) {
  // Group by reason and name the applications, because "已排除此应用" without the
  // name leaves the user unable to confirm the rule matched what they expected.
  // Reasons that are not about a specific application stay as a count.
  const byReason = new Map<string, Map<string, number>>();
  for (const item of skipped) {
    const apps = byReason.get(item.reason) ?? new Map<string, number>();
    if (item.app_name) {
      apps.set(item.app_name, (apps.get(item.app_name) ?? 0) + 1);
    }
    byReason.set(item.reason, apps);
  }

  const entries = [...byReason.entries()].sort(
    (a, b) =>
      countOf(b[1]) - countOf(a[1]) || a[0].localeCompare(b[0]),
  );

  return (
    <details className="group text-xs text-[var(--color-muted)]">
      <summary className="cursor-pointer select-none list-none rounded-[var(--radius-control)] px-1 py-1 hover:text-[var(--color-fg)]">
        <span className="inline-block transition-transform group-open:rotate-90">
          ›
        </span>{" "}
        已跳过 {skipped.length} 个窗口
      </summary>
      <ul className="mt-1 space-y-1 pl-4">
        {entries.map(([reason, apps]) => (
          <li key={reason}>
            {skipLabel(reason)} · {countOf(apps)} 个
            {apps.size > 0 ? (
              <span className="text-[var(--color-muted)]/70">
                {" "}
                （{[...apps.keys()].slice(0, 4).join("、")}
                {apps.size > 4 ? " 等" : ""}）
              </span>
            ) : null}
          </li>
        ))}
      </ul>
    </details>
  );
}

function countOf(apps: Map<string, number>): number {
  let total = 0;
  for (const count of apps.values()) total += count;
  return total;
}

/** Convert a rect to a CSS-friendly string; used by tests and future overlays. */
export function rectLabel(rect: RectDto): string {
  return `${rect.width}×${rect.height} @ ${rect.left},${rect.top}`;
}
