# 验证任务：Windows 平台技术事实核查

你是独立事实核查员。产品背景（仅作上下文，不需要你评价产品）：

Unfy 是一个跨平台桌面工具，检测到本机被远程访问时，把副屏上的窗口迁移到被捕获的那块屏，断开后恢复。技术栈 Tauri 2 + Rust。你的任务**只做 Windows 部分的事实核查**，产出带证据的结论，供我们否决规格文档里编造的 API。

## 输出

写一个文件：`docs/verify/windows.md`。简体中文，表格优先，每条结论必须给出：

- 结论（可行 / 有条件可行 / 不可行 / 不确定）
- 依据（官方文档 URL 或真实开源项目文件路径 + 具体符号名）
- 落地代价
- 是否需要管理员权限 / 代码签名 / 驱动签名
- 无法确认的，写 `不确定`，并给出验证方法

## 必须核查的条目

1. **枚举与操作他人窗口**：`EnumWindows`、`GetWindowThreadProcessId`、`DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`、`SetWindowPos`、`ShowWindow`、`GetWindowPlacement`/`SetWindowPlacement`。确认：跨进程操作任意窗口（不需要注入）是否可行；UIPI 对低完整性进程操作高完整性窗口的实际限制是什么（是否与 UIPI 关闭相关）。
2. **DPI 与坐标系**：Per-Monitor V2 awareness 下 `GetWindowRect` 返回的是物理像素还是 DIP；`GetDpiForWindow`、`AdjustWindowRectExForDpi`、`SetProcessDpiAwarenessContext` 的正确用法；`MONITORINFOEX`/`GetMonitorInfo` 的 rcMonitor / rcWork 在 DPI 缩放下的含义。给出"从 A 屏（150%）迁移到 B 屏（100%）时保持视觉大小"的正确算法。
3. **显示器身份稳定标识**：`QueryDisplayConfig`/`DISPLAYCONFIG_TARGET_DEVICE_NAME`（含 `monitorDevicePath`/EDID）用于跨重启的稳定显示器 ID；说明 `EnumDisplayMonitors` HMONITOR 不能持久化的原因；给出推荐的稳定 ID 方案。
4. **显示器热插拔/配置变更通知**：`WM_DISPLAYCHANGE`、`WM_DPICHANGED`、`WM_SETTINGCHANGE`（哪些 lParam）、`RegisterDisplayConfigChangeNotification`(CCD 通知 API) 的真实名称与可用性。
5. **临时禁用/启用显示器**：`SetDisplayConfig(QDC_VIRTUAL_MODE_AWARE)` + `DISPLAYCONFIG_PATH_ACTIVE` 清除，或 `ChangeDisplaySettingsEx(CDS_UPDATEREGISTRY)` 设 `DM_POSITION` 到无效值的做法。确认：Windows 10/11 上实际可行性与副作用（是否导致应用响应 WM_DISPLAYCHANGE、远程端列表变化、恢复失败风险）。
6. **虚拟显示器驱动（IDD / IddCx）**：确认 IddCx 是 UMDF2 用户态驱动；确认发布到 Windows Update / 一般用户机器需要的签名路径（EV 证书 + Microsoft attestation signing / WHQL）；说明 attestation 签名是否适用于"非 PnP 通用驱动"、ARM64 的额外要求；给出真实存在的开源参考实现（如 VirtualDrivers/Virtual-Display-Driver）与其签名现状。给出"用户自行安装驱动"与"随安装包分发驱动"的合规差异。
7. **远程会话检测**：`GetSystemMetrics(SM_REMOTESESSION)`、`WTSQuerySessionInformation(WTSIsRemoteSession/WTSClientProtocolType)` 的真实签名与语义；RDP 与 console 会话的行为差异；第三方远程工具（TeamViewer/AnyDesk/RustDesk/ToDesk/向日葵/Parsec/Sunshine）**是否会**设置这些标志（能查到证据就给，查不到就明确说没有证据）。
8. **服务/提权 helper**：Windows 服务与用户会话交互的限制（Session 0 隔离）；Named Pipe 的调用方鉴权 API（`GetNamedPipeClientProcessId` + 校验 exe 签名/路径）；`CreateProcessAsUser`/计划任务以最高权限运行 UI 的更简单替代。
9. **开机自启**：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`、Task Scheduler、StartupTask（UWP）在"需要管理员权限的 helper"场景下各自的限制。
10. **窗口状态细节**：最大化窗口的 `GetWindowPlacement.rcNormalPosition` 语义；`WM_GETMINMAXINFO` 对最小尺寸的约束；Snap Layouts /  snapped window 在 `SetWindowPos` 后的行为（是否脱离 snap）；虚拟桌面 API `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop`（公开可用性）。
11. **自我纠正的应用**：Electron/Chromium 应用在外部 `SetWindowPos` 后是否会回弹（有无公开 issue 证据）。

## 事实纪律

- 只写你有证据的内容。禁止凭印象编造 API 名。所有 API 名必须在官方文档或真实代码中出现。
- 优先 microsoft.com/learn 官方文档；其次真实开源仓库源码。
- 每条给出的 URL 必须是你实际读过的页面（可以读取后摘录关键句）。
