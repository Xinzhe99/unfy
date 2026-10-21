/**
 * Design-token guards.
 *
 * Run with `bun test`.
 *
 * # Why these are tests and not review comments
 *
 * The visual brief was "no orange boxes, no green lines, not obviously
 * AI-generated". Every violation of that in this project arrived the same way: a
 * page reached for a raw colour instead of a token, or a token gained saturation
 * that only shows up in one theme. Review catches it once; a test catches it
 * forever.
 *
 * The rules:
 *   * source files may only name colours through `var(--color-*)`;
 *   * the accent must be near-neutral in both themes (a blue accent was
 *     explicitly rejected);
 *   * semantic hues exist only as three values, and only for state.
 */
import { describe, expect, test } from "bun:test";

// `import.meta.dir` is already an absolute, platform-correct path. Building one
// from `import.meta.url` yields "/F:/..." on Windows, which no file API accepts.
const ROOT = `${import.meta.dir}/../..`;

async function sourceFiles(): Promise<string[]> {
  const glob = new Bun.Glob("src/**/*.{ts,tsx,css}");
  const files: string[] = [];
  for await (const file of glob.scan({ cwd: ROOT })) {
    // Tests contain the very patterns they search for; scanning them makes the
    // rule impossible to satisfy.
    if (file.includes(".test.")) continue;
    files.push(file);
  }
  return files;
}

async function readText(path: string): Promise<string> {
  return Bun.file(`${ROOT}/${path}`).text();
}

