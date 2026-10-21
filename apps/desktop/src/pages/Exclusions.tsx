import { useState } from "react";

import type { AppDto, SettingsDto } from "../api";
import { Button, Card, Switch } from "../components/ui";

/**
 * Exclusions.
 *
 * Exclusions are picked from the applications that actually own a window right
 * now. Typing an executable name by hand is a guessing game: the string has to
 * match exactly, and a typo fails silently — the app keeps getting moved and the
 * user has no idea why the rule did nothing.
 *
 * The text field stays for the case the picker cannot cover: an app that is not
 * running yet but will be (a game launcher, a full-screen client).
 */
export function ExclusionsPage({
  settings,
  runningApps,
  busy,
  onSave,
}: {
  settings: SettingsDto | null;
  runningApps: AppDto[];
  busy: boolean;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  const [draft, setDraft] = useState("");

  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  const rules = settings.excluded_apps;

  const setRules = (next: string[]) => onSave({ excluded_apps: next });

  const addRule = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    if (rules.some((rule) => rule.toLowerCase() === trimmed.toLowerCase())) {
      return;
    }
    setRules([...rules, trimmed]);
    setDraft("");
  };

  return (
    <>
      <Card
        title="排除的应用"
        description="这些应用的窗口不会在整理时被移动，也不会在恢复时被处理。"
        aside={<span className="text-[11px] tabular-nums text-[var(--color-muted)]">{rules.length} 条规则</span>}
      >
        {rules.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">
            还没有排除任何应用。
          </p>
        ) : (
          <ul className="space-y-1">
            {rules.map((rule) => (
              <li
                key={rule}
                className="flex items-center justify-between gap-2 rounded-[var(--radius-control)] border border-[var(--color-border)] px-2.5 py-1.5"
              >
                <span className="min-w-0 truncate font-mono text-xs">{rule}</span>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setRules(rules.filter((r) => r !== rule))}
                  className="shrink-0 text-[11px] text-[var(--color-muted)] transition-colors hover:text-[var(--color-danger)] disabled:opacity-40"
                >
                  移除
                </button>
              </li>
            ))}
          </ul>
        )}

        <form
          className="mt-2.5 flex gap-1.5"
          onSubmit={(event) => {
            event.preventDefault();
            addRule(draft);
          }}
        >
          <input
            value={draft}
            onChange={(event) => setDraft(event.currentTarget.value)}
            placeholder="例如 game.exe"
            spellCheck={false}
            disabled={busy}
            className="h-7 min-w-0 flex-1 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 font-mono text-xs outline-none placeholder:font-sans placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)]"
          />
          <Button size="sm" type="submit" disabled={busy || draft.trim() === ""}>
            添加
          </Button>
        </form>
        <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--color-muted)]">
          匹配可执行文件名，不区分大小写。全屏应用与工具窗口本来就不会被移动。
        </p>
      </Card>

      <Card
        title="正在运行"
        description="从当前有窗口的应用中选择，避免手写文件名出错。"
        aside={
          <Button
            size="sm"
            variant="quiet"
            disabled={busy}
            onClick={() => onSave({ excluded_apps: [] })}
          >
            清空规则
          </Button>
        }
      >
        {runningApps.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">没有检测到有窗口的应用。</p>
        ) : (
          <ul>
            {runningApps.map((app, index) => (
              <li
                key={app.name}
                className={`flex items-center justify-between gap-3 py-1.5 ${
                  index > 0 ? "border-t border-[var(--color-border)]" : ""
                }`}
              >
                <span className="min-w-0 truncate font-mono text-xs">{app.name}</span>
                <Switch
                  checked={app.excluded}
                  disabled={busy}
                  label={`排除 ${app.name}`}
                  onChange={(excluded) =>
                    setRules(
                      excluded
                        ? [...rules, app.name]
                        : rules.filter(
                            (r) => r.toLowerCase() !== app.name.toLowerCase(),
                          ),
                    )
                  }
                />
              </li>
            ))}
          </ul>
        )}
      </Card>
    </>
  );
}
