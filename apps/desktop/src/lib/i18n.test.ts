import { afterEach, describe, expect, test } from "bun:test";
import { applyLocale, currentLocale, resolveLocale, t } from "../i18n";
import { renderMessage } from "../api";

afterEach(() => applyLocale("zh"));

describe("locale resolution", () => {
  test("explicit language overrides the system", () => {
    expect(resolveLocale("zh")).toBe("zh");
    expect(resolveLocale("en")).toBe("en");
    expect(["zh", "en"]).toContain(resolveLocale("system"));
  });
  test("the active language changes immediately", () => {
    applyLocale("en");
    expect(currentLocale()).toBe("en");
    applyLocale("zh");
    expect(currentLocale()).toBe("zh");
  });
});

describe("message interpolation", () => {
  test("unknown messages remain readable in either locale", () => {
    for (const locale of ["zh", "en"] as const) {
      applyLocale(locale);
      expect(t("Uncatalogued {count}", { count: 7 })).toBe("Uncatalogued 7");
    }
  });
  test("missing parameters remain visible", () => {
    applyLocale("en");
    expect(t("Uncatalogued {count}")).toBe("Uncatalogued {count}");
  });
  test("backend evidence objects render their message and parameters", () => {
    applyLocale("en");
    expect(renderMessage({ key: "Connection {session}", params: { session: "42" } })).toBe("Connection 42");
  });
});
