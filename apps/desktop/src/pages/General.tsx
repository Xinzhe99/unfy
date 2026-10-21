import { useState } from "react";

import type { HotkeyStatusDto, SettingsDto } from "../api";
import { Button, Card, Segmented, Toggle } from "../components/ui";

/**
 * General page: startup, hotkeys, appearance, restore behaviour, privacy.
 *
 * Hotkey capture is a recorder, not a text field: users cannot be expected to
 * type Tauri's accelerator syntax correctly. Registration happens in Rust and its
 * result is shown here, because a shortcut that silently failed to register is
 * worse than no shortcut at all — the user presses it and nothing happens.
 */
export function GeneralPage({
  settings,
  hotkeys,
  busy,
  onSave,
}: {
  settings: SettingsDto | null;
  hotkeys: HotkeyStatusDto | null;
  busy: boolean;
  onSave: (patch: Partial<SettingsDto>) => void;
}) {
  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  return (
    <div className="flex flex-col gap-4">
      <Card title="启动">
        <div className="divide-y divide-[var(--color-border)]">
          <Toggle
            label="登录时启动"
            hint="启动后只驻留托盘，不打开设置窗口。"
            checked={settings.autostart}
            disabled={busy}
            onChange={(autostart) => onSave({ autostart })}
          />
        </div>
      </Card>

      <Card
        title="快捷键"
        description="留空表示不注册。注册失败时下方会显示原因，托盘入口始终可用。"
      >
        <div className="divide-y divide-[var(--color-border)]">
          <HotkeyRow
            label="整理到目标屏"
            value={settings.hotkey_gather}
            error={hotkeys?.gather_error ?? null}
            disabled={busy}
            onCommit={(value) => onSave({ hotkey_gather: value })}
          />
          <HotkeyRow
            label="恢复本次布局"
            value={settings.hotkey_restore}
            error={hotkeys?.restore_error ?? null}
            disabled={busy}
            onCommit={(value) => onSave({ hotkey_restore: value })}
          />
        </div>
      </Card>

      <Card title="外观">
        <Segmented
          label="主题"
          value={settings.theme}
          disabled={busy}
          onChange={(theme) => onSave({ theme })}
          options={[
            { value: "system", label: "跟随系统" },
            { value: "light", label: "浅色" },
            { value: "dark", label: "深色" },
          ]}
        />
      </Card>

      <Card
        title="恢复"
        description="远程连接常常断几秒又重连。延迟可以避免两次完整的窗口移动。"
      >
        <label className="flex items-center justify-between gap-6 py-1">
          <span>
            <span className="block text-[13px] font-medium">恢复延迟</span>
            <span className="mt-0.5 block text-xs text-[var(--color-muted)]">
              从点击恢复到开始移动窗口之间的等待，0 表示立即执行。
            </span>
          </span>
          <div className="flex items-center gap-1.5">
            <input
              type="number"
              defaultValue={settings.restore_delay_secs}
              min={0}
              max={300}
              disabled={busy}
              onBlur={(event) => {
                const next = Number(event.currentTarget.value);
                if (
                  Number.isFinite(next) &&
                  next !== settings.restore_delay_secs
                ) {
                  onSave({
                    restore_delay_secs: Math.min(300, Math.max(0, Math.round(next))),
                  });
                }
              }}
              className="h-9 w-20 rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 text-[13px] tabular-nums outline-none focus:border-[var(--color-accent)]"
            />
            <span className="text-xs text-[var(--color-muted)]">秒</span>
          </div>
        </label>
      </Card>

      <Card title="更新与隐私">
        <div className="divide-y divide-[var(--color-border)]">
          <Toggle
            label="自动检查更新"
            hint="只在窗口全部恢复后安装；有未完成的恢复记录时会延后。"
            checked={settings.auto_update}
            disabled={busy}
            onChange={(auto_update) => onSave({ auto_update })}
          />
          <Toggle
            label="发送匿名使用统计"
            hint="默认关闭。只包含版本、平台与成功/失败计数，不含窗口标题、屏幕画面或文件路径。当前版本没有任何上传代码，此项仅记录你的选择。"
            checked={settings.telemetry}
            disabled={busy}
            onChange={(telemetry) => onSave({ telemetry })}
          />
        </div>
      </Card>
    </div>
  );
}

function HotkeyRow({
  label,
  value,
  error,
  disabled,
  onCommit,
}: {
  label: string;
  value: string | null;
  error: string | null;
  disabled: boolean;
  onCommit: (value: string | null) => void;
}) {
  const [recording, setRecording] = useState(false);

  return (
    <div className="flex items-start justify-between gap-6 py-3">
      <div className="min-w-0">
        <span className="block text-[13px] font-medium">{label}</span>
        {error ? (
          <span className="mt-1 block text-xs text-[var(--color-warning)]">
            未能注册：{error}
          </span>
        ) : value ? (
          <span className="mt-1 block text-xs text-[var(--color-success)]">
            已注册
          </span>
        ) : (
          <span className="mt-1 block text-xs text-[var(--color-muted)]">
            未设置
          </span>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
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
          onBlur={() => setRecording(false)}
          className={`h-9 min-w-40 rounded-[var(--radius-control)] border px-3 text-xs font-medium tabular-nums transition-colors ${
            recording
              ? "border-[var(--color-accent)] bg-[var(--color-accent-soft)] text-[var(--color-accent)]"
              : "border-[var(--color-border)] bg-[var(--color-surface)]"
          }`}
        >
          {recording ? "请按下组合键…" : (value ?? "点击录制")}
        </button>
        <Button
          size="sm"
          tone="quiet"
          disabled={disabled || value === null}
          onClick={() => onCommit(null)}
        >
          清除
        </Button>
      </div>
    </div>
  );
}

/**
 * Convert a keyboard event into a Tauri accelerator.
 *
 * Returns `null` while only modifiers are held, so pressing Ctrl then Shift does
 * not immediately register "Control+Shift".
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
