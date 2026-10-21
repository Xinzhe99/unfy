import { useState } from "react";

import type {
  HealthItemDto,
  HotkeyStatusDto,
  SettingsDto,
  UpdateStatusDto,
} from "../api";
import { Button, Card, Segmented, SettingRow, Status, Switch } from "../components/ui";
import { HotkeyRecorder } from "../components/HotkeyRecorder";
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
    return <p className="text-xs text-[var(--color-muted)]">正在读取设置…</p>;
  }

  return (
    <>
      <Card title="外观">
        <SettingRow label="主题" hint="跟随系统会在系统切换深浅色时自动变化">
          <Segmented
            label="主题"
            value={settings.theme}
            disabled={busy}
            onChange={(theme) => onSave({ theme })}
            options={[
              { value: "system", label: "跟随系统" },
              { value: "light", label: "浅色" },
              { value: "dark", label: "深色" },
            ]}
          />
        </SettingRow>
      </Card>

      <Card
        title="远程会话检测"
        description="自动检测能识别到的远程会话，并按你的选择收拢或恢复窗口。"
        aside={
          settings.remote_active ? (
            <Status tone="accent">
              {settings.remote_evidence ?? "检测到远程会话"}
            </Status>
          ) : null
        }
      >
        <SettingRow
          label="检测灵敏度"
          hint={detectHint(settings.auto_detect)}
        >
          <Segmented
            label="检测灵敏度"
            value={settings.auto_detect}
            disabled={busy}
            onChange={(auto_detect) => onSave({ auto_detect })}
            options={[
              { value: "off", label: "关闭" },
              { value: "conservative", label: "保守" },
              { value: "balanced", label: "均衡" },
              { value: "aggressive", label: "激进" },
            ]}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow
          label="检测到远程时自动整理"
          hint="关闭时只提示，不移动窗口"
          disabled={settings.auto_detect === "off"}
        >
          <Switch
            checked={settings.auto_gather}
            disabled={busy || settings.auto_detect === "off"}
            label="检测到远程时自动整理"
            onChange={(auto_gather) => onSave({ auto_gather })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow
          label="断开后自动恢复"
          hint="远程会话结束后把窗口放回原位"
          disabled={settings.auto_detect === "off"}
        >
          <Switch
            checked={settings.auto_restore}
            disabled={busy || settings.auto_detect === "off"}
            label="断开后自动恢复"
            onChange={(auto_restore) => onSave({ auto_restore })}
          />
        </SettingRow>

        <p className="pt-1 text-[11px] leading-relaxed text-[var(--color-muted)]">
          没有任何系统接口能查询「谁正在远程这台电脑」。检测依靠可验证的信号：远程桌面会话、
          虚拟显示器、已知远控工具的运行与连接状态。级别越高覆盖的软件越多，误判也越多。
        </p>
      </Card>

      <Card title="启动与运行">
        <SettingRow label="开机自动启动" hint="登录后静默启动，不打开窗口">
          <Switch
            checked={settings.autostart}
            disabled={busy}
            label="开机自动启动"
            onChange={(autostart) => onSave({ autostart })}
          />
        </SettingRow>

        <div className="border-t border-[var(--color-border)]" />

        <SettingRow label="自动检查更新" hint="下载后等待窗口全部恢复再安装">
          <Switch
            checked={settings.auto_update}
            disabled={busy}
            label="自动检查更新"
            onChange={(auto_update) => onSave({ auto_update })}
          />
        </SettingRow>
      </Card>

      <Card title="快捷键">
        <SettingRow
          label="整理窗口"
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
          label="恢复布局"
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
        <p className="pt-1 text-[11px] leading-relaxed text-[var(--color-muted)]">
          点一下再按组合键即可录制，Esc 取消，Backspace 清除。被系统或其他应用占用时会在这里显示原因。
        </p>
      </Card>

      <Card title="恢复行为">
        <SettingRow
          label="恢复延迟"
          hint="等这么多秒再放回窗口。期间如果又检测到远程会话，这次恢复会取消——避免断开重连时来回搬两次"
        >
          <div className="flex items-center gap-1.5">
            <input
              type="number"
              defaultValue={settings.restore_delay_secs}
              min={0}
              max={60}
              disabled={busy}
              onBlur={(event) => {
                const next = Number(event.currentTarget.value);
                if (Number.isFinite(next) && next !== settings.restore_delay_secs) {
                  onSave({
                    restore_delay_secs: Math.min(60, Math.max(0, Math.round(next))),
                  });
                }
              }}
              className="h-7 w-16 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 text-right text-xs tabular-nums outline-none focus:border-[var(--color-accent)]"
            />
            <span className="text-[11px] text-[var(--color-muted)]">秒</span>
          </div>
        </SettingRow>
      </Card>

      <Card
        title="更新"
        aside={
          updateStatus?.configured ? (
            <Status tone="neutral">已配置更新源</Status>
          ) : (
            <Status tone="warning">未配置更新源</Status>
          )
        }
      >
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <p className="text-[13px]">
              当前版本 <span className="tabular-nums">v{updateStatus?.current_version ?? "—"}</span>
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
          >
            检查更新
          </Button>
        </div>
      </Card>

      <Card
        title="环境状态"
        description="Conflux 当前能不能正常工作。遇到问题时先看这里。"
      >
        {health.length === 0 ? (
          <p className="py-1 text-xs text-[var(--color-muted)]">正在检查…</p>
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
        title="重置"
        description="恢复除目标显示器以外的所有设置为出厂默认。"
      >
        <div className="flex items-center justify-between gap-4">
          <p className="text-xs leading-relaxed text-[var(--color-muted)]">
            会清除排除规则、快捷键、自启、检测设置与恢复延迟。目标显示器保留，
            因为那是你对着屏幕确认过的结果。
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
              >
                确认清除
              </Button>
              <Button
                size="sm"
                variant="quiet"
                disabled={busy}
                onClick={() => setResetArmed(false)}
              >
                取消
              </Button>
            </div>
          ) : (
            <Button
              size="sm"
              variant="danger"
              disabled={busy}
              onClick={() => setResetArmed(true)}
            >
              恢复默认设置
            </Button>
          )}
        </div>
      </Card>

      <Card title="隐私">
        <SettingRow
          label="使用统计"
          hint="当前版本没有任何上传代码，这个开关只记录你的选择。将来若实现，只发送功能使用次数与错误类型，不含窗口标题与屏幕内容"
        >
          <Switch
            checked={settings.telemetry}
            disabled={busy}
            label="使用统计"
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
      return "只识别 Windows 远程桌面会话。几乎不会误判。";
    case "balanced":
      return "还识别虚拟显示器（Parsec 等）。推荐。";
    case "aggressive":
      return "还识别已安装的远控工具是否在运行或已连接。可能误判。";
    default:
      return "关闭自动检测，只用快捷键、托盘和界面按钮。";
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
