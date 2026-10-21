/**
 * Interface language.
 *
 * Run with `bun test`.
 *
 * The catalogue is keyed by the Chinese source string, so the failure mode to guard
 * against is not a crash but a *silent* one: a missing English entry renders the
 * Chinese, which looks like a translation gap rather than a bug. These tests pin
 * the contract that makes that fallback safe, plus a coverage check so strings
 * added later cannot quietly stay untranslated.
 */
import { afterEach, describe, expect, test } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";

import { applyLocale, resolveLocale, t } from "../i18n";

afterEach(() => {
  applyLocale("zh");
});

describe("resolution", () => {
  test("an explicit choice wins over the system", () => {
    expect(resolveLocale("zh")).toBe("zh");
    expect(resolveLocale("en")).toBe("en");
  });

  test("system falls back to a real locale, never an empty one", () => {
    // `navigator` is undefined under Bun, which exercises the fallback path.
    expect(["zh", "en"]).toContain(resolveLocale("system"));
  });
});

describe("translation", () => {
  test("returns the source string in Chinese", () => {
    applyLocale("zh");
    expect(t("就绪")).toBe("就绪");
  });

  test("returns English when the entry exists", () => {
    applyLocale("en");
    expect(t("就绪")).toBe("Ready");
    expect(t("布局已启用")).toBe("Layout active");
  });

  test("falls back to the Chinese source for an unknown key", () => {
    applyLocale("en");
    // A string with no English entry must render something readable, not a key.
    expect(t("这条文案还没有英文")).toBe("这条文案还没有英文");
  });

  test("substitutes named placeholders", () => {
    applyLocale("en");
    expect(t("已整理 {count} 个窗口", { count: 3 })).toBe("Gathered 3 windows");
    expect(t("显示器 {number}（主）", { number: 2 })).toBe(
      "Display 2 (primary)",
    );
  });

  test("leaves an unsupplied placeholder visible", () => {
    applyLocale("en");
    // Rendering `undefined` would hide the mistake; leaving the braces shows it.
    expect(t("已整理 {count} 个窗口")).toContain("{count}");
  });
});

