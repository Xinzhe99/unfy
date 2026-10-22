/**
 * Committing a number typed into an uncontrolled field.
 *
 * Both numeric settings - the margin and gap on the displays page, the restore
 * delay on the settings page - are plain `<input type="number">` with a
 * `defaultValue`. That means the DOM keeps whatever the user typed, no matter what
 * the commit handler decides to store, so the handler has to hand the accepted
 * value back.
 *
 * That is not a hypothetical: typing 999 into the margin stored 200 and left "999"
 * on screen until the page was reloaded, and the restore delay had the same shape.
 * The rule lives here so a third numeric field cannot quietly invent its own
 * version.
 */

/** What to store, and what the field must show, for a typed value. */
export interface NumericCommit {
  /** The value to save. */
  value: number;
  /** What the input element must display afterwards. */
  display: string;
  /** Whether this differs from what is currently stored. */
  changed: boolean;
}

/**
 * Clamp `raw` to `[min, max]` and report what to store and display.
 *
 * `current` is used when the field cannot be read as a number at all - an empty
 * box, or the empty string a browser reports for input that fails its own `min`
 * check. Leaving the field blank next to a real setting is the confusion this
 * avoids.
 */
export function commitNumber(
  raw: string,
  current: number,
  min: number,
  max: number,
): NumericCommit {
  const typed = Number(raw);
  // `Number("")` is 0 and `Number(" ")` is 0, but an empty box means "I did not
  // type anything", not "set it to zero".
  const usable = raw.trim() !== "" && Number.isFinite(typed);
  const clamped = usable
    ? Math.min(max, Math.max(min, Math.round(typed)))
    : current;
  return {
    value: clamped,
    display: String(clamped),
    changed: clamped !== current,
  };
}
