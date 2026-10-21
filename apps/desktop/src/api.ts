/**
 * Typed wrappers around the Tauri command surface.
 *
 * The Rust side is the only source of truth; these types mirror the serde shapes
 * in `apps/desktop/src-tauri/src/commands.rs`. When the two drift, the UI shows
 * `undefined` at runtime, so every field is declared optional where the Rust
 * struct could legitimately omit it.
 */

import { invoke } from "@tauri-apps/api/core";

import { t } from "./i18n";

export type EngineStateName =
  | "idle"
  | "planning"
  | "migrating"
  | "active"
  | "restoring"
  | "degraded"
  | "error";

export interface TargetDto {
  native_id: string;
  label: string;
  present: boolean;
  /** User-facing number; the saved value when the display is currently absent. */
  number: number | null;
}

export interface StatusDto {
  state: EngineStateName;
  platform: string;
  supported: boolean;
  /** False on macOS before Accessibility permission is granted. */
  has_permission: boolean;
  /** A catalogued sentence naming the permission, translated by the caller. */
  permission_name: string;
  display_count: number;
  target: TargetDto | null;
  pending_windows: number;
  /** The most recent failure, as a sentence or an engine sentinel. */
  last_error: MessageDto | null;
  paused: boolean;
  captured_windows: number;
}

export interface DisplayDto {
  native_id: string;
  label: string;
  width: number;
  height: number;
  /** Position in the virtual desktop; used to draw the real arrangement. */
  origin_x: number;
  origin_y: number;
  work_width: number;
  work_height: number;
  scale: number;
  primary: boolean;
  restorable: boolean;
  is_target: boolean;
  /** 1-based number shown to the user; matches the badge on the physical screen. */
  number: number;
}

