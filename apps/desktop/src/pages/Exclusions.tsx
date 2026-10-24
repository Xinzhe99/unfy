import { t } from "../i18n";
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
  onNotice,
}: {
  settings: SettingsDto | null;
  runningApps: AppDto[];
  busy: boolean;
  onSave: (patch: Partial<SettingsDto>) => void;
  /** Report something the form cannot show inline. */
  onNotice: (text: string) => void;
}) {
  const [draft, setDraft] = useState("");
  // Arming state for "clear every rule", which is destructive and irreversible.
  const [clearArmed, setClearArmed] = useState(false);

  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">{t("正在读取设置…")}</p>;
  }

  const rules = settings.excluded_apps;

  const setRules = (next: string[]) => onSave({ excluded_apps: next });

  const addRule = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    // A duplicate used to return in silence: the box kept its text, the rule count
    // did not move, and nothing explained why. Pressing Add and seeing nothing
    // happen reads as a broken button. Say it was already there, and clear the box
    // so the next attempt starts empty.
    if (rules.some((rule) => rule.toLowerCase() === trimmed.toLowerCase())) {
      onNotice(t("这条规则已经存在。"));
      setDraft("");
      return;
    }
    setRules([...rules, trimmed]);
    setDraft("");
  };

  return (
    <>
      <Card
        title={t("排除的应用")}
        description={t("以后整理时跳过这些应用；已经整理的窗口仍可恢复。")}
        aside={
          <div className="flex items-center gap-2">
            <span className="text-[11px] tabular-nums text-[var(--color-muted)]">
              {t("{count} 条规则", { count: rules.length })}
            </span>
            {/* It clears this card's rules, so it belongs to this card - it sat on
                the "running" card, where it read as "clear the list below". Hidden
                rather than disabled when there is nothing to clear: a control that
                is present, styled like an action and does nothing is worse than no
                control. */}
            {rules.length > 0 ? (
              clearArmed ? (
                <div className="flex shrink-0 gap-1.5">
                  <Button
                    size="sm"
                    variant="danger"
                    disabled={busy}
                    onClick={() => {
                      setClearArmed(false);
                      onSave({ excluded_apps: [] });
                    }}
                  >{t("确认清空")}</Button>
                  <Button
                    size="sm"
                    variant="quiet"
                    disabled={busy}
                    onClick={() => setClearArmed(false)}
                  >{t("取消")}</Button>
                </div>
              ) : (
                // Two-step, like the reset row and the history row. Clearing wipes
                // every rule at once and cannot be undone, so a single click is out
                // of line with the app's own standard for destructive actions.
                <Button
                  size="sm"
                  variant="quiet"
                  disabled={busy}
                  onClick={() => setClearArmed(true)}
                >{t("清空规则")}</Button>
              )
            ) : null}
          </div>
        }
      >
        {rules.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">{t("还没有排除任何应用。")}</p>
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
                  className="shrink-0 text-[11px] text-[var(--color-muted)] transition-colors hover:text-[var(--color-danger)]"
                >{t("移除")}</button>
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
            placeholder={t("例如 game.exe")}
            spellCheck={false}
            disabled={busy}
            className="h-7 min-w-0 flex-1 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 font-mono text-xs outline-none placeholder:font-sans placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)]"
          />
          <Button size="sm" type="submit" disabled={busy || draft.trim() === ""}>{t("添加")}</Button>
        </form>
        <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--color-muted)]">{t("匹配可执行文件名，不区分大小写。全屏应用与工具窗口本来就不会被移动。")}</p>
      </Card>

      <Card
        title={t("正在运行")}
        description={t("从当前有窗口的应用中选择，避免手写文件名出错。")}
      >
        {runningApps.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">{t("没有检测到有窗口的应用。应用打开后回到这一页会自动刷新。")}</p>
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
                  label={t("排除 {name}", { name: app.name })}
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
