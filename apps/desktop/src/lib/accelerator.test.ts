/**
 * Accelerator conversion tests.
 *
 * Run with Bun: `bun test src/lib/accelerator.test.ts`.
 *
 * These matter because the recorder replaced a text field. A text field that
 * produces a syntactically invalid accelerator is visibly wrong (the user typed
 * something odd); a recorder that produces `Ctrl+` for a modifier-only press, or
 * `Super` where Tauri expects `CmdOrCtrl`, looks correct and silently fails to
 * register.
 */
import { describe, expect, test } from "bun:test";

import { toAccelerator, type KeyLike } from "./accelerator";

function key(partial: Partial<KeyLike> & { key: string }): KeyLike {
  return {
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...partial,
  };
}

describe("toAccelerator", () => {
  test("maps ctrl to CmdOrCtrl so one binding works on both platforms", () => {
    expect(toAccelerator(key({ key: "g", ctrlKey: true }))).toBe("CmdOrCtrl+G");
  });

  test("orders modifiers the way Tauri's parser expects", () => {
    expect(
      toAccelerator(key({ key: "g", ctrlKey: true, altKey: true, shiftKey: true })),
    ).toBe("CmdOrCtrl+Alt+Shift+G");
  });

  test("ignores a modifier pressed on its own", () => {
    // The recorder keeps waiting; returning "Ctrl" would bind the Control key
    // itself and break the machine's keyboard.
    expect(toAccelerator(key({ key: "Control", ctrlKey: true }))).toBeNull();
    expect(toAccelerator(key({ key: "Shift", shiftKey: true }))).toBeNull();
    expect(toAccelerator(key({ key: "Meta", metaKey: true }))).toBeNull();
  });

  test("rejects a bare letter", () => {
    // A global single-key shortcut steals that key from every application.
    expect(toAccelerator(key({ key: "g" }))).toBeNull();
  });

  test("maps named keys to Tauri names", () => {
    expect(toAccelerator(key({ key: "ArrowUp", ctrlKey: true }))).toBe("CmdOrCtrl+Up");
    expect(toAccelerator(key({ key: " ", ctrlKey: true }))).toBe("CmdOrCtrl+Space");
    expect(toAccelerator(key({ key: "Escape", altKey: true }))).toBe("Alt+Esc");
    expect(toAccelerator(key({ key: "PageDown", ctrlKey: true }))).toBe("CmdOrCtrl+PageDown");
  });

  test("keeps function keys", () => {
    expect(toAccelerator(key({ key: "F5", ctrlKey: true }))).toBe("CmdOrCtrl+F5");
    expect(toAccelerator(key({ key: "F12" }))).toBeNull();
  });

  test("maps the Windows key to Super", () => {
    expect(toAccelerator(key({ key: "d", metaKey: true }))).toBe("Super+D");
  });

  test("uppercases letters regardless of shift", () => {
    // Shift+G reports key "G"; without shift it reports "g". Both must produce
    // the same accelerator, or the same binding becomes two different rules.
    expect(toAccelerator(key({ key: "G", ctrlKey: true, shiftKey: true }))).toBe(
      "CmdOrCtrl+Shift+G",
    );
    expect(toAccelerator(key({ key: "g", ctrlKey: true }))).toBe("CmdOrCtrl+G");
  });

  test("rejects keys with no faithful accelerator spelling", () => {
    expect(toAccelerator(key({ key: "Unidentified", ctrlKey: true }))).toBeNull();
  });
});
