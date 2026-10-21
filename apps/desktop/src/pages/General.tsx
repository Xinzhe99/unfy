import { useState } from "react";

import type { SettingsDto } from "../api";

/**
 * General page: startup, theme, hotkeys, restore delay.
 *
 * Hotkey capture is a plain text field with a recorder. The recorder listens for
 * a key combination and writes it in Tauri's accelerator syntax, which is what
 * the Rust shortcut plugin parses. Validation happens in Rust, so a bad string
 * comes back as an error banner rather than a silent failure.
 */
export function GeneralPage({
  settings,
  busy,
  onSave,
}: {
  settings: SettingsDto | null;
  busy: boolean;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  return (
    <div className="space-y-4">
      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">启动</h2>
        <label className="mt-3 flex items-start gap-3">
          <input
            type="checkbox"
            checked={settings.autostart}
            disabled={busy}
            onChange={(event) => onSave({ autostart: event.currentTarget.checked })}
            className="mt-0.5 size-4 accent-[var(--color-accent)]"
          />
          <span>
            <span className="block text-xs font-medium">登录时启动</span>
            <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
              启动后只驻留托盘，不打开设置窗口。
            </span>
          </span>
        </label>
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">快捷键</h2>
        <p className="mt-1 text-xs text-[var(--color-muted)]">
          留空表示不注册。注册失败时会保留托盘入口，不会静默失效。
        </p>
        <div className="mt-3 space-y-3">
          <HotkeyField
            label="整理到目标屏"
            value={settings.hotkey_gather}
            disabled={busy}
            onCommit={(value) => onSave({ hotkey_gather: value })}
          />
          <HotkeyField
            label="恢复本次布局"
            value={settings.hotkey_restore}
            disabled={busy}
            onCommit={(value) => onSave({ hotkey_restore: value })}
          />
        </div>
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">外观</h2>
        <div className="mt-3 flex gap-2" role="radiogroup" aria-label="主题">
          {(["system", "light", "dark"] as const).map((theme) => (
            <button
              key={theme}
              type="button"
              role="radio"
              aria-checked={settings.theme === theme}
              disabled={busy}
              onClick={() => onSave({ theme })}
              className={`rounded-lg border px-3 py-1.5 text-xs font-medium ${
                settings.theme === theme
                  ? "border-[var(--color-accent)] bg-[var(--color-accent-soft)]"
                  : "border-[var(--color-border)]"
              }`}
            >
              {theme === "system" ? "跟随系统" : theme === "light" ? "浅色" : "深色"}
            </button>
          ))}
        </div>
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">更新</h2>
        <label className="mt-3 flex items-start gap-3">
          <input
            type="checkbox"
            checked={settings.auto_update}
            disabled={busy}
            onChange={(event) => onSave({ auto_update: event.currentTarget.checked })}
            className="mt-0.5 size-4 accent-[var(--color-accent)]"
          />
          <span>
            <span className="block text-xs font-medium">自动检查更新</span>
            <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
              只在窗口全部恢复后安装；有未完成的恢复记录时会延后。
            </span>
          </span>
        </label>
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">隐私</h2>
        <label className="mt-3 flex items-start gap-3">
          <input
            type="checkbox"
            checked={settings.telemetry}
            disabled={busy}
            onChange={(event) => onSave({ telemetry: event.currentTarget.checked })}
            className="mt-0.5 size-4 accent-[var(--color-accent)]"
          />
          <span>
            <span className="block text-xs font-medium">发送匿名使用统计</span>
            <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
              默认关闭。只包含版本、平台与成功/失败计数，不含窗口标题、屏幕画面或文件路径。拒绝不影响任何功能。
            </span>
          </span>
        </label>
      </section>

      <section className="rounded-xl border border-[var(--color-border)] p-5">
        <h2 className="text-sm font-semibold">恢复</h2>
        <label className="mt-3 block">
          <span className="text-xs font-medium">恢复延迟（秒）</span>
          <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
            从点击恢复到开始移动窗口之间的等待，0 表示立即执行。
          </span>
          <input
            type="number"
            defaultValue={settings.restore_delay_secs}
            min={0}
            max={300}
            disabled={busy}
            onBlur={(event) => {
              const next = Number(event.currentTarget.value);
              if (Number.isFinite(next) && next !== settings.restore_delay_secs) {
                onSave({
                  restore_delay_secs: Math.min(300, Math.max(0, Math.round(next))),
                });
              }
            }}
            className="mt-1.5 w-28 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-xs"
          />
        </label>
      </section>
    </div>
  );
}

function HotkeyField({
  label,
  value,
  disabled,
  onCommit,
}: {
  label: string;
  value: string | null;
  disabled: boolean;
  onCommit: (value: string | null) => void;
}) {
  const [recording, setRecording] = useState(false);

  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-xs font-medium">{label}</span>
      <div className="flex items-center gap-2">
        <button
          type="button"
          disabled={disabled}
          onClick={() => setRecording((v) => !v)}
          onKeyDown={(event) => {
            if (!recording) return;
            const combo = comboFromEvent(event);
            if (combo === null) return;
            event.preventDefault();
            setRecording(false);
            onCommit(combo);
          }}
          className={`min-w-44 rounded-lg border px-3 py-1.5 text-xs ${
            recording
              ? "border-[var(--color-accent)] bg-[var(--color-accent-soft)]"
              : "border-[var(--color-border)]"
          }`}
        >
          {recording ? "请按下组合键…" : (value ?? "未设置")}
        </button>
        <button
          type="button"
          disabled={disabled || value === null}
          onClick={() => onCommit(null)}
          className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-40"
        >
          清除
        </button>
      </div>
    </div>
  );
}

/**
 * Convert a keyboard event into a Tauri accelerator.
 *
 * Returns `null` while the user has only pressed modifiers, so pressing Ctrl and
 * then Shift does not immediately register "Ctrl+Shift".
 */
function comboFromEvent(event: React.KeyboardEvent): string | null {
  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push("Control");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");

  const key = event.key;
  if (["Control", "Alt", "Shift", "Meta"].includes(key)) return null;

  const normalized = normalizeKey(key);
  if (!normalized) return null;
  return [...modifiers, normalized].join("+");
}

function normalizeKey(key: string): string | null {
  if (key.length === 1) {
    const upper = key.toUpperCase();
    if (/^[A-Z]$/.test(upper) || /^[0-9]$/.test(upper)) return upper;
    if (key === " ") return "Space";
    return null;
  }
  const named: Record<string, string> = {
    Escape: "Escape",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    Delete: "Delete",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
  };
  if (key in named) return named[key] ?? null;
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return key;
  return null;
}
