/**
 * Typed wrappers around the Tauri command surface.
 *
 * The Rust side is the only source of truth; these types mirror the serde shapes
 * in `apps/desktop/src-tauri/src/commands.rs`. When the two drift, the UI shows
 * `undefined` at runtime, so every field is declared optional where the Rust
 * struct could legitimately omit it.
 */

import { invoke } from "@tauri-apps/api/core";

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
}

export interface StatusDto {
  state: EngineStateName;
  platform: string;
  supported: boolean;
  /** False on macOS before Accessibility permission is granted. */
  has_permission: boolean;
  permission_name: string;
  display_count: number;
  target: TargetDto | null;
  pending_windows: number;
  last_error: string | null;
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
}

export interface RectDto {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface PlacementDto {
  window_id: string;
  label: string;
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
  target: TargetDto | null;
  /** Live detector verdict. Not persisted. */
  remote_active: boolean;
  remote_evidence: string | null;
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
}

export interface HotkeyStatusDto {
  gather: string | null;
  gather_error: string | null;
  restore: string | null;
  restore_error: string | null;
}

export interface UpdateStatusDto {
  current_version: string;
  auto_update: boolean;
  /** False when this build has no updater endpoint or signing key. */
  configured: boolean;
  last_outcome: string | null;
  last_version: string | null;
}

export interface ActivityEntryDto {
  at: number;
  kind: "gather" | "restore" | "rescue" | "auto_gather" | "auto_restore" | "pause";
  affected: number;
  unsettled: number;
  note: string;
  trigger: string;
}

export interface DisplayThumbnailDto {
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
  checkUpdate: () => invoke<string>("check_update"),
  liveTransaction: () => invoke<string | null>("live_transaction"),
  setPaused: (paused: boolean) => invoke<PauseDto>("set_paused", { paused }),
  hotkeyStatus: () => invoke<HotkeyStatusDto>("hotkey_status"),
  identifyDisplays: (seconds?: number) =>
    invoke<boolean>("identify_displays", { seconds }),
  displayThumbnails: () =>
    invoke<DisplayThumbnailDto[]>("display_thumbnails"),
  runningApplications: () => invoke<AppDto[]>("running_applications"),
  activityLog: () => invoke<ActivityEntryDto[]>("activity_log"),
};

/** Human-readable Chinese labels for the engine states shown in the UI. */
export const stateLabels: Record<EngineStateName, string> = {
  idle: "就绪",
  planning: "正在计划",
  migrating: "正在整理窗口",
  active: "布局已启用",
  restoring: "正在恢复",
  degraded: "部分受限",
  error: "需要处理",
};

/** One-sentence explanation of what the state means for the user right now. */
export const stateHints: Record<EngineStateName, string> = {
  idle: "窗口保持原位。按快捷键或点「整理到目标屏」开始。",
  planning: "正在计算布局，还没有移动任何窗口。",
  migrating: "正在把窗口移到目标屏。",
  active: "窗口已收拢到目标屏。结束后点「恢复本次布局」。",
  restoring: "正在把窗口放回原来的位置。",
  degraded: "部分窗口没能处理，详情见下方结果。",
  error: "出现了无法自动处理的问题，请查看提示。",
};

/** Chinese labels for skip reasons returned by the engine. */
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
  return skipLabels[reason] ?? reason;
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
    return `还有 ${incomplete[1]} 个窗口没能恢复原位，已停止重新整理。请再点一次「恢复本次布局」，或先关闭这些窗口。`;
  }
  if (raw.includes("already_active")) {
    return "窗口仍处于整理状态。请先点「恢复本次布局」，再重新整理到新的目标屏。";
  }
  if (raw.includes("restore_cancelled")) {
    return "已取消这次恢复，因为期间开始了新的整理。";
  }
  if (raw.includes("paused")) {
    return "自动整理已暂停。在托盘菜单或概览页取消暂停后重试。";
  }
  if (raw.includes("TargetDisplayMissing") || raw.includes("no_displays")) {
    return "目标显示器当前不可用，请在「显示器与布局」里重新选择。";
  }
  if (raw.includes("PermissionDenied") || raw.includes("permission")) {
    return "系统拒绝了窗口操作权限，请检查授权后重试。";
  }
  return raw;
}

/** Format a byte count for the diagnostics line. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}
