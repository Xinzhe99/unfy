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
}

export interface DisplayDto {
  native_id: string;
  label: string;
  width: number;
  height: number;
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
  reason: string;
}

export interface PreviewDto {
  target_present: boolean;
  placements: PlacementDto[];
  unplaced: string[];
  skipped: SkipDto[];
  cascaded: boolean;
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
  /** Process ids of applications that refused to move. */
  refused_pids: number[];
}

export interface RestoreDto {
  restored: number;
  kept_user_change: number;
  fallback_visible: number;
  rejected: number;
  pending: number;
  state: EngineStateName;
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
  target: TargetDto | null;
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

export interface UpdateStatusDto {
  current_version: string;
  auto_update: boolean;
  last_outcome: string | null;
  last_version: string | null;
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
};

/** Human-readable Chinese labels for the engine states shown in the UI. */
export const stateLabels: Record<EngineStateName, string> = {
  idle: "未整理",
  planning: "正在计划",
  migrating: "正在整理窗口",
  active: "布局已启用",
  restoring: "正在恢复",
  degraded: "部分受限",
  error: "需要处理",
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
};

export function skipLabel(reason: string): string {
  return skipLabels[reason] ?? reason;
}
