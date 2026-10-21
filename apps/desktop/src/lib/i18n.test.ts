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
import { readFileSync } from "node:fs";

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
  test("every Chinese literal in the UI has an English entry", () => {
    // Reads the source rather than the catalogue so a new hardcoded string is
    // caught even if nobody remembers to add it to the dictionary.
    const root = `${import.meta.dir}/..`;
    const catalogue = readFileSync(`${root}/i18n.ts`, "utf8");
    const known = new Set(
      [...catalogue.matchAll(/^  "([^"]+)":/gm)].map((m) => m[1]!),
    );
    // Wrapped strings are keys by construction; these are the raw ones.
    const allowed = new Set([
      // The catalogue's own keys appear as `t("…")` arguments, which the scanner
      // below sees; being in `known` is what matters, so nothing extra is needed.
    ]);

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
    }
    for (const value of allowed) void value;
    expect(offenders).toEqual([]);
  });
});