export interface RectDto {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface PlacementDto {
  window_id: string;
  /** Executable name of the owning application, or empty when unreadable. */
  app_name: string;
  /** 1-based position in the plan; the preview numbers rectangles the same way. */
  order: number;
  rect: RectDto;
  clamped: boolean;
}

export interface SkipDto {
  window_id: string;
  /** Executable name, or empty when the platform could not read it. */
  app_name: string;
  reason: string;
}

export interface PreviewDto {
  target_present: boolean;
  placements: PlacementDto[];
  unplaced: string[];
  skipped: SkipDto[];
  cascaded: boolean;
  /** Work area of the target, so the preview can draw the real bounds. */
  target_work: RectDto;
}

export interface MigrationDto {
  transaction: string;
  applied: number;
  rejected: number;
  gone: number;
  skipped: number;
  unplaced: number;
  cascaded: boolean;
  state: EngineStateName;
  /** Names of applications that refused to move. */
  refused_apps: string[];
}

export interface RestoreDto {
  restored: number;
  kept_user_change: number;
  fallback_visible: number;
  rejected: number;
  pending: number;
  state: EngineStateName;
  /** Applications whose windows are still unresolved. */
  pending_apps: string[];
}

export interface SettingsDto {
  layout_mode: "grid" | "primary_side" | "preserve_order";
  margin: number;
  gap: number;
  autostart: boolean;
  theme: "system" | "light" | "dark";
  /** Interface language; `system` follows the OS. */
  language: "system" | "zh" | "en";
  hotkey_gather: string | null;
  hotkey_restore: string | null;
  restore_delay_secs: number;
  auto_update: boolean;
  telemetry: boolean;
  excluded_apps: string[];
  onboarding_done: boolean;
  onboarding_step: number;
  /** Evidence level the app may act on automatically. */
  auto_detect: "off" | "conservative" | "balanced" | "aggressive";
  auto_gather: boolean;
  auto_restore: boolean;
  /** What the remote client can display: one screen, or all of them. */
  remote_screens: "single" | "all";
  /** Applications that refused the last gather. */
  last_refused_apps: string[];
  target: TargetDto | null;
  /** Live detector verdict. Not persisted. */
  remote_active: boolean;
  remote_evidence: MessageDto | null;
}

export interface PendingWorkDto {
  present: boolean;
  windows: number;
  may_have_moved: boolean;
  truncated_bytes: number;
}

export interface DiagnosticsDto {
  path: string;
  bytes: number;
}

export interface PauseDto {
  paused: boolean;
  state: EngineStateName;
  /** Resume also ran a restore that the pause had deferred. */
  restored_deferred: boolean;
}

export interface HotkeyStatusDto {
  gather: string | null;
  gather_error: MessageDto | null;
  restore: string | null;
  restore_error: MessageDto | null;
}

export interface UpdateStatusDto {
  current_version: string;
  auto_update: boolean;
  /** False when this build has no updater endpoint or signing key. */
  configured: boolean;
  last_outcome: string | null;
  last_version: string | null;
  /** Why the last check failed, when it did. */
  last_detail: string | null;
}

export interface UpdateCheckDto {
  outcome: "up_to_date" | "deferred" | "installed" | "failed";
  /** The failure reason, empty unless `outcome` is `failed`. */
  detail: string;
}

/**
 * A sentence the backend chose, worded here.
 *
 * The backend cannot format these: it does not know the interface language, and a
 * sentence assembled there reached an English window in Chinese. `key` is the
 * Chinese source sentence and also the catalogue key.
 */
export interface MessageDto {
  key: string;
  /** Values for `{name}` placeholders. Empty when the sentence has none. */
  params: Record<string, string>;
}

export interface HealthItemDto {
  id: string;
  label: MessageDto;
  status: "ok" | "attention" | "problem";
  detail: MessageDto;
}

export interface ActivityEntryDto {
  at: number;
  kind:
    | "gather"
    | "restore"
    | "rescue"
    | "auto_gather"
    | "auto_restore"
    | "pause"
    | "keep_layout";
  affected: number;
  unsettled: number;
  /** What counts alone cannot say, or `null`. */
  note: MessageDto | null;
  trigger: string;
}

export interface DisplayThumbnailDto {
  /** True when the capture is solid black: the display is showing nothing. */
  blank: boolean;
  native_id: string;
  /** `data:image/png;base64,...` ready for an `<img src>`. */
  data_url: string;
}

export interface AppDto {
  name: string;
  /** True when an exclusion rule already covers this application. */
  excluded: boolean;
}

export const api = {
  getStatus: () => invoke<StatusDto>("get_status"),
  listDisplays: () => invoke<DisplayDto[]>("list_displays"),
  preview: () => invoke<PreviewDto>("preview"),
  migrate: () => invoke<MigrationDto>("migrate"),
  restore: () => invoke<RestoreDto>("restore"),
  rescueOffscreen: () => invoke<number>("rescue_offscreen"),
  getSettings: () => invoke<SettingsDto>("get_settings"),
  saveSettings: (request: Partial<SettingsDto>) =>
    invoke<SettingsDto>("save_settings", { request }),
  setTargetDisplay: (nativeId: string) =>
    invoke<void>("set_target_display", { nativeId }),
  pendingWork: () => invoke<PendingWorkDto>("pending_work"),
  dismissPendingWork: () => invoke<void>("dismiss_pending_work"),
  exportDiagnostics: () => invoke<DiagnosticsDto>("export_diagnostics"),
  /** Returns the permission state *before* the user answers; re-check after. */
  requestPermission: () => invoke<boolean>("request_permission"),
  openPermissionSettings: () => invoke<boolean>("open_permission_settings"),
  updateStatus: () => invoke<UpdateStatusDto>("update_status"),
  checkUpdate: () => invoke<UpdateCheckDto>("check_update"),
  setPaused: (paused: boolean) => invoke<PauseDto>("set_paused", { paused }),
  hotkeyStatus: () => invoke<HotkeyStatusDto>("hotkey_status"),
  identifyDisplays: (seconds?: number) =>
    invoke<boolean>("identify_displays", { seconds }),
  displayThumbnails: () =>
    invoke<DisplayThumbnailDto[]>("display_thumbnails"),
  runningApplications: () => invoke<AppDto[]>("running_applications"),
  activityLog: () => invoke<ActivityEntryDto[]>("activity_log"),
  healthCheck: () => invoke<HealthItemDto[]>("health_check"),
  keepCurrentLayout: () => invoke<void>("keep_current_layout"),
  dismissLastError: () => invoke<void>("dismiss_last_error"),
  resetSettings: () => invoke<SettingsDto>("reset_settings"),
  /**
   * Reveal a directory this app owns. Omit `path` for the config directory;
   * an explicit path is validated against that root on the backend.
   */
  openPath: (path?: string) => invoke<boolean>("open_path", { path: path ?? null }),
  /**
   * Tell the backend which language the interface is rendering in.
   *
   * The tray and its tooltip are built in Rust and cannot read the catalogue here,
   * so they are told the resolved locale instead of resolving it a second way.
   */
  setUiLocale: (locale: string) => invoke<void>("set_ui_locale", { locale }),
  /** Move the target to the next display and re-gather onto it. */
  cycleTarget: () => invoke<TargetDto>("cycle_target"),
  /** Exactly what an export would contain, without writing it. */
  previewDiagnostics: () => invoke<string>("preview_diagnostics"),
  /** Declare or end a remote session by hand; returns the new active state. */
  toggleRemoteSession: () => invoke<boolean>("toggle_remote_session"),
};

/**
 * Engine state labels, as catalogue keys.
 *
 * The values are the Chinese source strings the catalogue is keyed by, not the
 * text to display: callers pass them through `t()`. Keeping the map here rather
 * than duplicating it in each component means a new state has one place to be
 * translated.
 */
export const stateLabels: Record<EngineStateName, string> = {
  idle: "就绪",
  planning: "正在计划",
  migrating: "正在整理窗口",
  active: "布局已启用",
  restoring: "正在恢复",
  degraded: "部分受限",
  error: "需要处理",
};

/** Skip reasons, as catalogue keys. `skipLabel` translates them. */
export const skipLabels: Record<string, string> = {
  own_window: "本应用的窗口",
  invisible: "不可见",
  zero_size: "尺寸为零",
  tool_window: "工具窗口",
  shell_window: "系统外壳",
  modal_child: "附属于主窗口",
  fullscreen: "全屏",
  other_virtual_desktop: "其他虚拟桌面",
  permission_denied: "权限不足",
  minimized: "已最小化",
  excluded_app: "已排除此应用",
};

export function skipLabel(reason: string): string {
  // An unknown reason is a sentinel the engine added and the catalogue has not
  // caught up with; show it rather than an empty label.
  return t(skipLabels[reason] ?? reason);
}

/**
 * Word a sentence the backend chose, in the current language.
 *
 * Parameters are translated as well as the sentence: some of them are themselves
 * catalogued phrases (`辅助功能` arrives as the name of a permission), while most
 * are values - a count, a path, an operating-system error - that `t()` returns
 * unchanged because they are not keys.
 */
/**
 * Word the failure the engine is currently reporting, for the overview.
 *
 * Two mappers live here because the two situations want different copy.
 * `errorMessage` answers "the button you just pressed failed"; this one answers
 * "something went wrong earlier and is still true", where the user is not looking
 * at the action that caused it. Both take the same kind of raw value: an internal
 * code (`already_active`) or an operating-system error.
 */
export function humanError(raw: string): string {
  const incomplete = raw.match(/restore_incomplete:(\d+)/);
  if (incomplete) return t("还有 {count} 个窗口没能恢复原位", { count: incomplete[1]! });
  // Journal failures are the one path that leaves the engine unable to act at
  // all, and the raw text is an English OS error. Say what it means and what the
  // user can do; the exact reason is in the diagnostics bundle.
  if (raw.includes("journal write failed") || raw.includes("journal is corrupt")) {
    return t("无法写入恢复记录，为避免窗口位置丢失，整理已停止。请确认磁盘空间与目录权限，然后重启应用。");
  }
  switch (raw) {
    case "already_active":
      return t("还有未完成的整理，请先恢复");
    case "paused":
      return t("自动整理已暂停");
    case "no_displays":
      return t("没有可用的显示器");
    case "restore_cancelled":
      return t("恢复已被新的整理请求取消");
    case "no_displays_found":
      return t("没有检测到显示器");
    default:
      return raw;
  }
}

export function renderMessage(message: MessageDto): string {
  const params: Record<string, string | number> = {};
  for (const [name, value] of Object.entries(message.params)) {
    params[name] = t(value);
  }
  return t(message.key, params);
}

/**
 * Word a backend note that may be a sentinel rather than a sentence.
 *
 * Engine failures arrive as sentinels (`restore_incomplete:4`) that need a specific
 * next step, so they go through `humanError`; anything with parameters is a
 * sentence the backend wrote.
 */
export function renderNotice(note: MessageDto): string {
  if (Object.keys(note.params).length > 0) return renderMessage(note);
  return humanError(note.key);
}

/**
 * Turn an engine error into something a user can act on.
 *
 * The Rust side returns sentinels for the cases that need a specific next step;
 * everything else is passed through unchanged rather than being flattened into a
 * generic failure message.
 */
export function errorMessage(error: unknown): string {
  const raw = String(error);
  // Retargeting refuses when the restore it runs first left windows behind.
  // Saying "整理失败" would hide the actionable part: those windows are still
  // out, and trying again is the fix.
  const incomplete = raw.match(/restore_incomplete:(\d+)/);
  if (incomplete) {
    return t(
      "还有 {count} 个窗口没能恢复原位，已停止重新整理。请再点一次「恢复本次布局」，或先关闭这些窗口。",
      { count: incomplete[1]! },
    );
  }
  if (raw.includes("already_active")) {
    return t("窗口仍处于整理状态。请先点「恢复本次布局」，再重新整理到新的目标屏。");
  }
  if (raw.includes("restore_cancelled")) {
    return t("已取消这次恢复，因为期间开始了新的整理。");
  }
  if (raw.includes("paused")) {
    return t("自动整理已暂停。在托盘菜单或概览页取消暂停后重试。");
  }
  if (raw.includes("TargetDisplayMissing") || raw.includes("no_displays")) {
    return t("目标显示器当前不可用，请在「显示器与布局」里重新选择。");
  }
  if (raw.includes("PermissionDenied") || raw.includes("permission")) {
    return t("系统拒绝了窗口操作权限，请检查授权后重试。");
  }
  return raw;
}

/** Format a byte count for the diagnostics line. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}
