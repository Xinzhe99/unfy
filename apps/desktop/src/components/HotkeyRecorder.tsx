import { t } from "../i18n";
import { useCallback, useEffect, useRef, useState } from "react";

import { toAccelerator } from "../lib/accelerator";

/**
 * Global-shortcut recorder.
 *
 * The previous implementation was a text field, which meant the user had to type
 * Tauri's accelerator syntax by hand (`CmdOrCtrl+Alt+G`). Nobody guesses that
 * syntax, a typo fails silently, and the only feedback was that the shortcut did
 * not work. This is a button: click it, press the combination, done.
 *
 * # What it captures
 *
 * Only combinations with at least one modifier. A bare letter would swallow that
 * key system-wide while the app runs, which is a hostile default and hard to
 * diagnose once it happens.
 *
 * Escape cancels, Backspace clears. Both are the conventions in every other
 * shortcut editor.
 */
export function HotkeyRecorder({
  value,
  disabled,
  invalid,
  onChange,
}: {
  /** Accelerator in Tauri syntax, or null when unset. */
  value: string | null;
  disabled?: boolean;
  /** The bound combination failed to register; shown in place of the text. */
  invalid?: boolean;
  onChange: (value: string | null) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement | null>(null);

  const stop = useCallback(() => {
    setRecording(false);
    setError(null);
  }, []);

  useEffect(() => {
    if (!recording) return;

    const onKeyDown = (event: KeyboardEvent) => {
      // The browser's own shortcuts must not fire while recording: pressing
      // Ctrl+R to bind it would otherwise reload the window.
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        stop();
        buttonRef.current?.focus();
        return;
      }

      if (event.key === "Backspace" || event.key === "Delete") {
        onChange(null);
        stop();
        return;
      }

      const accelerator = toAccelerator(event);
      if (!accelerator) {
        // Modifier-only presses are not a shortcut; keep waiting. Saying so
        // avoids the impression that the keystroke was ignored.
        setError(t("还需要一个非修饰键"));
        return;
      }
      onChange(accelerator);
      stop();
      buttonRef.current?.focus();
    };

    const onKeyUp = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
    };

    // Leaving the window ends the recording. The capture listener swallows every
    // keystroke while it is attached, so a user who clicked record, switched away,
    // and came back would find Ctrl+R and Ctrl+W doing nothing - indistinguishable
    // from a frozen app - with only a border colour to explain why.
    const onBlur = () => stop();

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [recording, onChange, stop]);

  const label = recording
    ? (error ?? t("请按下组合键…"))
    : value
      ? formatAccelerator(value)
      : t("未设置");

  return (
    <div className="flex items-center gap-1.5">
      <button
        ref={buttonRef}
        type="button"
        disabled={disabled}
        onClick={() => {
          setRecording(true);
          setError(null);
        }}
        aria-label={
          value
            ? t("快捷键 {keys}，点击重新录制", { keys: formatAccelerator(value) })
            : t("录制快捷键")
        }
        className={`h-7 w-40 rounded-[var(--radius-control)] border px-2 font-mono text-xs tabular-nums transition-colors disabled:opacity-40 ${
          recording
            ? "border-[var(--color-accent)] bg-[var(--color-surface-hover)] text-[var(--color-fg)]"
            : invalid
              ? "border-[var(--color-warning)] text-[var(--color-fg-soft)]"
              : "border-[var(--color-border-strong)] text-[var(--color-fg-soft)] hover:bg-[var(--color-surface-hover)]"
        }`}
      >
        {label}
      </button>
      {value ? (
        <button
          type="button"
          disabled={disabled}
          onClick={() => onChange(null)}
          className="text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-40"
        >
          {t("清除")}
        </button>
      ) : null}
    </div>
  );
}

/** Human form for display: platform-appropriate modifier glyphs. */
export function formatAccelerator(accelerator: string): string {
  const mac = /Mac|iPhone|iPad/.test(navigator.platform);
  return accelerator
    .split("+")
    .map((part) => {
      switch (part) {
        case "CmdOrCtrl":
        case "CommandOrControl":
          return mac ? "⌘" : "Ctrl";
        case "Super":
          return mac ? "⌘" : "Win";
        case "Alt":
          return mac ? "⌥" : "Alt";
        case "Shift":
          return mac ? "⇧" : "Shift";
        default:
          return part;
      }
    })
    .join(mac ? "" : " + ");
}