describe("coverage", () => {
  /**
   * Every sentence the Rust side sends must exist in the catalogue too.
   *
   * The backend cannot word these itself, so it sends a key and the frontend looks
   * it up. Nothing on either side of the boundary checks that the entry is there:
   * the TypeScript scan cannot see Rust, and the Rust tests only check the shape of
   * the message. A missing entry renders the Chinese source in an English window.
   */
  test("every backend message key has an English entry", () => {
    const root = `${import.meta.dir}/..`;
    const catalogue = readFileSync(`${root}/i18n.ts`, "utf8");
    const known = new Set(
      [...catalogue.matchAll(/^  "([^"]+)":/gm)].map((m) => m[1]!),
    );
    const backend = `${root}/../src-tauri/src`;
    const missing: string[] = [];
    let found = 0;
    for (const entry of readdirSync(backend, { withFileTypes: true })) {
      if (!entry.isFile() || !entry.name.endsWith(".rs")) continue;
      const source = readFileSync(`${backend}/${entry.name}`, "utf8");
      for (const match of source.matchAll(
        /Message::(?:new|one)\((?:\s*)"((?:[^"\\]|\\.)*)"/g,
      )) {
        const key = match[1]!.replace(/\\"/g, '"').replace(/\\\\/g, "\\");
        if (!/[\u3000-\u303f\u4e00-\u9fff\uff00-\uffef]/.test(key)) continue;
        found += 1;
        if (!known.has(key)) missing.push(`${entry.name}: ${key}`);
      }
    }
    // Guard against the regex silently matching nothing after a refactor.
    expect(found).toBeGreaterThan(15);
    expect(missing).toEqual([]);
  });

  /**
   * Every `t("…")` key must exist in the catalogue.
   *
   * A key that is not there falls back to the Chinese source, which is invisible
   * to the compiler and to a render test that does not reach that branch. A batch
   * of them shipped that way: the wrappers were added by searching the source for
   * Chinese, while the English entries were added by searching for something else.
   */
  test("every key used in the source has an English entry", () => {
    const root = `${import.meta.dir}/..`;
    const catalogue = readFileSync(`${root}/i18n.ts`, "utf8");
    const known = new Set(
      [...catalogue.matchAll(/^  "([^"]+)":/gm)].map((m) => m[1]!),
    );
    const files: string[] = [];
    for (const dir of ["", "pages/", "components/", "lib/"]) {
      const base = `${root}/${dir}`;
      for (const entry of readdirSync(base, { withFileTypes: true })) {
        if (!entry.isFile()) continue;
        if (!/\.tsx?$/.test(entry.name)) continue;
        if (entry.name.includes(".test.")) continue;
        if (entry.name === "i18n.ts") continue;
        files.push(`${dir}${entry.name}`);
      }
    }
    const missing: string[] = [];
    for (const file of files) {
      const source = readFileSync(`${root}/${file}`, "utf8");
      for (const match of source.matchAll(/(?<![A-Za-z0-9_$.])t\(\s*"([^"]+)"/g)) {
        // A call written into a comment is documentation, not a lookup; and the
        // lookbehind keeps `onSetDetect("off", …)` from reading as a `t("off")`.
        const lineStart = source.lastIndexOf("\n", match.index!) + 1;
        if (/^\s*(\/\/|\*|\/\*)/.test(source.slice(lineStart, match.index!))) continue;
        if (!known.has(match[1]!)) missing.push(`${file}: ${match[1]}`);
      }
    }
    expect(missing).toEqual([]);
  });

  test("every Chinese literal in the UI has an English entry", () => {
    // Reads the source rather than the catalogue so a new hardcoded string is
    // caught even if nobody remembers to add it to the dictionary.
    const root = `${import.meta.dir}/..`;
    const catalogue = readFileSync(`${root}/i18n.ts`, "utf8");
    const known = new Set(
      [...catalogue.matchAll(/^  "([^"]+)":/gm)].map((m) => m[1]!),
    );
    // Wrapped strings are keys by construction; these are the raw ones.
    // `中文` is a language name and is identical in both locales by design.
    const allowed = new Set(["中文"]);
    for (const value of allowed) known.add(value);

    const files = [
      "App.tsx",
      "pages/Overview.tsx",
      "pages/Displays.tsx",
      "pages/Exclusions.tsx",
      "pages/General.tsx",
      "pages/Onboarding.tsx",
      "components/PlanPreview.tsx",
      "components/DisplayCards.tsx",
      "components/HotkeyRecorder.tsx",
    ];
    const offenders: string[] = [];
    for (const file of files) {
      const source = readFileSync(`${root}/${file}`, "utf8");
      // Quoted props and JSX text nodes that contain Chinese but are not already
      // inside a `t(...)` call.
      for (const match of source.matchAll(
        /(?:title|label|hint|description|placeholder|aria-label)="([^"{}]*[\u4e00-\u9fff][^"{}]*)"/g,
      )) {
        if (!known.has(match[1]!)) offenders.push(`${file}: ${match[1]}`);
      }
      for (const match of source.matchAll(/>([^<>{}]*[\u4e00-\u9fff][^<>{}]*)</g)) {
        const text = match[1]!.trim();
        if (text && !known.has(text)) offenders.push(`${file}: ${text}`);
      }
      // Every Chinese string literal must either be a catalogue key or be wrapped
      // in `t(...)`. This covers the array entries and ternary branches that the
      // JSX scan cannot see - `{ value: "all", label: "全部屏幕" }` looked
      // translated because the surrounding prop was, and shipped in Chinese.
      for (const match of source.matchAll(/"([^"\\<>\n]*[\u4e00-\u9fff][^"\\<>\n]*)"/g)) {
        const literal = match[1]!;
        // Comments quote example copy; they are documentation, not UI text.
        const lineStart = source.lastIndexOf("\n", match.index!) + 1;
        const line = source.slice(lineStart, match.index!);
        if (/^\s*(\/\/|\*|\/\*)/.test(line)) continue;
        if (literal.includes("${")) continue;
        if (literal.trim().length <= 1) continue;
        // The scanner runs over source text, so `>` and `/>` fragments from JSX
        // regexes leak in; they are not copy.
        if (/^[\s/>]*$/.test(literal)) continue;
        if (known.has(literal)) continue;
        const before = source.slice(Math.max(0, match.index! - 2), match.index!);
        if (before.endsWith("t(")) continue;
        offenders.push(`${file}: literal ${literal}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});
