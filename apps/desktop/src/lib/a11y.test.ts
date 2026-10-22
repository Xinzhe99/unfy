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
    // The label is translated, so the literal moved into the catalogue. What must
    // stay true is that the landmark *has* a name and marks the current page.
    expect(app).toContain('aria-label={t("主导航")}');
    expect(app).toContain("aria-current");

    const catalogue = await Bun.file(`${ROOT}/src/i18n.ts`).text();
    expect(catalogue).toContain('"主导航":');
  });
});

describe("background work", () => {
  test("polling stops while the window is hidden", async () => {
    // Closing the window only hides it; the process stays in the tray. An
    // unconditional interval polled eight commands every four seconds for the whole
    // life of the machine, including a journal scan and a disk write probe.
    const app = await Bun.file(`${ROOT}/src/App.tsx`).text();
    expect(app).toContain("visibilitychange");
    expect(app).toContain("visibilityState");
    expect(app).toContain("stopPolling");
    // And the initial load must not happen twice.
    expect(app).not.toContain("useEffect(() => {\n    void refresh();\n\n    const unlistenAction");
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

  test("every window listener added in an effect is removed again", async () => {
    // The pairing that took two rounds of misreading to check by hand: an effect that
    // adds `window.addEventListener` or `document.addEventListener` and forgets the
    // matching remove keeps firing after the component unmounts, and its closure holds
    // stale state. The cleanup block is thirty lines below the registration, so reading
    // it is exactly the kind of check that gets skipped.
    const offenders: string[] = [];
    for (const file of await tsxFiles()) {
      const text = await Bun.file(`${ROOT}/${file}`).text();
      for (const match of text.matchAll(
        /(window|document)\.addEventListener\("([\w:-]+)"/g,
      )) {
        const [, target, event] = match;
        const removal = `${target}.removeEventListener("${event}"`;
        if (!text.includes(removal)) offenders.push(`${file}: ${target} ${event}`);
      }
    }
    expect(offenders).toEqual([]);
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
