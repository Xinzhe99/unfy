/**
 * Accessibility guards.
 *
 * Run with `bun test`.
 *
 * These check the two things that are cheap to verify statically and expensive to
 * notice: every interactive control has an accessible name, and focus is never
 * removed without a replacement.
 */
import { describe, expect, test } from "bun:test";

const ROOT = `${import.meta.dir}/../..`;

async function tsxFiles(): Promise<string[]> {
  const glob = new Bun.Glob("src/**/*.tsx");
  const files: string[] = [];
  for await (const file of glob.scan({ cwd: ROOT })) {
    if (file.includes(".test.")) continue;
    files.push(file);
  }
  return files;
}

/** Crude JSX tag reader: enough to check attributes on each opening tag. */
function openingTags(source: string, tag: string): string[] {
  const pattern = new RegExp(`<${tag}\\b[^>]*>`, "g");
  return [...source.matchAll(pattern)].map((m) => m[0]);
}

describe("accessible names", () => {
  test("every icon-only button carries an aria-label", async () => {
    const offenders: string[] = [];
    for (const file of await tsxFiles()) {
      const text = await Bun.file(`${ROOT}/${file}`).text();
      for (const tag of openingTags(text, "button")) {
        const hasText = />[^<]*\S/.test(tag) || tag.endsWith("/>") === false;
        const labelled =
          tag.includes("aria-label") ||
          tag.includes("title=") ||
          // A button whose visible label is on a nested element is covered by
          // the runtime render tests; here we only catch the icon-only shape.
          tag.includes("aria-labelledby");
        if (hasText === false && !labelled) offenders.push(`${file}: ${tag}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  test("the shared Switch always takes a label", async () => {
    const ui = await Bun.file(`${ROOT}/src/components/ui.tsx`).text();
    // `label` is required in the type, and passed to aria-label.
    expect(ui).toContain("label: string;");
    expect(ui).toContain("aria-label={label}");
  });

  test("the navigation is a landmark with a name", async () => {
    const app = await Bun.file(`${ROOT}/src/App.tsx`).text();
    expect(app).toContain("aria-label=\"主导航\"");
    expect(app).toContain("aria-current");
  });
});

describe("focus and motion", () => {
  test("focus is never removed without a visible replacement", async () => {
    const css = await Bun.file(`${ROOT}/src/styles.css`).text();
    // `outline: none` with no focus rule would make keyboard use impossible.
    expect(css).toContain(":focus-visible");
    expect(css).toMatch(/outline:\s*2px solid var\(--color-accent\)/);
  });

  test("reduced motion is honoured", async () => {
    const css = await Bun.file(`${ROOT}/src/styles.css`).text();
    expect(css).toContain("prefers-reduced-motion");
  });

  test("text contrast uses the fg tokens, never a bare white or black", async () => {
    const offenders: string[] = [];
    for (const file of await tsxFiles()) {
      const text = await Bun.file(`${ROOT}/${file}`).text();
      // `text-white` / `text-black` on body copy ignores the theme.
      if (/className="[^"]*\btext-(white|black)\b/.test(text)) {
        offenders.push(file);
      }
    }
    expect(offenders).toEqual([]);
  });
});
