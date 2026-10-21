/**
 * Render smoke tests.
 *
 * Run with `bun test`.
 *
 * # Why this exists
 *
 * React failures are silent at build time: a wrong field access produces a
 * perfectly valid bundle that renders a blank window, and the macOS/Windows
 * screenshot checks cannot run on a locked or headless machine. These tests
 * render every page with representative data and fail if the tree throws or comes
 * out empty.
 *
 * They deliberately do not assert on copy or layout: those change constantly and
 * a test that re-pins them would only add friction. What they pin is "the page
 * renders without throwing and shows something".
 */
import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { OverviewPage } from "../pages/Overview";
import { DisplaysPage } from "../pages/Displays";
import { ExclusionsPage } from "../pages/Exclusions";
import { GeneralPage } from "../pages/General";
import { Onboarding } from "../pages/Onboarding";
import { HotkeyRecorder } from "../components/HotkeyRecorder";
import { PlanPreview } from "../components/PlanPreview";
import type {
  ActivityEntryDto,
  AppDto,
  DisplayDto,
  HotkeyStatusDto,
  PreviewDto,
  SettingsDto,
  StatusDto,
} from "../api";

const displays: DisplayDto[] = [
  {
    native_id: "DISPLAY1",
    label: "\\\\.\\DISPLAY1",
    width: 1920,
    height: 1080,
    origin_x: 0,
    origin_y: 0,
    work_width: 1920,
    work_height: 1040,
    scale: 1,
    primary: true,
    restorable: true,
    is_target: true,
  },
  {
    native_id: "DISPLAY2",
    label: "\\\\.\\DISPLAY2",
    width: 1920,
    height: 1080,
    origin_x: 1920,
    origin_y: 0,
    work_width: 1920,
    work_height: 1040,
    scale: 1.5,
    primary: false,
    restorable: true,
    is_target: false,
  },
];

const settings: SettingsDto = {
  layout_mode: "grid",
  margin: 12,
  gap: 8,
  autostart: false,
  theme: "system",
  hotkey_gather: null,
  hotkey_restore: null,
  restore_delay_secs: 0,
  auto_update: true,
  telemetry: false,
  excluded_apps: [],
  onboarding_done: true,
  onboarding_step: 3,
  auto_detect: "balanced",
  auto_gather: false,
  auto_restore: false,
  target: { native_id: "DISPLAY1", label: "DISPLAY1", present: true },
  remote_active: false,
  remote_evidence: null,
};

const status: StatusDto = {
  state: "idle",
  platform: "windows",
  supported: true,
  has_permission: true,
  permission_name: "窗口控制",
  display_count: 2,
  target: { native_id: "DISPLAY1", label: "DISPLAY1", present: true },
  pending_windows: 0,
  last_error: null,
  paused: false,
  captured_windows: 0,
};

const preview: PreviewDto = {
  target_present: true,
  placements: [
    {
      window_id: "1:1001",
      label: "notepad.exe",
      rect: { left: 0, top: 0, width: 960, height: 520 },
      clamped: false,
    },
  ],
  unplaced: [],
  skipped: [{ window_id: "1:1002", app_name: "msedge.exe", reason: "excluded_app" }],
  cascaded: false,
  target_work: { left: 0, top: 0, width: 1920, height: 1040 },
};

const activity: ActivityEntryDto[] = [
  {
    at: 1761148800,
    kind: "gather",
    affected: 3,
    unsettled: 1,
    note: "",
    trigger: "window",
  },
  {
    at: 1761148000,
    kind: "auto_restore",
    affected: 3,
    unsettled: 0,
    note: "",
    trigger: "auto",
  },
];

const hotkeys: HotkeyStatusDto = {
  gather: null,
  gather_error: null,
  restore: null,
  restore_error: null,
};

function noop() {}

