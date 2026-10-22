import { describe, expect, test } from "bun:test";

import { commitNumber } from "./numeric";

describe("numeric settings fields", () => {
  test("an out-of-range entry stores and shows the bound", () => {
    // The bug this exists for: 999 was stored as 200 while the box kept reading
    // 999 until the page reloaded.
    expect(commitNumber("999", 12, 0, 200)).toEqual({
      value: 200,
      display: "200",
      changed: true,
    });
  });

  test("a negative entry stores and shows the lower bound", () => {
    expect(commitNumber("-50", 12, 0, 200)).toEqual({
      value: 0,
      display: "0",
      changed: true,
    });
  });

  test("an empty field restores the current value instead of zero", () => {
    // `Number("")` is 0, which would silently set a delay of 15 to 0 - a real
    // change the user never asked for.
    expect(commitNumber("", 15, 0, 120)).toEqual({
      value: 15,
      display: "15",
      changed: false,
    });
    expect(commitNumber("   ", 15, 0, 120).value).toBe(15);
  });

  test("garbage restores the current value", () => {
    expect(commitNumber("abc", 15, 0, 120)).toEqual({
      value: 15,
      display: "15",
      changed: false,
    });
  });

  test("fractions round to a whole number", () => {
    expect(commitNumber("12.6", 12, 0, 200).value).toBe(13);
    expect(commitNumber("12.4", 12, 0, 200).value).toBe(12);
  });

  test("an unchanged value reports no change but still displays it", () => {
    const commit = commitNumber("12", 12, 0, 200);
    expect(commit.changed).toBe(false);
    expect(commit.display).toBe("12");
  });
});
