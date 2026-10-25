import { useState } from "react";

import type {
  HealthItemDto,
  HotkeyStatusDto,
  SettingsDto,
  UpdateStatusDto,
} from "../api";
import { Button, Card, Segmented, SettingRow, Status, Switch } from "../components/ui";
import { HotkeyRecorder } from "../components/HotkeyRecorder";
import { t } from "../i18n";
import { renderMessage } from "../api";
import { IconDownload } from "../components/icons";

/**
 * Settings.
 *
 * Each row states the consequence, not the mechanism: "关闭窗口后仍在后台" rather
 * than "注册托盘图标". A settings screen that repeats the feature name teaches
 * nothing.
 */
export function GeneralPage({
  settings,
  hotkeys,
  health,
  updateStatus,
  busy,
  onSave,
  onCheckUpdate,
  onReset,
  onShowGuide,
  onClearHistory,
  platform,
  onInstallUpdate,
  diagnosticsPreview,
  onPreviewDiagnostics,
  onExport,
  onOpenConfigDir,
}: {
  settings: SettingsDto | null;
  hotkeys: HotkeyStatusDto | null;
  health: HealthItemDto[];
  updateStatus: UpdateStatusDto | null;
  busy: boolean;
  onSave: (patch: Partial<SettingsDto>) => void;
  onCheckUpdate: () => void;
  /** Restore shipped defaults, keeping the target display. */
  onReset: () => void;
  /** Walk through the first-run guide again. */
  onShowGuide: () => void;
  /** Forget the activity history (spec P0-15: it is the user's record to erase). */
  onClearHistory: () => void;
  platform?: string;
  onInstallUpdate?: () => void;
  diagnosticsPreview?: string | null;
  onPreviewDiagnostics?: () => void;
  onExport?: () => void;
  onOpenConfigDir?: () => void;
}) {
  // The reset row is the one destructive action in the app; arming it is local
  // state so a stray click cannot wipe a configuration built over weeks.
  const [resetArmed, setResetArmed] = useState(false);
  // Same two-step as the reset row: erasing the history is not destructive to the
  // configuration, but it is not undoable either.
  const [historyArmed, setHistoryArmed] = useState(false);

  if (!settings) {
    return <p className="text-xs text-[var(--color-muted)]">{t("正在读取设置…")}</p>;
  }

  return (
    <>
      <Card title={t("外观")}>
        <SettingRow
          label={t("界面语言")}
          hint={t("跟随系统语言，可在需要时手动指定")}
        >
          <Segmented
            label={t("界面语言")}
            value={settings.language}
            disabled={busy}
            onChange={(language) => onSave({ language })}
            options={[
              { value: "system", label: t("跟随系统") },
              { value: "zh", label: "中文" },
              { value: "en", label: "English" },
            ]}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label={t("主题")} hint={t("跟随系统会在系统切换深浅色时自动变化")}>
          <Segmented
            label={t("主题")}
            value={settings.theme}
            disabled={busy}
            onChange={(theme) => onSave({ theme })}
            options={[
              { value: "system", label: t("跟随系统") },
              { value: "light", label: t("浅色") },
              { value: "dark", label: t("深色") },
            ]}
          />
        </SettingRow>
      </Card>

      <Card
        title={t("远程会话检测")}
        description={t("已验证的 RDP 连接仅找回越界窗口，不重排可见窗口，也不会自动恢复手动整理。其他工具请手动整理和恢复。")}
        aside={
          settings.remote_unknown ? <Status tone="warning">{t("连接状态未知，自动操作已停止")}</Status> : settings.remote_active ? (
            <Status tone="accent">
              {settings.remote_evidence
                ? renderMessage(settings.remote_evidence)
                : t("检测到远程会话")}
            </Status>
          ) : null
        }
      >
        <SettingRow
          label={t("远程会话检测")}
          hint={detectHint(settings.auto_detect)}
        >
          <Segmented
            label={t("远程会话检测")}
            value={settings.auto_detect === "verified" && settings.auto_gather ? "verified" : "off"}
            disabled={busy || platform !== "windows"}
            onChange={(auto_detect) => onSave({ auto_detect, auto_gather: auto_detect === "verified", remote_screens: "single" })}
            options={[
              { value: "off", label: t("手动") },
              { value: "verified", label: t("可靠连接触发") },
            ]}
          />
        </SettingRow>


        <p className="pt-1 text-[11px] leading-relaxed text-[var(--color-muted)]">
          {t(platform === "windows" ? "只有已验证的远程桌面（RDP）连接能触发自动操作。远控进程、已连接端口和虚拟显示器都只提供信息。" : "当前平台仅支持手动整理与恢复，自动连接检测不可用。")}
        </p>
      </Card>

      <Card title={t("启动与运行")}>
        <SettingRow label={t("开机自动启动")} hint={t("登录后静默启动，不打开窗口")}>
          <Switch
            checked={settings.autostart_actual === true}
            disabled={busy}
            label={t("开机自动启动")}
            onChange={(autostart) => onSave({ autostart })}
          />
        </SettingRow>
        {settings.autostart_actual === null || settings.autostart_actual !== settings.autostart || settings.autostart_error ? (
          <div role={settings.autostart_error ? "alert" : "status"} className="space-y-1 pb-2 text-xs text-[var(--color-fg-soft)]">
            <p>{t(settings.autostart ? "期望开机启动：开启" : "期望开机启动：关闭")}</p>
            <p>{t(settings.autostart_actual === null ? "系统启动注册状态无法确认" : settings.autostart_actual ? "系统实际启动注册：已开启" : "系统实际启动注册：未开启")}</p>
            {settings.autostart_error ? <p className="break-all">{settings.autostart_error}</p> : null}
            <Button size="sm" disabled={busy} onClick={() => onSave({ autostart: settings.autostart })}>{t("重新应用启动设置")}</Button>
          </div>
        ) : null}

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label={t("自动更新")} hint={t("自动下载、安装并重启 Unfy；远程连接或恢复工作未结束时会等待。") }>
          <Switch
            checked={settings.auto_update}
            disabled={busy || !updateStatus?.configured}
            label={t("自动更新")}
            onChange={(auto_update) => onSave({ auto_update })}
          />
        </SettingRow>
      </Card>

      <Card
        title={t("快捷键")}
        aside={
          !settings.hotkey_gather && !settings.hotkey_restore ? (
            <Button
              size="sm"
              variant="quiet"
              disabled={busy}
              onClick={() =>
                // The shipped default is "no shortcut" so the app never takes a
                // combination the user already relies on. That is right, but it
                // also means a new user never discovers the feature. This offers
                // the pair without forcing it.
                onSave({
                  hotkey_gather: "CmdOrCtrl+Alt+G",
                  hotkey_restore: "CmdOrCtrl+Alt+R",
                })
              }
            >{t("使用建议组合")}</Button>
          ) : null
        }
      >
        <SettingRow
          label={t("整理窗口")}
          hint={
            hotkeys?.gather_error
              ? t("未生效：{reason}", {
                  reason: renderMessage(hotkeys.gather_error),
                })
              : t("在其他应用中按下也能触发")
          }
        >
          <HotkeyRecorder
            value={settings.hotkey_gather}
            invalid={Boolean(hotkeys?.gather_error)}
            disabled={busy}
            onChange={(hotkey_gather) => onSave({ hotkey_gather })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow
          label={t("恢复布局")}
          hint={
            hotkeys?.restore_error
              ? t("未生效：{reason}", {
                  reason: renderMessage(hotkeys.restore_error),
                })
              : t("把窗口放回整理前的位置")
          }
        >
          <HotkeyRecorder
            value={settings.hotkey_restore}
            invalid={Boolean(hotkeys?.restore_error)}
            disabled={busy}
            onChange={(hotkey_restore) => onSave({ hotkey_restore })}
          />
        </SettingRow>
        <p className="pt-1 text-[11px] leading-relaxed text-[var(--color-muted)]">{t("点一下再按组合键即可录制，Esc 取消，Backspace 清除。被系统或其他应用占用时会在这里显示原因。")}</p>
      </Card>


      <Card
        title={t("更新")}
        aside={
          updateStatus?.configured ? (
            <Status tone="neutral">{t("已配置更新源")}</Status>
          ) : (
            <Status tone="warning">{t("未配置更新源")}</Status>
          )
        }
      >
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <p className="text-[13px]">{t("当前版本")}<span className="tabular-nums">v{updateStatus?.current_version ?? "—"}</span>
            </p>
            <p className="mt-0.5 text-[11px] leading-relaxed text-[var(--color-muted)]">
              {updateStatus?.configured
                ? updateStatus.last_version
                  ? t("最近一次结果：{outcome}（{version}）", {
                    outcome: outcomeLabel(updateStatus.last_outcome),
                    version: updateStatus.last_version,
                  })
                  : t("尚未检查过更新。")
                : t(
                    "开发版本，自动更新不可用。",
                  )}
            </p>
            {updateStatus?.configured &&
            updateStatus.last_outcome === "failed" &&
            updateStatus.last_detail ? (
              <p className="mt-1 font-mono text-[11px] leading-relaxed text-[var(--color-danger)]">
                {updateStatus.last_detail}
              </p>
            ) : null}
          </div>
          <Button
            size="sm"
            onClick={onCheckUpdate}
            disabled={busy || !updateStatus?.configured}
            icon={<IconDownload className="size-3.5" />}
          >{t("检查更新")}</Button>
          {updateStatus?.configured && (updateStatus.last_outcome === "available" || updateStatus.last_outcome === "deferred") ? (
            <Button size="sm" disabled={busy} onClick={onInstallUpdate}>{t("安装更新并重启")}</Button>
          ) : null}
        </div>
      </Card>

      <details>
        <summary className="cursor-pointer py-2 text-[13px]">{t("帮助与诊断")}</summary>
        <Card title={t("工具")}>
          <div className="flex flex-wrap gap-2">
            <Button onClick={onPreviewDiagnostics} disabled={busy}>{t("查看诊断内容")}</Button>
            <Button onClick={onExport} disabled={busy}>{t("导出诊断包")}</Button>
            <Button onClick={onOpenConfigDir} disabled={busy}>{t("打开配置目录")}</Button>
          </div>
          {diagnosticsPreview ? <pre className="mt-2 max-h-64 overflow-auto text-xs">{diagnosticsPreview}</pre> : null}
        </Card>
      <Card
        title={t("环境状态")}
        description={t("Unfy 当前能不能正常工作。遇到问题时先看这里。")}
      >
        {health.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">{t("正在检查…")}</p>
        ) : (
          <ul>
            {health.map((item, index) => (
              <li
                key={item.id}
                className={`flex items-start gap-2.5 py-2 ${
                  index > 0 ? "border-t border-[var(--color-border)]" : ""
                }`}
              >
                <span
                  aria-hidden
                  className={`mt-[5px] size-[6px] shrink-0 rounded-full ${
                    item.status === "problem"
                      ? "bg-[var(--color-danger)]"
                      : item.status === "attention"
                        ? "bg-[var(--color-warning)]"
                        : "bg-[var(--color-success)]"
                  }`}
                />
                <div className="min-w-0">
                  <p className="text-[13px] leading-5">{renderMessage(item.label)}</p>
                  <p className="text-[11px] leading-relaxed text-[var(--color-muted)]">
                    {renderMessage(item.detail)}
                  </p>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card
        title={t("重置")}
        description={t("恢复除目标显示器以外的所有设置为出厂默认。")}
      >
        <div className="flex items-center justify-between gap-4">
          <p className="text-xs leading-relaxed text-[var(--color-muted)]">
            {t("会清除排除规则、快捷键、自启、检测设置与恢复延迟。目标显示器保留，因为那是你对着屏幕确认过的结果。")}
          </p>
          {/* Two-step confirmation in the page, not `window.confirm`: the native
              dialog does not render reliably inside a Tauri webview (it produced a
              stray window in verification), and a destructive action must not
              depend on a dialog that may not appear. */}
          {resetArmed ? (
            <div className="flex shrink-0 gap-1.5">
              <Button
                size="sm"
                variant="danger"
                disabled={busy}
                onClick={() => {
                  setResetArmed(false);
                  onReset();
                }}
              >{t("确认清除")}</Button>
              <Button
                size="sm"
                variant="quiet"
                disabled={busy}
                onClick={() => setResetArmed(false)}
              >{t("取消")}</Button>
            </div>
          ) : (
            <Button
              size="sm"
              variant="danger"
              disabled={busy}
              onClick={() => setResetArmed(true)}
            >{t("恢复默认设置")}</Button>
          )}
        </div>
      </Card>

      <Card title={t("帮助")}>
        <SettingRow
          label={t("重新显示引导")}
          hint={t("确认这台主机与目标显示器，然后开始使用。")}
        >
          <Button size="sm" onClick={onShowGuide} disabled={busy}>
            {t("重新显示")}
          </Button>
        </SettingRow>
      </Card>

      <Card title={t("隐私")}>
        <SettingRow
          label={t("活动历史")}
          hint={t("记录了每次整理、恢复与救援的时间与窗口数量。清除后无法恢复，其他设置不受影响")}
        >
          <div className="flex items-center gap-1.5">
            {historyArmed ? (
              <>
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => {
                    onClearHistory();
                    setHistoryArmed(false);
                  }}
                  disabled={busy}
                >
                  {t("确认清除")}
                </Button>
                <Button
                  size="sm"
                  variant="quiet"
                  disabled={busy}
                  onClick={() => setHistoryArmed(false)}
                >
                  {t("取消")}
                </Button>
              </>
            ) : (
              <Button size="sm" onClick={() => setHistoryArmed(true)} disabled={busy}>
                {t("清除历史")}
              </Button>
            )}
          </div>
        </SettingRow>
      </Card>
      </details>
    </>
  );
}

function detectHint(level: SettingsDto["auto_detect"]): string {
  switch (level) {
    case "verified":
      return t("已验证的 RDP 连接仅找回越界窗口，不重排可见窗口，也不会自动恢复手动整理。其他工具请手动整理和恢复。");
    default:
      return t("关闭自动检测，只用快捷键、托盘和界面按钮。");
  }
}

function outcomeLabel(outcome: string | null): string {
  switch (outcome) {
    case "available":
      return t("发现新版本");
    case "up_to_date":
      return t("已是最新");
    case "deferred":
      return t("已下载，等待安装");
    case "installed":
      return t("已安装");
    case "failed":
      return t("检查失败");
    default:
      return t("未知");
  }
}