/** Every page must render non-empty markup without throwing. */
describe("pages render", () => {
  test("overview with no history", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        onOpenDisplays={noop}
      />,
    );
    expect(html.length).toBeGreaterThan(200);
    expect(html).toContain("整理到目标屏");
  });

  test("overview with active layout, history and a lost target", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{
          ...status,
          state: "degraded",
          captured_windows: 4,
          target: { native_id: "GONE", label: "DISPLAY9", present: false },
        }}
        settings={{ ...settings, remote_active: true, remote_evidence: "AnyDesk 已连接" }}
        preview={preview}
        displays={displays}
        activity={activity}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        onOpenDisplays={noop}
      />,
    );
    // A degraded state must offer restore, not a gather that can only fail.
    expect(html).toContain("恢复本次布局");
    expect(html).toContain("已断开");
    expect(html).toContain("AnyDesk 已连接");
  });

  test("overview on a single display disables gathering", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, display_count: 1 }}
        settings={settings}
        preview={null}
        displays={[displays[0]!]}
        activity={[]}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        onOpenDisplays={noop}
      />,
    );
    expect(html).toContain("只检测到一块显示器");
  });

  test("displays page", () => {
    const html = renderToStaticMarkup(
      <DisplaysPage
        displays={displays}
        settings={settings}
        target={status.target}
        thumbnails={{ DISPLAY1: "data:image/png;base64,AA==" }}
        busy={false}
        onSelect={noop}
        onIdentify={noop}
        onSave={noop}
      />,
    );
    expect(html).toContain("使用中");
    expect(html).toContain("无法截取画面");
  });

  test("exclusions page with running apps", () => {
    const apps: AppDto[] = [
      { name: "msedge.exe", excluded: true },
      { name: "notepad.exe", excluded: false },
    ];
    const html = renderToStaticMarkup(
      <ExclusionsPage
        settings={{ ...settings, excluded_apps: ["msedge.exe"] }}
        runningApps={apps}
        busy={false}
        onSave={noop}
      />,
    );
    expect(html).toContain("msedge.exe");
    expect(html).toContain("1 条规则");
  });

  test("settings page in both update configurations", () => {
    const configured = renderToStaticMarkup(
      <GeneralPage
        settings={settings}
        hotkeys={hotkeys}
        updateStatus={{
          current_version: "0.1.0",
          auto_update: true,
          configured: true,
          last_outcome: "failed",
          last_version: "0.1.1",
          last_detail: "install failed: invalid updater binary format",
        }}
        busy={false}
        onSave={noop}
        onCheckUpdate={noop}
      />,
    );
    expect(configured).toContain("已配置更新源");
    expect(configured).toContain("invalid updater binary format");

    const unconfigured = renderToStaticMarkup(
      <GeneralPage
        settings={settings}
        hotkeys={hotkeys}
        updateStatus={{
          current_version: "0.1.0",
          auto_update: true,
          configured: false,
          last_outcome: null,
          last_version: null,
          last_detail: null,
        }}
        busy={false}
        onSave={noop}
        onCheckUpdate={noop}
      />,
    );
    expect(unconfigured).toContain("未配置更新源");
  });

  test("onboarding at every step", () => {
    for (const step of [0, 1, 2, 3]) {
      const html = renderToStaticMarkup(
        <Onboarding
          displays={displays}
          settings={{ ...settings, onboarding_done: false, onboarding_step: step }}
          thumbnails={{}}
          busy={false}
          onSelectTarget={async () => {}}
          onIdentify={noop}
          onSetDetect={async () => {}}
          onFinish={noop}
        />,
      );
      expect(html.length).toBeGreaterThan(100);
    }
  });

  test("hotkey recorder shows the accelerator in human form", () => {
    const html = renderToStaticMarkup(
      <HotkeyRecorder
        value="CmdOrCtrl+Alt+G"
        disabled={false}
        onChange={noop}
      />,
    );
    expect(html).toContain("Ctrl");
    expect(html).toContain("Alt");
    // "未设置" must not appear when a shortcut is bound.
    expect(html).not.toContain("未设置");
  });

  test("plan preview draws every placement", () => {
    const html = renderToStaticMarkup(<PlanPreview preview={preview} />);
    expect(html).toContain("<svg");
    expect(html).toContain("rect");
  });

  test("plan preview explains an empty plan", () => {
    const html = renderToStaticMarkup(
      <PlanPreview
        preview={{ ...preview, placements: [], skipped: [] }}
      />,
    );
    expect(html).toContain("没有可整理的窗口");
  });
});
