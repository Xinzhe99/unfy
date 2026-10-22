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
import { applyLocale, currentLocale, t } from "../i18n";
import { errorMessage, humanError, skipLabels, stateLabels } from "../api";
import { navigation } from "../App";
import { DisplaysPage } from "../pages/Displays";
import { HotkeyRecorder } from "../components/HotkeyRecorder";
import { PendingCard, PermissionPrompt } from "../components/prompts";
import { PlanPreview } from "../components/PlanPreview";
import type {
  ActivityEntryDto,
  AppDto,
  DisplayDto,
  HealthItemDto,
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
    number: 1,
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
    number: 2,
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
  remote_screens: "single",
  last_refused_apps: [],
  target: { native_id: "DISPLAY1", label: "DISPLAY1", present: true, number: 1 },
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
  target: { native_id: "DISPLAY1", label: "DISPLAY1", present: true, number: 1 },
  pending_windows: 0,
  last_error: null,
  paused: false,
  captured_windows: 0,
  settings_recovered: false,
};

const preview: PreviewDto = {
  target_present: true,
  placements: [
    {
      window_id: "1:1001",
      app_name: "notepad.exe",
      order: 1,
      rect: { left: 0, top: 0, width: 960, height: 520 },
      clamped: false,
    },
    {
      window_id: "1:1002",
      app_name: "notepad.exe",
      order: 2,
      rect: { left: 960, top: 0, width: 960, height: 520 },
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
    note: null,
    trigger: "window",
  },
  {
    at: 1761148000,
    kind: "auto_restore",
    affected: 3,
    unsettled: 0,
    note: null,
    trigger: "auto",
  },
];

const hotkeys: HotkeyStatusDto = {
  gather: null,
  gather_error: null,
  restore: null,
  restore_error: null,
};

/** A sentence as the backend sends it: the key, plus any placeholder values. */
function message(key: string, params: Record<string, string> = {}) {
  return { key, params };
}

const apps: AppDto[] = [
  { name: "msedge.exe", excluded: true },
  { name: "notepad.exe", excluded: false },
];

const health: HealthItemDto[] = [
  {
    id: "permission",
    label: message("窗口控制权限"),
    status: "ok",
    detail: message("可以移动其他应用的窗口。"),
  },
  {
    id: "displays",
    label: message("显示器"),
    status: "ok",
    detail: message("检测到 {count} 块显示器。", { count: "2" }),
  },
  {
    id: "hotkeys",
    label: message("全局快捷键"),
    status: "problem",
    // Raw text from the operating system, which is already in its own language.
    detail: message("Ctrl+Alt+G is already in use"),
  },
  {
    id: "storage",
    label: message("配置目录"),
    status: "ok",
    detail: message("可写入 {path}", { path: "C:\\Users\\dell\\AppData\\Roaming\\Conflux" }),
  },
];

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
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(html.length).toBeGreaterThan(200);
    expect(html).toContain("整理到目标屏");
  });

  test("an unknown activity kind still names what happened", () => {
    // The kind table mirrors a Rust enum. A new variant there used to render as an
    // empty label, because `t(undefined)` returns undefined and React draws nothing:
    // the row read "· 5 个窗口" with no verb. The raw code is not pretty, but it says
    // something and names the string that needs a translation.
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={settings}
        preview={preview}
        displays={displays}
        activity={[
          {
            at: 1761148800,
            kind: "brand_new_kind",
            affected: 5,
            unsettled: 0,
            note: null,
            trigger: "window",
          },
        ]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(html).toContain("brand_new_kind");
    expect(html).toContain("5 个窗口");
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
        settings={{ ...settings, remote_active: true, remote_evidence: message("{name} 已连接", { name: "AnyDesk" }) }}
        preview={preview}
        displays={displays}
        activity={activity}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
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
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(html).toContain("只检测到一块显示器");
  });

  test("overview reports the gather count while it runs", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={{ done: 3, total: 7 }}
        busy
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // "working" alone is indistinguishable from "stuck" when one application
    // holds the batch.
    expect(html).toContain("正在整理 3/7");
  });

  test("overview offers to exclude an application that refused to move", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={{
          ...settings,
          last_refused_apps: ["msedge.exe", "notepad.exe"],
          excluded_apps: ["notepad.exe"],
        }}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // The refusing app is offered; the already-excluded one is not, because a
    // rule that exists cannot be added again.
    expect(html).toContain("msedge.exe");
    expect(html).not.toContain(">notepad.exe<");
    expect(html).toContain("没有接受位置调整");
    // A single remaining name needs no bulk action.
    expect(html).not.toContain("全部排除");
  });

  test("overview reports windows held by an unfinished transaction", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, pending_windows: 5, state: "degraded" }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // "ready" while five windows are displaced is the failure this row prevents.
    expect(html).toContain("待恢复窗口");
    expect(html).toContain("5 个");
  });

  test("overview explains how to trigger a gather without the window", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={{ ...settings, hotkey_gather: null, auto_detect: "off" }}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // Without a shortcut and without detection, the feature is only reachable from
    // this window - which defeats the point of a one-key action.
    expect(html).toContain("设置一个快捷键");
  });

  test("the preview says it keeps itself current", () => {
    // The copy used to call this a snapshot taken at click time, which was true and
    // also the problem: it stayed on screen naming windows the user had since
    // closed. It is recomputed now, and the copy has to say so.
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={settings}
        preview={preview}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(html).toContain("自动更新");
    expect(html).toContain("布局预览");
  });

  test("overview translates a raw command error into an instruction", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, last_error: message("already_active") }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // The raw code is an implementation detail; the sentence is the product.
    expect(html).toContain("请先恢复");
    expect(html).not.toContain("already_active");
    // A journal failure is the only path that stops the engine acting at all; the
    // raw OS error in English is not an explanation a user can act on.
    const journalFailure = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, state: "error", last_error: message("journal write failed: io error: os error 183") }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(journalFailure).toContain("无法写入恢复记录");
    expect(journalFailure).not.toContain("os error");
    // A message about a past failure must be dismissable: once read, it is noise
    // until the next success overwrites it.
    expect(html).toContain("知道了");
  });

  test("an idle engine shows a green dot, not a grey one", () => {
    // Reported directly by the user: "就绪" had a grey dot, which in this palette
    // is the colour of a disabled control. Asserting the class is the only way to
    // see it from a test - the colour lives in CSS, not in the markup.
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, state: "idle" }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    const idleBlock = html.slice(html.indexOf("就绪") - 400, html.indexOf("就绪"));
    expect(idleBlock).toContain("bg-[var(--color-success)]");
    expect(idleBlock).not.toContain("bg-[var(--color-muted)]");
  });

  test("overview names the target display exactly as the picker does", () => {
    // The reported bug: the overview said "显示器 1" while the picker highlighted
    // "显示器 2" for the same monitor. They disagreed because each derived the
    // number differently - one from its array index, the other from a sorted copy.
    const picker = renderToStaticMarkup(
      <DisplaysPage
        displays={displays}
        settings={settings}
        target={status.target}
        thumbnails={{}}
        busy={false}
        onSelect={noop}
        onIdentify={noop}
        onCycle={noop}
        onSave={noop}
      />,
    );
    const overview = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // The target is `displays[0]`: numbered 1 and primary.
    expect(overview).toContain("显示器 1（主）");
    expect(picker).toContain("显示器 1（主）");
  });

  test("overview offers a manual remote-session marker", () => {
    const inactive = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(inactive).toContain("标记为远程会话");

    const active = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={{ ...settings, remote_active: true, remote_evidence: message("手动标记") }}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // The action must flip: a marker that cannot be cleared would claim a session
    // forever.
    expect(active).toContain("结束远程会话标记");
    expect(active).toContain("手动标记");
  });

  test("overview names the application that refused to restore", () => {
    // "A window did not come back" leaves the user hunting through the taskbar; the
    // backend knows which application it was, and the overview used to throw that
    // away.
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, state: "degraded", captured_windows: 2 }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={["clash-verge.exe"]}
      />,
    );
    expect(html).toContain("clash-verge.exe");
    expect(html).toContain("排除这些应用");
  });

  test("overview offers recovery, not a gather, when the engine is in error", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, state: "error" }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // The engine refuses new gathers in this state. Offering the button anyway
    // means the user presses the only prominent action and gets an error.
    expect(html).not.toContain("整理到目标屏");
    expect(html).toContain("尝试恢复");
    expect(html).toContain("救援越界窗口");
  });

  test("overview offers a way out when restoring is impossible", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={{ ...status, state: "degraded", captured_windows: 4, pending_windows: 4 }}
        settings={settings}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    // Without this the exit barrier refuses to quit, forever, and the old copy
    // told the user to pick an option that did not exist.
    expect(html).toContain("保留当前布局");
    expect(html).toContain("恢复本次布局");
  });

  test("overview offers a bulk exclude when several applications refused", () => {
    const html = renderToStaticMarkup(
      <OverviewPage
        status={status}
        settings={{
          ...settings,
          last_refused_apps: ["a.exe", "b.exe", "c.exe"],
          excluded_apps: [],
        }}
        preview={null}
        displays={displays}
        activity={[]}
        progress={null}
        busy={false}
        onPreview={noop}
        onMigrate={noop}
        onRestore={noop}
        onPause={noop}
        onRescue={noop}
        onIdentify={noop}
        onExport={noop}
        diagnosticsPreview={null}
        onPreviewDiagnostics={noop}
        onOpenConfigDir={noop}
        onOpenDisplays={noop}
        onExclude={noop}
        onKeepLayout={noop}
        onDismissError={noop}
        onToggleRemote={noop}
        pendingApps={[]}
      />,
    );
    expect(html).toContain("全部排除");
  });

  test("displays page", () => {
    const html = renderToStaticMarkup(
      <DisplaysPage
        displays={displays}
        settings={settings}
        target={status.target}
        thumbnails={{
          DISPLAY1: { data_url: "data:image/png;base64,AA==", blank: false },
        }}
        busy={false}
        onSelect={noop}
        onIdentify={noop}
        onCycle={noop}
        onSave={noop}
      />,
    );
    expect(html).toContain("使用中");
    expect(html).toContain("无法截取画面");
  });

  test("clearing every rule asks first", () => {
    // Clearing wipes the whole list at once and cannot be undone. The reset row and
    // the history row both require a second click; this one used to fire on the
    // first, which is out of line with the app's own standard.
    //
    // `renderToStaticMarkup` cannot click, so this only pins the unarmed state: the
    // button says 清空规则 and no confirmation is on screen until something arms it.
    // The click path itself was checked in the running window.
    const html = renderToStaticMarkup(
      <ExclusionsPage
        settings={{ ...settings, excluded_apps: ["msedge.exe", "notepad.exe"] }}
        runningApps={apps}
        busy={false}
        onSave={noop}
        onNotice={noop}
      />,
    );
    expect(html).toContain("清空规则");
    expect(html).not.toContain("确认清空");
  });

  test("exclusions page with running apps", () => {
    const html = renderToStaticMarkup(
      <ExclusionsPage
        settings={{ ...settings, excluded_apps: ["msedge.exe"] }}
        runningApps={apps}
        busy={false}
        onSave={noop}
        onNotice={noop}
      />,
    );
    expect(html).toContain("msedge.exe");
    expect(html).toContain("1 条规则");
  });

  test("a recovered settings file is announced, not silently replaced", () => {
    // Falling back to defaults without a word leaves the user staring at an
    // onboarding guide they already finished, with every rule and shortcut gone -
    // indistinguishable from data loss.
    const render = (recovered: boolean) =>
      renderToStaticMarkup(
        <OverviewPage
          status={{ ...status, settings_recovered: recovered }}
          settings={settings}
          preview={preview}
          displays={displays}
          activity={activity}
          progress={null}
          busy={false}
          onPreview={noop}
          onMigrate={noop}
          onRestore={noop}
          onPause={noop}
          onRescue={noop}
          onIdentify={noop}
          onExport={noop}
          diagnosticsPreview={null}
          onPreviewDiagnostics={noop}
          onOpenConfigDir={noop}
          onOpenDisplays={noop}
          onExclude={noop}
          onKeepLayout={noop}
          onDismissError={noop}
          onToggleRemote={noop}
          pendingApps={[]}
        />,
      );

    expect(render(true)).toContain("settings.json.invalid");
    expect(render(false)).not.toContain("settings.json.invalid");
  });

  test("the pending card says whether windows may already have moved", () => {
    // The sentence carries a conditional clause, and the only fixture that set
    // `may_have_moved: true` was used by the "no Chinese in English" sweep, which
    // checks for stray characters rather than for this clause. Neither branch was
    // asserted, so the clause could have been dropped or inverted unnoticed.
    const moved = renderToStaticMarkup(
      <PendingCard
        pending={{ present: true, windows: 3, may_have_moved: true }}
        busy={false}
        onRestore={noop}
        onDismiss={noop}
      />,
    );
    expect(moved).toContain("其中部分可能已被移动");

    const unmoved = renderToStaticMarkup(
      <PendingCard
        pending={{ present: true, windows: 3, may_have_moved: false }}
        busy={false}
        onRestore={noop}
        onDismiss={noop}
      />,
    );
    expect(unmoved).not.toContain("其中部分可能已被移动");
    expect(unmoved).toContain("有 3 个窗口的快照仍待处理。");
  });

  test("a hotkey that failed to register says so on its own row", () => {
    // The health card already lists a failed combination, but that is a summary on a
    // different card. The row next to the recorder is where a user looks after
    // recording a shortcut, and its error branch was never exercised: the fixture
    // always had `gather_error: null`.
    const html = renderToStaticMarkup(
      <GeneralPage
        settings={settings}
        hotkeys={{
          ...hotkeys,
          gather: "Ctrl+Alt+G",
          gather_error: message("这个组合已被其他程序占用"),
        }}
        health={health}
        updateStatus={null}
        busy={false}
        onSave={noop}
        onCheckUpdate={noop}
        onReset={noop}
        onShowGuide={noop}
        onClearHistory={noop}
      />,
    );
    expect(html).toContain("未生效：这个组合已被其他程序占用");
  });

  test("settings page in both update configurations", () => {
    const configured = renderToStaticMarkup(
      <GeneralPage
        settings={settings}
        hotkeys={hotkeys}
        health={health}
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
        onReset={noop}
        onShowGuide={noop}
        onClearHistory={noop}
      />,
    );
    expect(configured).toContain("已配置更新源");
    // A failed hotkey must be visible without pressing it and wondering.
    expect(configured).toContain("Ctrl+Alt+G is already in use");
    expect(configured).toContain("检测到 2 块显示器");
    expect(configured).toContain("invalid updater binary format");

    const unconfigured = renderToStaticMarkup(
      <GeneralPage
        settings={settings}
        hotkeys={hotkeys}
        health={health}
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
        onReset={noop}
        onShowGuide={noop}
        onClearHistory={noop}
      />,
    );
    expect(unconfigured).toContain("未配置更新源");
    // A configuration built over weeks needs a way back; the copy must say the
    // target display survives, or nobody will dare use it.
    expect(unconfigured).toContain("恢复默认设置");
    expect(unconfigured).toContain("目标显示器保留");
  });

  test("onboarding at every step", () => {
    for (const step of [0, 1, 2, 3, 4]) {
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

  test("plan preview warns when the chosen display is gone", () => {
    const html = renderToStaticMarkup(
      <PlanPreview preview={{ ...preview, target_present: false }} />,
    );
    // Falling back silently means the user discovers it by finding their windows
    // on the wrong screen and having to restore.
    expect(html).toContain("会整理到主显示器");
  });

  test("plan preview names the applications instead of only numbering them", () => {
    const html = renderToStaticMarkup(<PlanPreview preview={preview} />);
    expect(html).toContain("<svg");
    // A grid of numbers tells the user nothing about where their work lands.
    expect(html).toContain("notepad");
    // Two windows of one application must be distinguishable, because
    // "which explorer window goes where" is the question the drawing exists for.
    expect(html).toContain("notepad #1");
    expect(html).toContain("notepad #2");
  });

  test("plan preview truncates a long application name instead of clipping it", () => {
    const html = renderToStaticMarkup(
      <PlanPreview
        preview={{
          ...preview,
          placements: [
            {
              window_id: "1:7",
              app_name: "WindowsTerminal.exe",
              order: 1,
              rect: { left: 0, top: 0, width: 100, height: 100 },
              clamped: false,
            },
          ],
        }}
      />,
    );
    // A clipped word looks like a rendering bug; an ellipsis does not.
    expect(html).toContain("WindowsTe…");
  });

  test("plan preview falls back to a number when the app name is unreadable", () => {
    const html = renderToStaticMarkup(
      <PlanPreview
        preview={{
          ...preview,
          placements: [
            {
              window_id: "1:9",
              app_name: "",
              order: 1,
              rect: { left: 0, top: 0, width: 100, height: 100 },
              clamped: false,
            },
          ],
        }}
      />,
    );
    expect(html).toContain("窗口 1");
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

/**
 * English mode.
 *
 * A page that renders Chinese while the interface is English is invisible to a
 * source scan: `title: "这个工具做什么"` is a valid catalogue key *and* raw copy, so
 * asking "is this string in the dictionary" answers the wrong question. The
 * onboarding titles passed that check for months while rendering in Chinese.
 *
 * Rendering is the only check that asks the right one.
 */
describe("English mode", () => {
  // Ideographs plus CJK punctuation: a full-width parenthesis or an enumeration
  // comma in an English interface is as wrong as a Chinese word, and the
  // ideograph range alone does not include them.
  const CJK = /[\u3000-\u303f\u4e00-\u9fff\uff00-\uffef]/;
  // The language picker names each language in its own script, on purpose.
  const intentional = ["中文"];
  const strip = (html: string) =>
    intentional.reduce((acc, word) => acc.split(word).join(""), html);

  const original = currentLocale();

  const pages: [string, () => string][] = [
    [
      "overview",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={status}
            settings={settings}
            preview={preview}
            displays={displays}
            activity={activity}
            progress={{ done: 2, total: 5 }}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "displays",
      () =>
        renderToStaticMarkup(
          <DisplaysPage
            displays={displays}
            settings={settings}
            // One live capture and one black one, so the "no picture" note is part
            // of what this test checks.
            thumbnails={{
              [displays[0]!.native_id]: {
                data_url: "data:image/png;base64,AA==",
                blank: true,
              },
            }}
            busy={false}
            onSelect={noop}
            onIdentify={noop}
            onSave={noop}
            onRefresh={noop}
          />,
        ),
    ],
    [
      "exclusions",
      () =>
        renderToStaticMarkup(
          <ExclusionsPage
            settings={{ ...settings, excluded_apps: ["msedge.exe"] }}
            runningApps={apps}
            busy={false}
            onSave={noop}
            onNotice={noop}
          />,
        ),
    ],
    [
      "settings",
      () =>
        renderToStaticMarkup(
          <GeneralPage
            settings={settings}
            hotkeys={hotkeys}
            health={health}
            updateStatus={{
              current_version: "0.1.0",
              auto_update: true,
              configured: true,
              last_outcome: null,
              last_version: null,
              last_detail: null,
            }}
            busy={false}
            onSave={noop}
            onCheckUpdate={noop}
            onReset={noop}
            onShowGuide={noop}
            onClearHistory={noop}
        onClearHistory={noop}
          />,
        ),
    ],
    ...([0, 1, 2, 3, 4] as const).map(
      (step) =>
        [
          `onboarding step ${step + 1}`,
          () =>
            renderToStaticMarkup(
              <Onboarding
                displays={displays}
                settings={{
                  ...settings,
                  onboarding_done: false,
                  onboarding_step: step,
                }}
                thumbnails={{
                  [displays[0]!.native_id]: {
                    data_url: "data:image/png;base64,AA==",
                    blank: false,
                  },
                }}
                busy={false}
                onSelectTarget={async () => {}}
                onIdentify={noop}
                onSetDetect={async () => {}}
                onFinish={noop}
              />,
            ),
        ] as [string, () => string],
    ),
    [
      "overview with an active layout",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{
              ...status,
              state: "active",
              captured_windows: 5,
              pending_windows: 5,
              target: { native_id: "DISPLAY2", label: "DISPLAY2", present: true },
            }}
            settings={{ ...settings, remote_active: false }}
            preview={null}
            displays={displays}
            activity={[
              {
                at: Math.floor(Date.now() / 1000) - 120,
                kind: "gather",
                affected: 5,
                unsettled: 0,
                note: message("先恢复了 {count} 个窗口", { count: "5" }),
                trigger: "window",
              },
            ]}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "overview degraded with a lost target",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{ ...status, state: "degraded", captured_windows: 4, target: { native_id: "GONE", label: "DISPLAY9", present: false } }}
            settings={{ ...settings, remote_active: true, remote_evidence: message("{name} 已连接", { name: "AnyDesk" }) }}
            preview={preview}
            displays={displays}
            activity={activity}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "overview reporting a failure",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{ ...status, state: "error", last_error: message("journal write failed: io error: os error 183") }}
            settings={settings}
            preview={null}
            displays={displays}
            activity={[]}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "overview while gathering",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={status}
            settings={settings}
            preview={null}
            displays={displays}
            activity={[]}
            progress={{ done: 3, total: 7 }}
            busy={true}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "overview with one display",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{ ...status, display_count: 1 }}
            settings={settings}
            preview={null}
            displays={[displays[0]!]}
            activity={[]}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "overview with refused applications",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{ ...status, last_refused_apps: ["msedge.exe", "notepad.exe"], excluded_apps: ["notepad.exe"] }}
            settings={settings}
            preview={null}
            displays={displays}
            activity={[]}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "permission warning",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{
              ...status,
              has_permission: false,
              permission_name: "辅助功能",
            }}
            settings={settings}
            preview={null}
            displays={displays}
            activity={[]}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "overview on a platform without window control",
      () =>
        renderToStaticMarkup(
          <OverviewPage
            status={{ ...status, supported: false, display_count: 0 }}
            settings={settings}
            preview={null}
            displays={displays}
            activity={[]}
            progress={null}
            busy={false}
            onPreview={noop}
            onMigrate={noop}
            onRestore={noop}
            onPause={noop}
            onRescue={noop}
            onIdentify={noop}
            onExport={noop}
            diagnosticsPreview={null}
            onPreviewDiagnostics={noop}
            onOpenConfigDir={noop}
            onOpenDisplays={noop}
            onExclude={noop}
            onKeepLayout={noop}
            onDismissError={noop}
            onToggleRemote={noop}
            pendingApps={[]}
          />,
        ),
    ],
    [
      "settings on a build without an update source",
      () =>
        renderToStaticMarkup(
          <GeneralPage
            settings={settings}
            hotkeys={hotkeys}
            health={health}
            updateStatus={{
              current_version: "0.1.0",
              auto_update: false,
              configured: false,
              last_outcome: null,
              last_version: null,
              last_detail: null,
            }}
            busy={false}
            onSave={noop}
            onCheckUpdate={noop}
            onReset={noop}
            onShowGuide={noop}
            onClearHistory={noop}
        onClearHistory={noop}
          />,
        ),
    ],
    [
      "settings after a failed update check",
      () =>
        renderToStaticMarkup(
          <GeneralPage
            settings={settings}
            hotkeys={hotkeys}
            health={health}
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
            onReset={noop}
            onShowGuide={noop}
            onClearHistory={noop}
        onClearHistory={noop}
          />,
        ),
    ],
    [
      "permission prompt",
      () =>
        renderToStaticMarkup(
          <PermissionPrompt
            status={{ ...status, has_permission: false, permission_name: "辅助功能" }}
            busy={false}
            onRequest={noop}
            onSettings={noop}
            onRecheck={noop}
          />,
        ),
    ],
    [
      "pending work card",
      () =>
        renderToStaticMarkup(
          <PendingCard
            pending={{ present: true, windows: 3, may_have_moved: true }}
            busy={false}
            onRestore={noop}
            onDismiss={noop}
          />,
        ),
    ],
    [
      "plan preview",
      () => renderToStaticMarkup(<PlanPreview preview={preview} />),
    ],
  ];

  for (const [name, render] of pages) {
    test(`${name} renders no Chinese`, () => {
      applyLocale("en");
      try {
        const html = strip(render());
        const offenders = [...html.matchAll(/>([^<]*[\u3000-\u303f\u4e00-\u9fff\uff00-\uffef][^<]*)</g)].map(
          (m) => m[1]!.trim(),
        );
        expect(offenders).toEqual([]);
      } finally {
        applyLocale(original);
      }
    });
  }

  // `api.ts` holds the label tables and the sentinel mapper, none of which a page
  // render covers: they are consumed by whichever component needs them.
  test("every table entry and error sentinel is translated", () => {
    applyLocale("en");
    try {
      for (const value of [
        ...Object.values(stateLabels),
        ...Object.values(skipLabels),
      ]) {
        expect([value, t(value)]).toEqual([value, expect.not.stringMatching(CJK)]);
      }
      // Sentinels the Rust side returns instead of a sentence. Both mappers are
      // checked: which one a caller uses depends on whether the failure is the
      // result of the button just pressed or a condition already in effect.
      for (const sentinel of [
        "already_active",
        "paused",
        "no_displays",
        "no_displays_found",
        "restore_cancelled",
        "restore_incomplete:3",
        "TargetDisplayMissing",
        "PermissionDenied",
        "journal write failed: io error",
      ]) {
        for (const [name, rendered] of [
          ["errorMessage", errorMessage(sentinel)],
          ["humanError", humanError(sentinel)],
        ] as const) {
          expect([name, sentinel, rendered]).toEqual([
            name,
            sentinel,
            expect.not.stringMatching(CJK),
          ]);
        }
      }
    } finally {
      applyLocale(original);
    }
  });

  // The sidebar is the one surface the page renders above cannot reach: its labels
  // live in a module-scope array and are translated at render time, because a
  // module-scope `t()` would be evaluated once at import and never follow a
  // language change. So the entries are checked one by one instead.
  test("every sidebar label, group and state has an English entry", () => {
    const labels = [
      ...navigation.map((item) => item.label),
      ...navigation.map((item) => item.group),
      ...Object.values(stateLabels),
    ];
    applyLocale("en");
    try {
      for (const label of labels) {
        // A missing entry falls back to the Chinese source, which is the bug.
        expect([label, t(label)]).toEqual([label, expect.not.stringMatching(CJK)]);
      }
    } finally {
      applyLocale(original);
    }
  });
});
