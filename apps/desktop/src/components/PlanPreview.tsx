import { t } from "../i18n";
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
  const squeezed = preview.placements.filter((p) => p.clamped).length;

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
        {t("没有可整理的窗口")}
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
          aria-label={t("布局预览：{placed} 个窗口排入 {width}×{height} 的区域", {
            placed,
            width: work.width,
            height: work.height,
          })}
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
              {/* SVG has no `title` attribute; a `<title>` child is the standard way
                  to give an element a tooltip, and the label inside the box is
                  shortened on purpose. Hovering reveals the full name and, for a
                  squeezed window, why it is drawn dashed. */}
              <title>{`${placement.app_name || t("窗口 {number}", { number: index + 1 })}${
                placement.clamped
                  ? t("：比它的格子需要更大，会压到相邻窗口")
                  : ""
              }`}</title>
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

      {preview.target_present ? null : (
        <p className="rounded-[var(--radius-control)] border border-[var(--color-border)] border-l-2 border-l-[var(--color-warning)] px-3 py-2 text-xs leading-relaxed text-[var(--color-fg-soft)]">
          {t("你选择的显示器当前不可用，这次会整理到主显示器。")}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[var(--color-muted)]">
        <span>
          {t("将移动 {count} 个窗口", { count: placed })}
        </span>
        {preview.cascaded ? (
          <span className="text-[var(--color-warning)]">
            {t("空间不足，窗口将叠放")}
          </span>
        ) : null}
        {preview.unplaced.length > 0 ? (
          <span className="text-[var(--color-warning)]">
            {t("{count} 个窗口放不下，保持原位", { count: preview.unplaced.length })}
          </span>
        ) : null}
        {squeezed > 0 ? (
          <span>
            {/* Dashed cells already meant this; without a legend the drawing just
                looked wrong. The count matters: "one of eight will overlap" is a
                different decision from "most of them will". */}
            {t("虚线框的窗口比格子需要更大，会略微压到相邻窗口（{count} 个）", {
              count: squeezed,
            })}
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
  if (!appName) return t("窗口 {number}", { number: index + 1 });
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
        {t("已跳过 {count} 个窗口", { count: skipped.length })}
      </summary>
      <ul className="mt-1 space-y-1 pl-4">
        {entries.map(([reason, apps]) => (
          <li key={reason}>
            {t("{reason} · {count} 个窗口", {
              reason: skipLabel(reason),
              count: countOf(apps),
            })}
            {apps.size > 0 ? (
              <span className="text-[var(--color-muted)]/70">
                {" "}
                {t("（{apps}）", { apps: appNames(apps) })}
              </span>
            ) : null}
          </li>
        ))}
      </ul>
    </details>
  );
}

/**
 * The application names beside a skip reason.
 *
 * Both the separator and the "and more" marker are locale-specific, so neither is
 * written inline: English needs ", " where Chinese needs "、".
 */
function appNames(apps: Map<string, number>): string {
  const shown = [...apps.keys()].slice(0, 4);
  const list = shown.join(t("、"));
  return apps.size > shown.length ? t("{list} 等", { list }) : list;
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
