import { useState } from "react";

import type { SettingsDto } from "../api";
import { Button, Card, EmptyState } from "../components/ui";

/**
 * Application exclusions.
 *
 * The engine moves every eligible window, which is the point — but a few
 * applications should never be rearranged (capture tools, media players, a
 * dashboard on a spare screen). This page is the escape hatch.
 *
 * Rules match the executable file name only, never a path or a wildcard: a path
 * would not survive being compared against the name the platform reports, and a
 * pattern would make a typo silently match nothing. Both are rejected by the Rust
 * validator, and the hint here says so before the user tries.
 */
export function ExclusionsPage({
  settings,
  busy,
  onSave,
}: {
  settings: SettingsDto | null;
  busy: boolean;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  const [draft, setDraft] = useState("");

  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  const trimmed = draft.trim();
  const duplicate = settings.excluded_apps.some(
    (app) => app.toLowerCase() === trimmed.toLowerCase(),
  );
  const invalid = trimmed.length > 0 && /[\\/*?]/.test(trimmed);
  const canAdd = trimmed.length > 0 && !duplicate && !invalid && !busy;

  const add = () => {
    if (!canAdd) return;
    onSave({ excluded_apps: [...settings.excluded_apps, trimmed] });
    setDraft("");
  };

  return (
    <div className="flex flex-col gap-4">
      <Card
        title="排除的应用"
        description="这些应用的窗口永远不会被移动。按可执行文件名匹配，忽略大小写。"
      >
        <div className="flex gap-2">
          <input
            type="text"
            value={draft}
            disabled={busy}
            placeholder="例如 obs64.exe"
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                add();
              }
            }}
            className="h-9 flex-1 rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-[13px] outline-none focus:border-[var(--color-accent)]"
            aria-label="要排除的应用名"
            aria-invalid={invalid || duplicate}
          />
          <Button tone="accent" onClick={add} disabled={!canAdd}>
            添加
          </Button>
        </div>
        {invalid ? (
          <p className="mt-2 text-xs text-[var(--color-warning)]">
            只能填文件名，不能包含路径或通配符（* ? / \）。
          </p>
        ) : duplicate ? (
          <p className="mt-2 text-xs text-[var(--color-warning)]">
            这个应用已经在列表里了。
          </p>
        ) : (
          <p className="mt-2 text-[11px] text-[var(--color-muted)]">
            名称区分不了时，可以先整理一次，然后在「概览」的跳过清单里查看真实的应用名。
          </p>
        )}

        <div className="mt-4">
          {settings.excluded_apps.length === 0 ? (
            <EmptyState title="还没有排除任何应用">
              列表为空时，所有合格窗口都会参与整理。
            </EmptyState>
          ) : (
            <ul className="divide-y divide-[var(--color-border)]">
              {settings.excluded_apps.map((app) => (
                <li
                  key={app}
                  className="flex items-center justify-between gap-4 py-2"
                >
                  <span className="truncate text-[13px]">{app}</span>
                  <Button
                    size="sm"
                    tone="quiet"
                    disabled={busy}
                    onClick={() =>
                      onSave({
                        excluded_apps: settings.excluded_apps.filter(
                          (entry) => entry !== app,
                        ),
                      })
                    }
                  >
                    移除
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Card>
    </div>
  );
}
