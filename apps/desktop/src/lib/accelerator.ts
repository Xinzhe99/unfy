/**
 * Keyboard-event to accelerator conversion.
 *
 * Kept out of the React component so it can be tested directly: the browser
 * cannot be driven in this project's verification environment, and a recorder
 * whose converter is wrong is worse than a text field, because it looks correct.
 */

/** The parts of a `KeyboardEvent` this module needs. */
export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** Tauri accelerator syntax for a key event, or null for modifiers alone. */
export function toAccelerator(event: KeyLike): string | null {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("CmdOrCtrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");

  const key = event.key;
  // A modifier on its own: hold and wait for the real key.
  if (["Control", "Alt", "Shift", "Meta", "OS"].includes(key)) return null;

  const named: Record<string, string> = {
    " ": "Space",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Escape: "Esc",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    Delete: "Delete",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
  };

  let name = named[key];
  if (!name) {
    if (key.length === 1) {
      name = key.toUpperCase();
    } else if (/^F\d{1,2}$/.test(key)) {
      name = key;
    } else {
      return null;
    }
  }

  if (parts.length === 0) return null;
  return [...parts, name].join("+");
}

