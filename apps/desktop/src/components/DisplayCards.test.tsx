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
    ...overrides,
  };
}

function render(displays: DisplayDto[]): string {
  return renderToStaticMarkup(
    <DisplayCards
      displays={displays}
      target={null}
      thumbnails={{}}
      onSelect={() => {}}
      disabled={false}
    />,
  );
}

describe("display cards", () => {
  test("numbers a left-hand monitor first even when the platform lists it second", () => {
    // The real configuration: DISPLAY2 sits at x = -1920, so it is physically to
    // the left but enumerated second.
    const html = render([
      display({ native_id: "primary", origin_x: 0, primary: true }),
      display({ native_id: "left", origin_x: -1920 }),
    ]);

    // Cards are in DOM order == reading order, so the left monitor comes first.
    const leftIndex = html.indexOf("显示器 1");
    const rightIndex = html.indexOf("显示器 2");
    expect(leftIndex).toBeGreaterThan(-1);
    expect(rightIndex).toBeGreaterThan(-1);
    expect(leftIndex).toBeLessThan(rightIndex);

    // And the badge inside the first card is "1".
    const firstBadge = html.indexOf(">1<");
    expect(firstBadge).toBeGreaterThan(-1);
  });

  test("stacks a vertically arranged pair instead of laying it out side by side", () => {
    const html = render([
      display({ native_id: "top", origin_y: 0 }),
      display({ native_id: "bottom", origin_y: 1080 }),
    ]);
    // The second card is below: its `top` offset is non-zero.
    expect(html).toMatch(/top:\s*50%/);
    expect(html).toContain("显示器 1");
    expect(html).toContain("显示器 2");
  });

  test("keeps a single display at full width", () => {
    const html = render([display({ native_id: "only", primary: true })]);
    expect(html).toMatch(/width:\s*100%/);
    expect(html).toContain("主显示器");
  });

  test("reports an empty topology instead of rendering an empty box", () => {
    expect(render([])).toContain("未检测到显示器");
  });

  test("marks the selected display", () => {
    const html = renderToStaticMarkup(
      <DisplayCards
        displays={[display({ native_id: "a" }), display({ native_id: "b", origin_x: 1920 })]}
        target={{ native_id: "b", label: "b", present: true }}
        thumbnails={{}}
        onSelect={() => {}}
        disabled={false}
      />,
    );
    expect(html).toContain("使用中");
    // Exactly one card carries it, or the user cannot tell which is chosen.
    expect(html.split("使用中").length - 1).toBe(1);
  });
});
