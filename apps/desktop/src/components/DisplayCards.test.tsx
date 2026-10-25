/**
 * Display card layout.
 *
 * Run with `bun test`.
 *
 * The numbering rule is load-bearing: the badge flashed on each physical screen
 * uses the same 1-based index, so if the cards and the badges disagree the feature
 * that exists to identify a display points at the wrong one. The platform
 * enumerates in adapter order, which for a monitor placed left of the primary
 * counts right-to-left; the UI sorts by position instead.
 */
import { describe, expect, test } from "bun:test";

import { DisplayCards } from "./DisplayCards";
import type { DisplayDto } from "../api";
import { renderToStaticMarkup } from "react-dom/server";

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

function render(
  displays: DisplayDto[],
  thumbnails: Record<string, { native_id: string; data_url: string; blank: boolean }> = {},
): string {
  return renderToStaticMarkup(
    <DisplayCards
      displays={displays}
      target={null}
      thumbnails={thumbnails}
      onSelect={() => {}}
      disabled={false}
    />,
  );
}

describe("display cards", () => {
  test("shows the number the backend assigned, not the array position", () => {
    // The backend numbers by physical position and the on-screen badge uses the
    // same value. The component must render that number: deriving it from the
    // array index is what made the overview and the picker name the same monitor
    // differently.
    const html = render([
      display({ native_id: "left", origin_x: -1920, number: 1 }),
      display({ native_id: "primary", origin_x: 0, primary: true, number: 2 }),
    ]);

    expect(html).toContain(">1<");
  });

  test("stacks a vertically arranged pair instead of laying it out side by side", () => {
    const html = render([
      display({ native_id: "top", origin_y: 0, number: 1 }),
      display({ native_id: "bottom", origin_y: 1080, number: 2 }),
    ]);
    // One column, two rows: the second card is on grid row 2 and there is no second
    // column for it to sit in.
    expect(html).toMatch(/grid-column:\s*1/);
    expect(html).toMatch(/grid-row:\s*2/);
    expect(html).not.toMatch(/grid-column:\s*2/);
  });

  test("a side-by-side pair occupies one row in two columns", () => {
    // The shape the old absolute layout got wrong: two 16:9 monitors in one box were
    // each squeezed to a quarter of the height.
    const html = render([
      display({ native_id: "left", number: 1 }),
      display({ native_id: "right", origin_x: 1920, number: 2 }),
    ]);
    expect(html).toMatch(/grid-column:\s*2/);
    expect(html).not.toMatch(/grid-row:\s*2/);
  });

  test("a left/right pair with mismatched y origins is still one row", () => {
    // The desktop this was written on: the right-hand monitor is the primary at
    // y=0, the left-hand one sits at y=-78 because its taskbar is elsewhere. Keying
    // rows on exact origins put them on different rows and drew a diagonal.
    const html = render([
      display({ native_id: "left", origin_x: -1920, origin_y: -78, number: 1 }),
      display({ native_id: "right", origin_x: 0, origin_y: 0, primary: true, number: 2 }),
    ]);
    expect(html).toMatch(/grid-column:\s*1/);
    expect(html).toMatch(/grid-column:\s*2/);
    expect(html).not.toMatch(/grid-row:\s*2/);
  });

  test("keeps a single display at full width", () => {
    const html = render([display({ native_id: "only", primary: true, number: 1 })]);
    expect(html).toMatch(/grid-column:\s*1/);
  });


  test("a captured display shows its picture", () => {
    const html = render([display({ native_id: "A" })], {
      A: { native_id: "A", data_url: "data:image/png;base64,AAAA", blank: false },
    });
    expect(html).toContain("<img");
  });

  test("a black capture remains selectable and displays its captured pixels", () => {
    const html = render([display({ native_id: "A" })], {
      A: { native_id: "A", data_url: "data:image/png;base64,AAAA", blank: true },
    });
    expect(html).toContain('src="data:image/png;base64,AAAA"');
    expect(html).not.toMatch(/<button[^>]*\sdisabled(?:=|\s|>)/);
  });

  test("capture failure does not disable the display selector", () => {
    const html = render([display({ native_id: "A" })]);
    expect(html).not.toContain("<img");
    expect(html).toContain("<button");
    expect(html).not.toMatch(/<button[^>]*\sdisabled(?:=|\s|>)/);
  });

  test("marks the selected display", () => {
    const html = renderToStaticMarkup(
      <DisplayCards
        displays={[
          display({ native_id: "a", number: 1 }),
          display({ native_id: "b", origin_x: 1920, number: 2 }),
        ]}
        target={{ native_id: "b", label: "b", present: true, confirmed: true, number: 2 }}
        thumbnails={{}}
        onSelect={() => {}}
        disabled={false}
      />,
    );
    expect(html.split('aria-pressed="true"').length - 1).toBe(1);
    expect(html.split('aria-pressed="false"').length - 1).toBe(1);
  });
});
