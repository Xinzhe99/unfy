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
}) {
  // The reset row is the one destructive action in the app; arming it is local
  // state so a stray click cannot wipe a configuration built over weeks.
  const [resetArmed, setResetArmed] = useState(false);

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
        description={t("自动检测能识别到的远程会话，并按你的选择收拢或恢复窗口。")}
        aside={
          settings.remote_active ? (
            <Status tone="accent">
              {settings.remote_evidence ?? t("检测到远程会话")}
            </Status>
          ) : null
        }
      >
        <SettingRow
          label={t("远程端能看到")}
          hint={
            settings.remote_screens === "all"
              ? t("既然全部屏幕都看得到，自动整理不会搬动任何窗口。")
              : t("默认。远端只有一块屏，整理才有意义。")
          }
        >
          <Segmented
            label={t("远程端能看到")}
            value={settings.remote_screens}
            disabled={busy}
            onChange={(remote_screens) => onSave({ remote_screens })}
            options={[
              { value: "single", label: t("一块屏") },
              { value: "all", label: t("全部屏幕") },
            ]}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow
          label={t("检测灵敏度")}
          hint={detectHint(settings.auto_detect)}
        >
          <Segmented
            label={t("检测灵敏度")}
            value={settings.auto_detect}
            disabled={busy}
            onChange={(auto_detect) => onSave({ auto_detect })}
            options={[
              { value: "off", label: t("关闭") },
              { value: "conservative", label: t("保守") },
              { value: "balanced", label: t("均衡") },
              { value: "aggressive", label: t("激进") },
            ]}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow
          label={t("检测到远程时自动整理")}
          hint={
            settings.auto_gather
              ? t("关闭时会先把窗口恢复原位，再停止自动整理")
              : t("关闭时只提示，不移动窗口")
          }
          disabled={settings.auto_detect === "off"}
        >
          <Switch
            checked={settings.auto_gather}
            disabled={busy || settings.auto_detect === "off"}
            label={t("检测到远程时自动整理")}
            onChange={(auto_gather) => onSave({ auto_gather })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow
          label={t("断开后自动恢复")}
          hint={t("远程会话结束后把窗口放回原位")}
          disabled={settings.auto_detect === "off"}
        >
          <Switch
            checked={settings.auto_restore}
            disabled={busy || settings.auto_detect === "off"}
            label={t("断开后自动恢复")}
            onChange={(auto_restore) => onSave({ auto_restore })}
          />
        </SettingRow>

        <p className="pt-1 text-[11px] leading-relaxed text-[var(--color-muted)]">
          {t("没有任何系统接口能查询「谁正在远程这台电脑」。检测依靠可验证的信号：远程桌面会话、虚拟显示器、已知远控工具的运行与连接状态。级别越高覆盖的软件越多，误判也越多。")}
        </p>
      </Card>

      <Card title={t("启动与运行")}>
        <SettingRow label={t("开机自动启动")} hint={t("登录后静默启动，不打开窗口")}>
          <Switch
            checked={settings.autostart}
            disabled={busy}
            label={t("开机自动启动")}
            onChange={(autostart) => onSave({ autostart })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label={t("自动检查更新")} hint={t("下载后等待窗口全部恢复再安装")}>
          <Switch
            checked={settings.auto_update}
            disabled={busy}
            label={t("自动检查更新")}
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
              ? `未生效：${hotkeys.gather_error}`
              : "在其他应用中按下也能触发"
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
              ? `未生效：${hotkeys.restore_error}`
              : "把窗口放回整理前的位置"
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

      <Card title={t("恢复行为")}>
        <SettingRow
          label={t("恢复延迟")}
          hint={t("等这么多秒再放回窗口。期间如果又检测到远程会话，这次恢复会取消——避免断开重连时来回搬两次")}
        >
          <div className="flex items-center gap-1.5">
            <input
              type="number"
              defaultValue={settings.restore_delay_secs}
              min={0}
              max={120}
              disabled={busy}
              onBlur={(event) => {
                const next = Number(event.currentTarget.value);
                if (Number.isFinite(next) && next !== settings.restore_delay_secs) {
                  onSave({
                    // Spec range is 15-120 s; 0 stays legal because a user may
                    // prefer an immediate restore and accept the risk of two
                    // migrations on a flapping connection.
                    restore_delay_secs: Math.min(120, Math.max(0, Math.round(next))),
                  });
                }
              }}
              className="h-7 w-16 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 text-right text-xs tabular-nums outline-none focus:border-[var(--color-accent)]"
            />
            <span className="text-[11px] text-[var(--color-muted)]">{t("秒")}</span>
          </div>
        </SettingRow>
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
                  ? `最近一次结果：${outcomeLabel(updateStatus.last_outcome)}（${updateStatus.last_version}）`
                  : "尚未检查过更新。"
                : "此构建没有签名公钥与更新地址，检查更新不会成功。发布版才会启用。"}
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
        </div>
      </Card>

      <Card
        title={t("环境状态")}
        description={t("Conflux 当前能不能正常工作。遇到问题时先看这里。")}
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
                  <p className="text-[13px] leading-5">{item.label}</p>
                  <p className="text-[11px] leading-relaxed text-[var(--color-muted)]">
                    {item.detail}
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
          <p className="text-xs leading-relaxed text-[var(--color-muted)]">会清除排除规则、快捷键、自启、检测设置与恢复延迟。目标显示器保留，因为那是你对着屏幕确认过的结果。</p>
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

      <Card title={t("隐私")}>
        <SettingRow
          label={t("使用统计")}
          hint={t("当前版本没有任何上传代码，这个开关只记录你的选择。将来若实现，只发送功能使用次数与错误类型，不含窗口标题与屏幕内容")}
        >
          <Switch
            checked={settings.telemetry}
            disabled={busy}
            label={t("使用统计")}
            onChange={(telemetry) => onSave({ telemetry })}
          />
        </SettingRow>
      </Card>
    </>
  );
}

function detectHint(level: SettingsDto["auto_detect"]): string {
  switch (level) {
    case "conservative":
      return t("只识别 Windows 远程桌面会话。几乎不会误判。");
    case "balanced":
      return t("还识别虚拟显示器（Parsec 等）。推荐。");
    case "aggressive":
      return t("还识别已安装的远控工具是否在运行或已连接。可能误判。");
    default:
      return t("关闭自动检测，只用快捷键、托盘和界面按钮。");
  }
}

function outcomeLabel(outcome: string | null): string {
  switch (outcome) {
    case "up_to_date":
      return "已是最新";
    case "deferred":
      return "已下载，等待安装";
    case "installed":
      return "已安装";
    case "failed":
      return "检查失败";
    default:
      return "未知";
  }
}
