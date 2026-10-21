/**
 * Display naming.
 *
 * Run with `bun test`.
 *
 * This exists because the overview and the picker named the same monitor
 * differently: the picker sorted by physical position, the overview derived its
 * number from its own array index. Both now read `DisplayDto::number`, which the
 * backend assigns once, in the same order as the badges drawn on the screens.
 *
 * The test passes an array whose order deliberately differs from the numbering,
 * which is what the old code would have got wrong.
 */
import { describe, expect, test } from "bun:test";

import { targetDisplayName } from "./Overview";
import type { DisplayDto } from "../api";

function display(overrides: Partial<DisplayDto> & { native_id: string }): DisplayDto {
  return {
    label: overrides.native_id,
    width: 1920,
    height: 1080,
    origin_x: 0,
    origin_y: 0,
    work_width: 1920,
    work_height: 1040,
    scale: 1,
    primary: false,
    restorable: true,
    is_target: false,
    number: 1,
    ...overrides,
  };
}

describe("targetDisplayName", () => {
  test("uses the backend number, not the array position", () => {
    // Array order (right monitor first) contradicts the numbering, exactly as a
    // platform enumeration order can.
    const displays = [
      display({ native_id: "right", number: 2 }),
      display({ native_id: "left", origin_x: -1920, number: 1, primary: true }),
    ];
    expect(targetDisplayName(displays, { native_id: "left", present: true })).toBe(
      "显示器 1（主）",
    );
    expect(targetDisplayName(displays, { native_id: "right", present: true })).toBe(
      "显示器 2",
    );
  });

  test("names the same display identically regardless of list order", () => {
    const target = { native_id: "primary", present: true };
    const first = [
      display({ native_id: "primary", number: 1, primary: true }),
      display({ native_id: "secondary", number: 2 }),
    ];
    const second = [first[1]!, first[0]!];
    expect(targetDisplayName(first, target)).toBe(targetDisplayName(second, target));
  });

  test("falls back when there is no target or it is gone", () => {
    expect(targetDisplayName([], null)).toBe("跟随主显示器");
    expect(
      targetDisplayName([display({ native_id: "a" })], { native_id: "gone", present: false }),
    ).toBe("不可用");
    expect(
      targetDisplayName([display({ native_id: "a" })], { native_id: "gone", present: true }),
    ).toBe("已选择");
  });
});
