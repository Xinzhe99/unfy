import { afterEach, describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { OverviewPage } from "./Overview";
import { Onboarding } from "./Onboarding";
import { GeneralPage } from "./General";
import { PendingCard } from "../components/prompts";
import { PlanPreview } from "../components/PlanPreview";
import { applyLocale, t } from "../i18n";
import type { PreviewDto, SettingsDto, StatusDto } from "../api";

const settings: SettingsDto = {
  layout_mode: "default", margin: 12, gap: 8, autostart: false, theme: "system",
  autostart_actual: false, autostart_error: null, migration_notice: false,
  language: "en", hotkey_gather: null, hotkey_restore: null, restore_delay_secs: 15,
  auto_update: true, excluded_apps: [], onboarding_done: true, onboarding_step: 0,
  auto_detect: "off", auto_gather: false, auto_restore: false, remote_screens: "single",
  target: { native_id: "screen", label: "screen", present: true, confirmed: true, number: 1 },
  remote_active: false, remote_unknown: false, remote_evidence: null,
};
const status: StatusDto = {
  state: "idle", platform: "windows", supported: true, has_permission: true,
  permission_name: "窗口控制", display_count: 1, target: settings.target,
  pending_windows: 0, last_error: null, paused: false, captured_windows: 0,
  settings_recovered: false, recovery_blockers: 0,
};
const overviewProps = {
  status, settings, preview: null, displays: [], activity: [], progress: null,
  busy: false, pendingApps: [], refusedApps: [], diagnosticsPreview: null,
  onPreview() {}, onMigrate() {}, onRestore() {}, onPause() {}, onRescue() {},
  onIdentify() {}, onExport() {}, onPreviewDiagnostics() {}, onOpenConfigDir() {},
  onOpenDisplays() {}, onExclude() {}, onKeepLayout() {}, onDismissError() {}, onRecheck() {},
};

// Resolve labels through the catalogue: wording changes should not fail behavior tests.
function buttonMarkup(html: string, key: string): string | undefined {
  return html.match(/<button\b[^>]*>[\s\S]*?<\/button>/g)?.find((button) => button.includes(t(key)));
}

afterEach(() => applyLocale("zh"));

describe("recovery actions", () => {
  test("idle and paused state cannot hide outstanding recovery", () => {
    applyLocale("en");
    const html = renderToStaticMarkup(<OverviewPage {...overviewProps} status={{ ...status, paused: true, pending_windows: 3, captured_windows: 6 }} />);
    const restore = buttonMarkup(html, "恢复本次布局");
    expect(restore).toBeDefined();
    expect(restore).not.toMatch(/\sdisabled(?:=|\s|>)/);
    expect(buttonMarkup(html, "放弃恢复记录")).toBeUndefined();
    expect(buttonMarkup(html, "保留当前位置并结束本次整理")).toBeDefined();
  });

  test("unknown recovery blockers prohibit new gathers without offering global discard", () => {
    applyLocale("en");
    const html = renderToStaticMarkup(<OverviewPage {...overviewProps} status={{ ...status, recovery_blockers: 1 }} />);
    expect(buttonMarkup(html, "恢复本次布局")).toBeDefined();
    expect(buttonMarkup(html, "重新检查")).toBeDefined();
    expect(buttonMarkup(html, "整理到目标屏")).toBeUndefined();
    expect(buttonMarkup(html, "保留当前位置并结束本次整理")).toBeUndefined();
  });

  test("permission loss keeps recovery visible but disables platform writes", () => {
    applyLocale("en");
    const html = renderToStaticMarkup(<OverviewPage {...overviewProps} status={{ ...status, pending_windows: 2, has_permission: false }} />);
    expect(buttonMarkup(html, "恢复本次布局")).toMatch(/\sdisabled(?:=|\s|>)/);
  });

  test("each unowned journal has its own recovery action", () => {
    applyLocale("en");
    const html = renderToStaticMarkup(<PendingCard pending={{ present: true, windows: 3, blockers: 1, transactions: [{ id: "first", windows: 1 }, { id: "second", windows: 2 }], may_have_moved: true, truncated_bytes: 0 }} busy={false} onRestore={() => {}} onDismiss={() => {}} onRecheck={() => {}} onRestoreTransaction={() => {}} />);
    expect((html.match(/<button\b[^>]*>[\s\S]*?<\/button>/g) ?? []).filter((button) => button.includes(t("查看并恢复"))).length).toBe(2);
    expect(html).toContain('role="alert"');
  });
});

test("a saved but unconfirmed target requires selection before gathering", () => {
  applyLocale("en");
  const html = renderToStaticMarkup(<OverviewPage {...overviewProps} status={{ ...status, target: { ...settings.target!, confirmed: false } }} />);
  expect(buttonMarkup(html, "选择目标显示器")).toBeDefined();
  expect(buttonMarkup(html, "整理到目标屏")).toBeUndefined();
});

test("single-display topology does not itself prohibit a manual gather", () => {
  applyLocale("en");
  const html = renderToStaticMarkup(<OverviewPage {...overviewProps} />);
  expect(buttonMarkup(html, "整理到目标屏")).toBeDefined();
  expect(buttonMarkup(html, "整理到目标屏")).not.toMatch(/\sdisabled(?:=|\s|>)/);
});

describe("plans and results", () => {
  const preview: PreviewDto = {
    mode: "default", target_present: true, cascaded: true, unplaced: [], skipped: [],
    target_work: { left: 0, top: 0, width: 1920, height: 1080 },
    placements: ["editor.exe", "browser.exe"].map((app_name, index) => ({ window_id: String(index), app_name, order: index + 1, clamped: true, rect: { left: 0, top: 0, width: 1000, height: 800 } })),
  };
  test("overlapping default placements retain a readable application list", () => {
    const html = renderToStaticMarkup(<PlanPreview preview={preview} />);
    const list = html.match(/<ol\b[^>]*>([\s\S]*?)<\/ol>/)?.[1];
    expect(list).toContain("editor.exe");
    expect(list).toContain("browser.exe");
    expect((list?.match(/<li\b/g) ?? []).length).toBe(2);
  });

  test("actual results survive without a preview and identify unresolved windows", () => {
    const html = renderToStaticMarkup(<OverviewPage {...overviewProps} latestResults={[{ window_id: "window-proof", app_name: null, outcome: "unknown", reason: "identity_uncertain" }]} />);
    expect(html).toContain("window-proof");
    expect(html).not.toContain(t("布局预览"));
  });
});

test("legacy onboarding step cannot crash the two-step guide", () => {
  const html = renderToStaticMarkup(<Onboarding displays={[]} settings={{ ...settings, onboarding_step: 4 }} thumbnails={{}} busy={false} onSelectTarget={async () => true} onIdentify={() => {}} onFinish={() => {}} />);
  expect(html).toContain("<main");
});

test("unsupported automatic updates have disabled controls", () => {
  applyLocale("en");
  const html = renderToStaticMarkup(<GeneralPage settings={settings} hotkeys={null} health={[]} updateStatus={{ current_version: "1.0.0", configured: false, auto_update: true, last_outcome: null, last_version: null, last_detail: null }} platform="windows" busy={false} onSave={() => {}} onCheckUpdate={() => {}} onReset={() => {}} onShowGuide={() => {}} onClearHistory={() => {}} />);
  expect(buttonMarkup(html, "检查更新")).toMatch(/\sdisabled(?:=|\s|>)/);
  expect(buttonMarkup(html, "安装更新并重启")).toBeUndefined();
});

test("autostart reflects failed OS registration while preserving the requested preference", () => {
  applyLocale("en");
  const html = renderToStaticMarkup(<GeneralPage settings={{ ...settings, autostart: true, autostart_actual: false, autostart_error: "registration-denied" }} hotkeys={null} health={[]} updateStatus={null} platform="windows" busy={false} onSave={() => {}} onCheckUpdate={() => {}} onReset={() => {}} onShowGuide={() => {}} onClearHistory={() => {}} />);
  const startupSwitch = html.match(/<button\b[^>]*>/g)?.find((button) => button.includes(`aria-label="${t("开机自动启动")}"`));
  expect(startupSwitch).toContain('aria-checked="false"');
  expect(html).toContain("registration-denied");
  expect(html).toContain('role="alert"');
  expect(buttonMarkup(html, "重新应用启动设置")).toBeDefined();
});
