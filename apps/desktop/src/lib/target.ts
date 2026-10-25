/**
 * What the saved target display is doing right now.
 *
 * Two pages need this answer and they had each written their own test for it: the
 * overview warned when the target was gone, the displays page did not, and for a while
 * a user could open the displays page to check "which screen am I pointing at" and be
 * told nothing. The presentation differs on purpose - the overview has no list of
 * screens, so it offers a button; the displays page has the list right there, so a
 * sentence pointing at it is enough - but the condition must not be written twice.
 */
import type { DisplayDto, TargetDto } from "../api";

export interface TargetState {
  /** A target is saved. */
  saved: boolean;
  /** The saved target is no longer attached. */
  gone: boolean;
  /**
   * The saved target is attached but its identity is weak - the platform could only
   * recognise it by resolution, so a restore lands it in the visible area rather than
   * on its exact rectangle.
   */
  weak: boolean;
}

export function targetState(
  target: TargetDto | null | undefined,
  displays: DisplayDto[],
): TargetState {
  if (!target) return { saved: false, gone: false, weak: false };
  const live = displays.find((d) => d.native_id === target.native_id);
  return {
    saved: true,
    gone: !target.present,
    // A display that is gone cannot also be "weak"; there is nothing to recognise.
    weak: target.present && (!target.confirmed || (live !== undefined && !live.restorable)),
  };
}
