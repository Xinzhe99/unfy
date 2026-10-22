import { describe, expect, test } from "bun:test";

import { targetState } from "./target";
import type { DisplayDto, TargetDto } from "../api";

const display = (overrides: Partial<DisplayDto> & { native_id: string }): DisplayDto => ({
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
});

const target = (overrides: Partial<TargetDto> = {}): TargetDto => ({
  native_id: "A",
  label: "DISPLAY1",
  present: true,
  number: 1,
  ...overrides,
});

describe("target state", () => {
  test("no saved target is not a fault", () => {
    expect(targetState(null, [])).toEqual({ saved: false, gone: false, weak: false });
    expect(targetState(undefined, [display({ native_id: "A" })])).toEqual({
      saved: false,
      gone: false,
      weak: false,
    });
  });

  test("an attached, well-identified target is simply fine", () => {
    expect(targetState(target(), [display({ native_id: "A" })])).toEqual({
      saved: true,
      gone: false,
      weak: false,
    });
  });

  test("a target that is no longer attached reports gone", () => {
    // The overview warned about this and the displays page did not, which is why the
    // rule now lives in one place.
    const state = targetState(target({ present: false }), [display({ native_id: "B" })]);
    expect(state.gone).toBe(true);
  });

  test("an attached target the platform only knows by resolution is weak", () => {
    const state = targetState(target(), [display({ native_id: "A", restorable: false })]);
    expect(state.weak).toBe(true);
    expect(state.gone).toBe(false);
  });

  test("a target that is gone is never also weak", () => {
    // There is nothing to recognise when the display is not there; reporting both would
    // stack two warnings that mean the same thing.
    const state = targetState(target({ present: false }), []);
    expect(state.gone).toBe(true);
    expect(state.weak).toBe(false);
  });

  test("present but missing from the list is not weak", () => {
    // `present` comes from the backend and the list from a separate call; a display that
    // is present but absent here is a refresh race, not a weak identity.
    const state = targetState(target(), []);
    expect(state.weak).toBe(false);
  });
});