describe("colour tokens", () => {
  test("no source file hardcodes a hex, rgb() or oklch() outside the token file", async () => {
    const offenders: string[] = [];
    for (const file of await sourceFiles()) {
      if (file.endsWith("styles.css")) continue;
      const text = await readText(file);
      // Allow the SVG icon set's uses of currentColor and the recorder's none.
      const hits = text.match(/#[0-9a-fA-F]{3,8}\b|\brgba?\(|\boklch\(/g);
      if (hits) offenders.push(`${file}: ${hits.join(", ")}`);
    }
    expect(offenders).toEqual([]);
  });

  test("every colour token referenced in TSX is defined in styles.css", async () => {
    const css = await readText("src/styles.css");
    const defined = new Set(
      [...css.matchAll(/--color-[a-z-]+(?=\s*:)/g)].map((m) => m[0]),
    );

    const used = new Set<string>();
    for (const file of await sourceFiles()) {
      const text = await readText(file);
      for (const match of text.matchAll(/var\((--color-[a-z-]+)\)/g)) {
        used.add(match[1]!);
      }
    }

    const missing = [...used].filter((name) => !defined.has(name));
    expect(missing).toEqual([]);
  });

  test("the accent is near-neutral in both themes", async () => {
    const css = await readText("src/styles.css");

    // oklch(L C H): the second component is chroma. Below 0.02 the hue is not
    // perceptible, which is what "monochrome" means here. A blue accent was
    // 0.155 and the dark one 0.14.
    // Only *literal* definitions count. `--color-accent: var(--conflux-dark-accent)`
    // is the dark-theme alias and carries no chroma of its own.
    const accentLines = css
      .split("\n")
      .filter(
        (line) =>
          /^\s*--(color|conflux-dark)-accent(-hover)?\s*:/.test(line) &&
          line.includes("oklch("),
      );

    expect(accentLines.length).toBeGreaterThan(0);
    for (const line of accentLines) {
      const match = line.match(/oklch\(\s*[\d.]+\s+([\d.]+)/);
      expect(match).not.toBeNull();
      const chroma = Number(match![1]);
      expect(chroma).toBeLessThan(0.02);
    }
  });

  test("semantic colours are declared once per theme and nowhere else", async () => {
    const css = await readText("src/styles.css");
    for (const token of ["success", "warning", "danger"]) {
      const occurrences = [
        ...css.matchAll(new RegExp(`--(?:color|conflux-dark)-${token}\\s*:`, "g")),
      ];
      // One light value, one dark value, and two aliases: the explicit dark
      // class and the `prefers-color-scheme` block that serves `theme-system`.
      // Any other declaration means a page or component invented a hue.
      expect(occurrences.length).toBe(4);
    }
  });
});

describe("layout primitives", () => {
  test("pages use Card rather than bespoke bordered containers", async () => {
    // A page that hand-rolls its own container drifts in radius, padding and
    // border colour, which is exactly how a UI starts to look assembled.
    const pages = new Bun.Glob("src/pages/*.tsx");
    const offenders: string[] = [];
    for await (const file of pages.scan({ cwd: ROOT })) {
      if (file.includes(".test.")) continue;
      const text = await readText(file);
      const cards = (text.match(/<Card[\s>]/g) ?? []).length;
      const bordered = (text.match(/rounded-\[var\(--radius-card\)\]/g) ?? []).length;
      if (bordered > 0) offenders.push(`${file}: ${bordered} bespoke card(s)`);
      if (cards === 0 && !file.includes("Onboarding")) {
        offenders.push(`${file}: no Card used`);
      }
    }
    expect(offenders).toEqual([]);
  });
});

/**
 * WCAG contrast for the text tokens.
 *
 * The muted token carries 11 px hint text, which is small text by WCAG's
 * definition and therefore needs 4.5:1. At its original lightness it reached
 * 3.96:1 on white - readable in a screenshot, failing for anyone with reduced
 * contrast sensitivity, and invisible to every other check.
 */
describe("text contrast", () => {
  function oklchToRgb(L: number, C: number, h: number): [number, number, number] {
    const a = C * Math.cos((h * Math.PI) / 180);
    const b = C * Math.sin((h * Math.PI) / 180);
    const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
    const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
    const s_ = L - 0.0894841775 * a - 1.291485548 * b;
    const l = l_ ** 3;
    const m = m_ ** 3;
    const s = s_ ** 3;
    const enc = (x: number) => {
      const v = x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055;
      return Math.max(0, Math.min(255, Math.round(v * 255)));
    };
    return [
      enc(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
      enc(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
      enc(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    ];
  }

  function luminance([r, g, b]: [number, number, number]): number {
    const f = (c: number) => {
      const s = c / 255;
      return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  }

  function ratio(a: [number, number, number], b: [number, number, number]): number {
    const la = luminance(a);
    const lb = luminance(b);
    return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
  }

  test("every text token reaches WCAG AA on its surface", async () => {
    // Light: surface is white. Dark: surface is the dark surface token.
    const lightSurface = oklchToRgb(1, 0, 0);
    const darkSurface = oklchToRgb(0.213, 0.007, 265);

    const pairs: [string, number, number, number, [number, number, number]][] = [
      ["light fg", 0.235, 0.008, 265, lightSurface],
      ["light fg-soft", 0.44, 0.008, 265, lightSurface],
      ["light muted", 0.535, 0.007, 265, lightSurface],
      ["light danger", 0.51, 0.16, 25, lightSurface],
      ["dark fg", 0.94, 0.004, 265, darkSurface],
      ["dark fg-soft", 0.79, 0.006, 265, darkSurface],
      ["dark muted", 0.63, 0.008, 265, darkSurface],
      ["dark danger", 0.68, 0.15, 25, darkSurface],
    ];

    const failures = pairs
      .map(([name, L, C, h, surface]) => ({
        name,
        ratio: ratio(oklchToRgb(L, C, h), surface),
      }))
      .filter(({ ratio: r }) => r < 4.5);

    expect(failures).toEqual([]);
  });

  test("muted text is still visibly quieter than body text", async () => {
    // A contrast fix that makes hint text as dark as body text loses the
    // hierarchy the design depends on.
    const white = oklchToRgb(1, 0, 0);
    const fg = ratio(oklchToRgb(0.235, 0.008, 265), white);
    const muted = ratio(oklchToRgb(0.535, 0.007, 265), white);
    expect(muted).toBeLessThan(fg - 5);
  });
});
