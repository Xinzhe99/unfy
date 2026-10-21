# Windows 平台技术事实核查

核查对象：`docs/verify/brief-windows.md` 的 11 项。所有 API 名均来自下方「依据」中实际读过的页面或真实仓库符号；标 `不确定` 的条目给出验证方法。
未标注的推断一律写 `[推断]`。

## 总览

| # | 主题 | 结论 | 主要风险 |
|---|---|---|---|
| 1 | 枚举/操作他人窗口 | 有条件可行 | 高完整性窗口（UIPI）失败；被操作进程可能回弹 |
| 2 | DPI 与坐标系 | 可行（算法必须按物理像素算） | 缩放比算错会得到 1.5× 尺寸错误 |
| 3 | 显示器稳定 ID | 有条件可行 | 同型号双屏 EDID 冲突 |
| 4 | 显示变更通知 | 可行 | CCD 通知无官方 Win32 回调，只有 WinRT 事件 |
| 5 | 临时禁用显示器 | 有条件可行（高副作用） | 远程会话内直接 `ERROR_ACCESS_DENIED`；恢复失败=黑屏 |
| 6 | IDD / IddCx 驱动 | 有条件可行（重资产） | 必须微软签名；ARM64 事实上要 WHQL |
| 7 | 远程会话检测 | **不可行（对第三方工具）** | 只有 RDP 会置位，商业远控基本不置位 |
| 8 | 服务/提权 helper | 可行 | Session 0 隔离，必须 IPC 到用户会话 |
| 9 | 开机自启 | 有条件可行 | Run 键不提权；提权需计划任务 |
| 10 | 窗口状态细节 | 有条件可行 | snap/maximized 语义无法完全还原 |
| 11 | Electron/Chromium 回弹 | 有条件可行（有真实证据） | maximized/fullscreen 被应用自己改回 |

---

## 1. 枚举与跨进程操作窗口

| 项 | 结论 | 依据 |
|---|---|---|
| `EnumWindows` 枚举顶层窗口 | 可行 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumwindows>：“enumerates all top-level windows on the screen”；注意 “For Windows 8 and later, EnumWindows enumerates only top-level windows of desktop apps” |
| `GetWindowThreadProcessId` 拿 PID | 可行 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid> |
| `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` 拿可见边框 | 可行 | <https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute>；`DWMWA_EXTENDED_FRAME_BOUNDS` 定义见 <https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute>（“Retrieves the extended frame bounds rectangle in screen space”，类型 `RECT`） |
| `GetWindowRect` 的坑 | — | 同页 Remarks：“the Window Rect now may include invisible resize borders”；且 “GetWindowRect is virtualized for DPI” |
| `SetWindowPos` / `ShowWindow` / `Get`+`SetWindowPlacement` | 可行，无需注入 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos>、<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow>、<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowplacement>、<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowplacement> |

**跨进程操作任意窗口（不注入）**：API 层面可行，但**不是无条件**：

1. **Session 边界**：`SetWindowPos` Remarks 原文：“all services were moved off the interactive desktop into Session 0. hwnd and window manager operations are only effective inside a session and cross-session attempts to manipulate the hwnd will fail.” → helper 必须在用户会话内运行（见第 8 条）。
2. **UIPI（完整性级别）**：低 IL 进程不能操作高 IL 窗口，`SetWindowPos` 直接 `ERROR_ACCESS_DENIED`。实证：
   - <https://stackoverflow.com/questions/29142491/setwindowpos-returns-access-denied>（“When selected window is running with Administrator privileges I'am getting error 'Access denied'”）
   - <https://learn.microsoft.com/en-us/answers/questions/2152111/not-working-setwindowpos-for-on-screen-keyboard-is>：RLWA32 回答指出 `osk.exe` 是 UIAccess 应用（Medium+），`SetWindowPos` 失败，`MoveWindow` “is also subject to UIPI restrictions”，同样 access denied。
   - UIPI 定义与 UIAccess 提权条件：<https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview>（“An application that doesn't have UIAccess in the manifest starts with medium IL and cannot access elevated ('medium+' IL) process UI”；UIAccess 需 Authenticode 签名 + 装在安全目录 + manifest `uiAccess="true"`）
   - UIPI 的官方语义表述（Microsoft 支持文档原文）：“Your application can only send commands to applications of the same or lower elevation.” <https://learn.microsoft.com/en-us/troubleshoot/power-platform/power-automate/desktop-flows/ui-automation/uipi-issues>
3. **`ChangeWindowMessageFilterEx` 解决不了移动窗口**：它只放行**消息**，不改变 `SetWindowPos` 的权限检查。文档 <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changewindowmessagefilterex>：“Modifies the User Interface Privilege Isolation (UIPI) message filter for a specified window”，`action` 为 `MSGFLT_ALLOW`/`MSGFLT_DISALLOW`/`MSGFLT_RESET`。它修改的是**某个 hwnd** 的过滤器，且文档未说明低 IL 进程可以修改他人窗口的过滤器（只说明 `SECURITY_MANDATORY_LOW_RID` 及以下进程调用会 `ERROR_ACCESS_DENIED`）。因此“UIPI 关掉就无所谓”这个说法在本产品里**不成立**：我们无法从外部让别人的窗口接受我们的移动请求。

**落地代价**：低（纯 Win32 调用）；**权限**：操作普通用户窗口无需管理员，操作提权窗口需本进程也提权；**签名**：仅当要 UIAccess 时需 Authenticode + Program Files 安装。

**备注**：`ShowWindow` 首次调用会被 `STARTUPINFO` 覆盖（文档明示），跨进程调用它改 show state 是可行但语义脆弱；建议只在必要时用，且用 `GetWindowPlacement` 的 `showCmd` 做状态来源。

---

## 2. DPI 与坐标系（重点）

### 2.1 各 API 在 Per-Monitor V2 下返回什么

| API | PMv2 进程下返回 | 依据 |
|---|---|---|
| `GetWindowRect` | **物理像素**（虚拟屏幕坐标）。文档只说 “virtualized for DPI”，即按调用线程 awareness 换算；PMv2 = 原始像素 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowrect> + <https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows>（PMv2 行为列表：“The application seeing the raw pixels of each display”“never being bitmap scaled”） |
| `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` | **始终物理像素**（“unlike the Window Rect, the DWM Extended Frame Bounds are not adjusted for DPI”） | 同上 GetWindowRect 页 Remarks |
| `GetDpiForWindow(hwnd)` | 取决于**该窗口**的 awareness：unaware→96，system→系统 DPI，per-monitor→所在显示器 DPI | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow>（含表格） |
| `GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, …)` | 对 per-monitor aware 调用方返回“用户为该显示器设置的实际 DPI”；`MDT_EFFECTIVE_DPI`=0 的定义是“incorporates the scale factor set by the user for this specific display”，而 `MDT_RAW_DPI`/`MDT_ANGULAR_DPI` **不含**用户缩放——换算视觉大小必须用 `MDT_EFFECTIVE_DPI` | <https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/nf-shellscalingapi-getdpiformonitor>、<https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/ne-shellscalingapi-monitor_dpi_type> |
| `GetMonitorInfo` 的 `rcMonitor`/`rcWork` | 虚拟屏幕坐标；文档未写 DPI 虚拟化 → 在 PMv2 下即物理像素 `[推断]` | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-monitorinfo>（“expressed in virtual-screen coordinates”） |
| `AdjustWindowRectExForDpi` | 按给定 dpi 把**客户区** rect 换算成**窗口** rect | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-adjustwindowrectexfordpi> |
| `GetSystemMetrics` | **不是 DPI-aware**，PMv2 线程下不要用；用 `GetSystemMetricsForDpi` | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics>（“This API is not DPI aware, and should not be used if the calling thread is per-monitor DPI aware”） |
| `SetProcessDpiAwarenessContext` | 必须在任何 UI/DPI 依赖 API 之前调用；manifest 已设置则返回 `ERROR_ACCESS_DENIED`；推荐用 manifest 而非 API | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext> |

PMv2 上下文常量：`DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`（值 -4），见 <https://learn.microsoft.com/en-us/windows/win32/hidpi/dpi-awareness-context>。运行时切换单线程上下文用 `SetThreadDpiAwarenessContext`（<https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-improvements-for-desktop-applications>），这是操作**别人窗口**时正确的做法：本进程用 manifest 设 PMv2，但读第三方窗口前显式 `SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`，避免被调用线程上下文污染。

### 2.2 关键陷阱

- **不能对第三方窗口用 `GetDpiForWindow` 判断它所在屏的缩放**：若目标进程是 DPI-unaware，该函数返回 96（见上表）。正确做法是 `MonitorFromWindow` + `GetDpiForMonitor(..., MDT_EFFECTIVE_DPI, ...)`，且本线程必须是 per-monitor aware，否则同样被虚拟化。
- **`DWMWA_EXTENDED_FRAME_BOUNDS` 才是“看得见的矩形”**：`GetWindowRect` 含不可见 resize 边框（Win10 起约 7px/边），用它算比例会把不可见边框一起缩放，导致视觉尺寸漂移。
- 不要自己伪造 `WM_DPICHANGED`；跨屏移动后由系统投递（<https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged>：lParam 是建议 RECT，应用应据此 `SetWindowPos`）。

### 2.3 「A 屏 150% → B 屏 100% 保持视觉大小」的正确算法

设 A 屏 effective DPI = `dpiA`（150% → 144），B 屏 `dpiB`（100% → 96）；`USER_DEFAULT_SCREEN_DPI = 96`。

```text
1. SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
2. 读源矩形（物理 px）：
     R_vis = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS)   // 可见边框
     R_win = GetWindowRect(hwnd)                                        // 含不可见边框
     d     = R_win.origin - R_vis.origin      // 每边的不可见边框差
3. hmonA = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)   // 取交叠面积最大的显示器
   dpiA  = GetDpiForMonitor(hmonA, MDT_EFFECTIVE_DPI)
   dpiB  = GetDpiForMonitor(目标 HMONITOR, MDT_EFFECTIVE_DPI)
   （MonitorFromWindow 的 dwFlags 语义见
     https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow）
4. 缩放（四舍五入，保证不小于 1px）：
     w2 = round(w1 * dpiB / dpiA)     // 1200px@144dpi → 800px@96dpi
     h2 = round(h1 * dpiB / dpiA)
5. 位置：保持“在工作区内的相对位置”，不要按绝对像素平移：
     fx = (R_vis.left - rcWorkA.left) / (rcWorkA.right  - rcWorkA.left)
     fy = (R_vis.top  - rcWorkA.top ) / (rcWorkA.bottom - rcWorkA.top )
     x2 = rcWorkB.left + round(fx * (rcWorkB.right  - rcWorkB.left))
     y2 = rcWorkB.top  + round(fy * (rcWorkB.bottom - rcWorkB.top ))
6. 夹取：保证标题栏可见（y2 >= rcWorkB.top，x2 使窗口至少露出 N px）
7. SetWindowPos(hwnd, NULL, x2 - d.x, y2 - d.y, w2 + d.w, h2 + d.h,
                SWP_NOZORDER | SWP_NOACTIVATE)
8. 回读 GetWindowRect / DWMWA_EXTENDED_FRAME_BOUNDS 校验（见第 10、11 条：
   应用可能夹取或回弹）
```

要点：

- 只对**非最大化**窗口走这条路径。最大化窗口尺寸由系统决定，`SetWindowPos` 的 cx/cy 会被忽略或触发应用自己改回（第 10、11 条）。最大化窗口要“搬屏”：先 `ShowWindow(SW_RESTORE)`，移动，再 `ShowWindow(SW_MAXIMIZE)`；或用 `SetWindowPlacement` 改 `rcNormalPosition` 后保持 `SW_SHOWMAXIMIZED`。
- `AdjustWindowRectExForDpi` **不是**这条路径的必需品；只有当你要保持“客户区 DIP 尺寸”而非“外框视觉尺寸”时才需要：`AdjustWindowRectExForDpi(&rcClientDIP, style, hasMenu, exStyle, dpiB)`。
- 坐标单位一致性：`rcWork` 来自 `GetMonitorInfo`，在 PMv2 线程下与 `GetWindowRect` 同一物理坐标系；若本线程不是 PMv2，两者都会被虚拟化，比例会算错。

**落地代价**：中（约 150–250 行 Rust + win32 绑定，含边界夹取与回读校验）；**权限**：无；**签名**：无。

---

## 3. 显示器身份稳定标识

| 结论 | 依据 |
|---|---|
| `HMONITOR` **不能**持久化 | <https://learn.microsoft.com/en-us/windows/win32/gdi/hmonitor-and-the-device-context>：“A physical display has the same HMONITOR as long as it is part of the desktop. When a WM_DISPLAYCHANGE message is sent, any monitor may be removed from the desktop and thus its HMONITOR becomes invalid or has its settings changed.” |
| `QueryDisplayConfig` 取路径 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig>；flags 用 `QDC_ONLY_ACTIVE_PATHS`（0x2）或 `QDC_ONLY_ACTIVE_PATHS \| QDC_VIRTUAL_MODE_AWARE`（0x10） |
| `DisplayConfigGetDeviceInfo(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME)` 取 `monitorDevicePath` | <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_target_device_name>：字段 `monitorDevicePath[128]`、`monitorFriendlyDeviceName[64]`、`edidManufactureId`、`edidProductCodeId`、`connectorInstance`；`edid*` 仅在 flags 的 `edidIdsValid` 置位时有效 |
| 路径结构 | <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_path_info>（`DISPLAYCONFIG_PATH_ACTIVE` = 0x1）；`targetInfo.adapterId` + `targetInfo.id` 见 <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_path_target_info> |
| GDI 侧设备名 | `EnumDisplayDevices(..., EDD_GET_DEVICE_INTERFACE_NAME)` 的 `DeviceID` 可拿到 `\\?\DISPLAY#...` 接口名，用作 GDI↔SetupAPI 的桥 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaydevicesw> |

**推荐稳定 ID 方案**（结论：有条件可行）：

```
key = monitorDevicePath（主键，形如 \\?\DISPLAY#DELA1B2#5&...&0&UID4352#{e6f07b5f-...}）
    + edidManufactureId/edidProductCodeId（仅 edidIdsValid 时）
    + connectorInstance（同型号同 EDID 时的区分位）
    + 分辨率/物理尺寸（作为辅助校验）
```

- `monitorDevicePath` 由监视器设备实例路径构成，含 EDID 派生信息与接口实例号，跨重启稳定 `[推断]`（官方文档只描述“path to the device name for the monitor”，未承诺跨重启稳定性）。
- **同型号双屏**（例如两块相同的 Dell）EDID 完全相同，仅靠 `edidManufactureId`+`edidProductCodeId` 会撞；必须叠加 `monitorDevicePath` 里的 UID 或 `connectorInstance`。
- 换接口（HDMI1 → HDMI2）后 device path 会变 → 需要“回退匹配”策略（分辨率 + 位置 + EDID）。

`不确定` 项：`monitorDevicePath` 是否在换端口/换扩展坞后保持不变。验证方法：同一台机器，用 `QueryDisplayConfig` 打印全部 active path 的 `monitorDevicePath`，插拔到另一个端口、重启、再比对 diff。

**落地代价**：低；**权限**：无（`QueryDisplayConfig` 在远程会话返回 `ERROR_ACCESS_DENIED`，见第 5 条，但本机 console 会话正常）。

---

## 4. 显示器热插拔 / 配置变更通知

| 通知 | 真实名称 | 语义 | 依据 |
|---|---|---|---|
| `WM_DISPLAYCHANGE` | 存在 | 分辨率变化时发给所有窗口；wParam=新 bpp，LOWORD(lParam)=宽，HIWORD=高。**只保证发到顶层窗口**，其余窗口是 post | <https://learn.microsoft.com/en-us/windows/win32/gdi/wm-displaychange> |
| `WM_DPICHANGED` | 存在（0x02E0） | 窗口 effective DPI 变化；wParam 高低字 = X/Y DPI，lParam = 建议 RECT | <https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged> |
| `WM_SETTINGCHANGE` | 存在（= `WM_WININICHANGE` 0x001A） | `SystemParametersInfo` 改设置或策略变化时广播；lParam 是**区域名字符串**（注册表叶子名 / `"Policy"` / `"intl"` / `"Environment"` / `"ConvertibleSlateMode"` / `"SystemDockMode"`）；文档明确 “This parameter does not usually indicate which specific system parameter changed” | <https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-settingchange> |
| `WM_DEVICECHANGE` | 存在 | wParam 为 `DBT_DEVICEARRIVAL`(0x8000) / `DBT_DEVICEREMOVECOMPLETE`(0x8004) / `DBT_DEVNODES_CHANGED`(0x0007) 等 | <https://learn.microsoft.com/en-us/windows/win32/devio/wm-devicechange> |
| `RegisterDisplayConfigChangeNotification` | **不存在此 Win32 API** | 在 CCD API 列表（`QueryDisplayConfig`/`SetDisplayConfig`/`DisplayConfigGetDeviceInfo`/`DisplayConfigSetDeviceInfo`/`GetDisplayConfigBufferSizes`）中没有该函数 | <https://learn.microsoft.com/en-us/windows-hardware/drivers/display/ccd-apis> |
| 真实可用的“CCD 变更事件” | WinRT `Windows.Devices.Display.Core.DisplayManager` 的 `Changed` 事件 + `Start()`/`Stop()` | “raised when system display hardware is added, removed, or modified”；另有 `Disabled`/`Enabled`/`PathsFailedOrInvalidated` | <https://learn.microsoft.com/en-us/uwp/api/windows.devices.display.core.displaymanager> |

**推荐做法**：主路径用 `WM_DISPLAYCHANGE`（广播、无需 COM）+ `WM_DEVICECHANGE`（`DBT_DEVNODES_CHANGED`）做粗触发，收到后**重新 `QueryDisplayConfig` 取真值**，不要相信消息参数；需要精确拓扑时再加 WinRT `DisplayManager`（Win10 1809+，注意它是“exclusive ownership”模型，`TryAcquireTarget*` 会夺取目标所有权，与系统显示控制冲突，**不要**为了监听而 acquire）。

`不确定`：是否存在未公开的 `DisplayConfig*` 变更回调。验证方法：对 `user32.dll` 导出表 dump 全部 `DisplayConfig*`/`*Display*Notification*` 符号。

**落地代价**：低（一个隐藏的顶层窗口 + 去抖 + 重查 `QueryDisplayConfig`）；**权限**：无（`WM_DISPLAYCHANGE` 是广播消息，普通进程可收）；**签名**：无。**注意**：`WM_DISPLAYCHANGE` 只保证投递给顶层窗口（文档：“This message is only sent to top-level windows. For all other windows it is posted.”），message-only 窗口（`HWND_MESSAGE`）不属于顶层窗口，因此 `[推断]` 收不到，必须建真正的顶层隐藏窗口。

---

## 5. 临时禁用 / 启用显示器（重点）

| 做法 | 结论 | 依据 |
|---|---|---|
| `SetDisplayConfig` + 去掉某 path 的 `DISPLAYCONFIG_PATH_ACTIVE` | 可行（文档语义支持） | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setdisplayconfig>：“Only the paths within this array that have the DISPLAYCONFIG_PATH_ACTIVE flag set in the flags member … are set”；`DISPLAYCONFIG_PATH_ACTIVE`=0x1 见 path_info 页 |
| flags 组合 | `SDC_APPLY \| SDC_USE_SUPPLIED_DISPLAY_CONFIG`（临时，不写库）；**不要**加 `SDC_SAVE_TO_DATABASE` | 同页“Set a temporary display configuration (that is, the display configuration will not be saved)”；`SDC_SAVE_TO_DATABASE` 只能与 `SDC_USE_SUPPLIED_DISPLAY_CONFIG` 同用 |
| `ChangeDisplaySettingsEx` 把 `DM_POSITION` 设无效/尺寸清零来“摘除” | 可行（老路） | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changedisplaysettingsexw>：“To detach the monitor, set DEVMODE.dmFields to DM_POSITION but set DEVMODE.dmPelsWidth and DEVMODE.dmPelsHeight to zero.” |

> **规格纠错**：brief 里写的 `SetDisplayConfig(QDC_VIRTUAL_MODE_AWARE)` **标志名用错了**。`QDC_VIRTUAL_MODE_AWARE`（0x10）是 **`QueryDisplayConfig`** 的 flags；`SetDisplayConfig` 对应的是 **`SDC_VIRTUAL_MODE_AWARE`（0x00008000）**，且文档限定它“A modifier to the SDC_USE_SUPPLIED_DISPLAY_CONFIG and SDC_TOPOLOGY_SUPPLIED flags”。
> 依据：<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig>（QDC 表）与 <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setdisplayconfig>（SDC 表）。

### 真实可行性与副作用（结论：有条件可行，副作用重）

1. **远程会话里直接失败**：`SetDisplayConfig` / `QueryDisplayConfig` 的返回码表都列了 `ERROR_ACCESS_DENIED`：“The caller does not have access to the console session. This error occurs if the calling process does not have access to the current desktop or is running on a remote session.”
   → 这一条与产品前提**正面冲突**：如果检测到“正被 RDP 远程访问”，此刻调用方就在远程会话中，**无法改显示拓扑**。必须在 console 会话里的进程执行（见第 8 条 IPC 架构）。
2. **必然触发 `WM_DISPLAYCHANGE`**：`ChangeDisplaySettingsEx` 页明写动态改模式时会向所有应用发 `WM_DISPLAYCHANGE`（并给出 wParam/lParam 含义）。所有全屏应用、游戏、播放器、截图/录制工具都会收到；部分应用会重新布局或退出全屏。
3. **远程端列表会变**：被禁用的显示器从桌面消失，远控软件（Sunshine/Parsec 之类按输出枚举）的显示器列表随之变化；若禁用的正是它正在抓取的那块，远端会直接黑屏/断流。
4. **恢复失败风险（最高）**：`SDC_APPLY` 不写数据库，进程崩溃/断电后拓扑停留在“少一块屏”的状态；用户若当时只有该屏可用会看不到界面。`SetDisplayConfig` 文档明确“By default, SetDisplayConfig never changes any supplied path…”且失败返回 `ERROR_BAD_CONFIGURATION`，需要 `SDC_ALLOW_CHANGES` 才能让系统自行调整——意味着恢复调用同样可能失败。
5. **主屏不可禁**：文档未给出禁用 primary 的行为；`[推断]` 系统会拒绝或重排主屏。验证方法：单屏 + 一块副屏，尝试对 primary 的 path 去掉 ACTIVE，读返回码与 `GetMonitorInfo` 的 `MONITORINFOF_PRIMARY`。

**落地代价**：中高（拓扑快照 + 事务式恢复 + 崩溃后自愈的“上次未恢复”标记）；**权限**：需在 console 会话内，不需要管理员（文档未要求提权）`[推断]`，但需在用户桌面会话；**签名**：无（普通进程即可）。

**备注**：更安全的替代是**不禁用**，只做“窗口搬迁”（第 1、2 条）——副作用为零。禁用显示器应作为可选高级功能，且必须有 watchdog（独立进程/服务在 N 秒后检查拓扑是否恢复，未恢复则强制 `SDC_APPLY | SDC_USE_DATABASE_CURRENT`，见 SetDisplayConfig 页场景表“Return from a temporary mode to the last saved display configuration”）。

---

## 6. 虚拟显示器驱动（IDD / IddCx）（重点）

### 6.1 事实

| 项 | 结论 | 依据 |
|---|---|---|
| IddCx 是 UMDF2 用户态驱动 | **是** | <https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview>：“An IDD is the third party-provided UMDF driver for the device”；“The IDD uses a user-mode model and doesn't support kernel-mode components”；“runs in Session 0 without any components running in the user session” |
| 微软官方样例 | 存在 | 同页 “Sample code” 指向 `Microsoft/Windows-driver-samples/tree/main/video/IndirectDisplay` |
| 公开参考实现 | `VirtualDrivers/Virtual-Display-Driver`（VDD） | <https://raw.githubusercontent.com/VirtualDrivers/Virtual-Display-Driver/master/README.md>（含与 `usbmmid_v2`、`virtual-display-rs`、`parsec-vdd`、`RustDesklddDriver` 的对比表；自述 “It operates in User Mode(Session0)”）；其签名现状：SignPath 免费代码签名（README Sponsors 段），ARM64 需 testsigning（对比表脚注 ¹） |
| 第三方实现 | `nomi-san/parsec-vdd`（Parsec VDD 的独立封装） | <https://github.com/nomi-san/parsec-vdd>；Parsec 官方说明 VDD 只在 Parsec 会话中创建显示 <https://support.parsec.app/hc/en-us/articles/32381178803604-VDD-Overview-Prerequisites-and-Installation> |

### 6.2 发布到普通用户机器的签名路径（结论：有条件可行，且是重资产）

官方流程（**EV 证书 + Partner Center attestation**）：

1. 注册 Microsoft Windows Hardware Developer Program。
2. 必须持有 **EV 代码签名证书**（用于建立/关联 Partner Center 账号，并在提交时对 CAB 签名）。
3. `MakeCab` 打包含 `.sys/.dll` + `.inf` + `.pdb` 的 CAB；用 EV 证书 `signtool sign /fd sha256` 签 CAB。
4. Partner Center → Submit new hardware → 两个 test-signing 选项都**不勾** → 请求签名 → 下载微软重签后的包。
5. 微软会追加 SHA-2 嵌入签名并重建 catalog。
依据：<https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-attestation>（含 CAB 组成、“Leave both test-signing options unchecked”、“The driver uses the following process when it resigns the driver”）与 <https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs>（“To submit binaries for attestation signing, your Hardware Dev Center dashboard account must have at least one EV certificate associated with it”；SHA-2/`/fd sha256`）。

**attestation 的限制**（<https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings>）：

- “Attestation signed drivers can't be published to Windows Update for retail audiences.”（要进 WU 给普通用户推送，必须走 WHCP/HLK）
- “Attestation signing only works on Windows 10 Desktop and later versions.”
- “Attestation signing supports Windows Desktop kernel mode and user mode drivers.”
- “When a driver receives attestation signing, it's not Windows Certified… there are no assurances about compatibility or functionality.”
- 表：`Windows Server >= 2016` 下 “Attestation dashboard signed = No”，只有 HLK 通过的 dashboard 签名可用。
- 另：`Windows Server 2016 and greater doesn't accept attested device and filter driver signing submissions.`

**结论**：attestation 签名**足以**让 Win10/11 客户端机器（含开启 Secure Boot/HVCI 的普通用户）安装并加载驱动，但①不进 Windows Update（不能靠 WU 自动分发，必须自己带安装器/`pnputil`/SetupAPI）；②无 WHQL 兼容性背书；③Server 与 ARM64 实际不可用（见下）。

**是否适用于“非 PnP 通用驱动”**：`不确定`。文档只承诺支持 “Windows Desktop kernel mode and user mode drivers”，且 CAB 必须含 INF；对**没有 INF 的纯 non-PnP `.sys`** 能否走 attestation 未明说。验证方法：在 Partner Center 用最小 INF（`Root\` 枚举的 PnP 设备）+ 同一 `.sys` 分别提交，观察是否被拒。IDD 天然是 PnP（`Root\MttVDD` 这类 root-enumerated 设备），所以实际不受此限制。

**ARM64 是否要更多**：**是，事实上需要 Microsoft 签名（WHQL/attestation），本地自签不行**。

- VDD issue #465 提供了完整证据链：ARM64 Win11 上 SignPath 证书链在 `LocalMachine\TrustedPublisher`/`Root` 都完整（`X509Chain.Build` = True），但 `setupapi.dev.log` 报 `sig: Error 0x800b0109 ... Code Integrity is enforced`、`Driver package import failed for device`；维护者与用户结论是 “On Windows 11 ARM64, Secure Boot cannot be disabled, Test signing cannot be enabled”。
  <https://github.com/VirtualDrivers/Virtual-Display-Driver/issues/465>
- VDD 自己的文档：ARM64 在 Win11 24H2+ “may require test signing mode enabled, even with valid signatures”。
  <https://virtualdrivers-virtual-display-driver.mintlify.app/development/driver-signing>
- 通用规则：Win10 1607 起“Windows will not load any new kernel mode drivers which are not signed by the Microsoft through the Hardware Dev Center”，虚拟驱动同样适用（“Virtual drivers have the same requirements as actual hardware drivers”）。
  <https://learn.microsoft.com/en-us/windows-hardware/drivers/install/kernel-mode-code-signing-requirements--windows-vista-and-later->

### 6.3 「用户自行安装驱动」vs「随安装包分发驱动」

| 维度 | 用户自行安装 | 随安装包分发 |
|---|---|---|
| 合法性 | 用户自担风险；开源项目常见做法 | 微软没有公开条款禁止分发已签名驱动 `不确定`（需查 Partner Center 协议/WHQL 条款） |
| 技术前提 | 同左：**必须有微软签名**，否则要关 Secure Boot + 开 testsigning（ARM64 不可行） | 同上 |
| 安装权限 | 管理员/UAC 提示；未签名会弹 “Windows can't verify the publisher of this driver software” | 你的安装器需以管理员调用 `pnputil /add-driver /install` 或 SetupAPI；`PnpLockDown=1` 会阻止应用直接改驱动文件（<https://learn.microsoft.com/en-us/windows-hardware/drivers/install/inf-version-section>） |
| 更新 | 用户手动 | 你负责版本与回滚；attestation 驱动**不能**用 DUA（driver update acceptable），见 driver-signing-offerings |
| 责任 | 用户 | 你的签名/品牌承担蓝屏与兼容性责任（VDD README 明确警告 GPU 驱动更新会导致黑屏） |

**落地代价**：很高（驱动开发 + HLK 测试 + Partner Center 流程 + 安装器 + 崩溃恢复）；**权限**：安装需管理员；**签名**：EV 证书（年费）+ 微软 attestation（免费但需审核），ARM64/Server/WU 分发需 WHQL。

**产品建议**：虚拟显示器驱动与本产品（窗口搬迁）**没有依赖关系**。窗口搬迁完全不需要 IDD；把 IDD 做成独立可选组件，避免主产品被驱动签名流程绑架。

---

## 7. 远程会话检测（重点）

### 7.1 官方 API 的真实语义

| API | 真实签名/取值 | 语义 | 依据 |
|---|---|---|---|
| `GetSystemMetrics(SM_REMOTESESSION)` | `int GetSystemMetrics(int nIndex)`；`SM_REMOTESESSION` = **0x1000** | “If the calling process is associated with a Terminal Services client session, the return value is nonzero. If … console session, the return value is 0.” | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics>（SM_REMOTESESSION 行） |
| `GetSystemMetrics(SM_REMOTECONTROL)` | 0x2001 | 当前会话**正被远程控制**（shadow/tsadmin） | 同页 |
| `WTSQuerySessionInformationW(hServer, SessionId, WTSInfoClass, &buf, &bytes)` | `BOOL`；`WTS_CURRENT_SERVER_HANDLE` + `WTS_CURRENT_SESSION` | 通用会话信息查询；返回缓冲需 `WTSFreeMemory` | <https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsquerysessioninformationa> |
| `WTSIsRemoteSession` | `WTS_INFO_CLASS` 枚举成员；返回 TRUE/FALSE | “Determines whether the current session is a remote session… This value can only be used for the local machine, so the hServer parameter … must contain WTS_CURRENT_SERVER_HANDLE.” | <https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ne-wtsapi32-wts_info_class> |
| `WTSClientProtocolType` | `USHORT`：**0 = console，1 = 保留，2 = RDP** | 判断是否物理 console 的官方方法 | 同上 + <https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsquerysessioninformationa>（“If ppBuffer is '0', the session is attached to the physical console”） |

**重要官方警告（决定成败）**：`SM_REMOTESESSION` 在 RemoteFX vGPU 场景会把远程会话误判为本地：

> “You should not use GetSystemMetrics(SM_REMOTESESSION) to determine if your application is running in a remote session in Windows 8 and later … if the remote session may also be using the RemoteFX vGPU improvements … GetSystemMetrics(SM_REMOTESESSION) will identify the remote session as a local session.”
> 官方给的补丁是读 `HKLM\System\CurrentControlSet\Control\Terminal Server\GlassSessionId`，与 `ProcessIdToSessionId` 比较。
> 依据：<https://learn.microsoft.com/en-us/windows/win32/termserv/detecting-the-terminal-services-environment>

### 7.2 RDP vs console 行为差异

- RDP 连接会创建/激活**独立的 Terminal Services 会话**（会话名 `rdp-tcp#N` 或 `RDP-Tcp#N`），该会话内 `SM_REMOTESESSION != 0`、`WTSClientProtocolType == 2`；物理屏幕所在的 console 会话此时**被断开（disconnected）**，而不是被“共享”。
- console 会话（会话名 `console`，`WTSClientProtocolType == 0`）在无人 RDP 时是本机的物理会话。
- 因此“RDP 期间把窗口搬到被捕获的屏”在 RDP 场景下**语义是错的**：RDP 会话的“屏幕”是虚拟的客户端分辨率，物理副屏根本不参与。这是产品设计层面的问题，不只是 API 问题。

### 7.3 第三方远控工具是否会置位这些标志

**结论：没有证据表明 TeamViewer / AnyDesk / RustDesk / ToDesk / 向日葵 / Parsec / Sunshine 中的任何一个会设置 `SM_REMOTESESSION` 或让 `WTSClientProtocolType` 变成 2。** 逐项：

| 工具 | 是否有证据置位 `SM_REMOTESESSION` | 证据 / 说明 |
|---|---|---|
| TeamViewer | **无** | 官方 KB 只描述连接 Windows Server 时的“console 会话 / 指定用户会话”选择，未提及任何会话标志；其工作方式是镜像已有交互会话 <https://www.teamviewer.com/en-ca/global/support/knowledge-base/teamviewer-remote/remote-control/use-teamviewer-remote-on-windows-servers>。`SM_REMOTESESSION` 由 Terminal Services 在**创建 RDP 会话时**设置，第三方镜像工具不改这个内核/会话状态 `[推断]` |
| AnyDesk | **无** | 官方文档：连 Windows Server 的**非 console 账户**时“that user must already be logged into the session … typically requires an RDP session to be open beforehand”，并建议装 AnyDesk Mirror Driver 提升性能 <https://support.anydesk.com/docs/anydesk-for-windows-server>。说明 AnyDesk 是**附着到已有会话**，不创建 RDP 会话 |
| RustDesk | **无（可查源码）** | 源码 `src/platform/windows.cc` 中 `WTSQuerySessionInformationA(..., WTSClientProtocolType, ...)` 只用于 `is_rdp_session_by_protocol()` **识别并选择**目标会话（配合 `WTSGetActiveConsoleSessionId`、`WTSEnumerateSessionsA` 匹配 `"console"`/`"rdp"`/`"ica"` 前缀）；全仓库 grep 不到 `SM_REMOTESESSION` / `WTSIsRemoteSession` 的写入或设置。仓库：`rustdesk/rustdesk`，符号：`is_rdp_session_by_protocol`、`get_current_session`、`WTS_PROTOCOL_TYPE_RDP` |
| ToDesk | **无** | 闭源，未找到任何官方或社区证据；`不确定` |
| 向日葵（Sunlogin） | **无** | 闭源，未找到证据；`不确定` |
| Parsec | **无** | 官方仅说明 VDD 在 Parsec 会话中创建虚拟显示 <https://support.parsec.app/hc/en-us/articles/32381178803604-VDD-Overview-Prerequisites-and-Installation>；无会话标志相关说明 |
| Sunshine | **无（可查源码）** | 仓库 `LizardByte/Sunshine` 全量 grep：`GetSystemMetrics` 只出现在 `src/platform/windows/display_base.cpp`（`SM_CXVIRTUALSCREEN`/`SM_CYVIRTUALSCREEN`/`SM_XVIRTUALSCREEN`/`SM_YVIRTUALSCREEN`）与 `src/platform/windows/misc.cpp`（`SM_MOUSEPRESENT`）；无 `SM_REMOTESESSION`、无 `WTSIsRemoteSession`。相反它在 `display_base.cpp` 里 `GetProcAddress(user32, "SetProcessDpiAwarenessContext")` 后调用 `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`（同一函数 `platf::dxgi::…` 所在文件） |

**为什么它们不该置位**：`SM_REMOTESESSION` 的语义是“本进程关联的是 Terminal Services 客户端会话”，这是会话创建属性，不是“有人在看我的屏幕”。第三方工具在 console 会话内做屏幕捕获 + 注入输入，会话本身仍是 console，所以 `SM_REMOTESESSION == 0`、`WTSClientProtocolType == 0`。

**可行的替代检测思路**（都需实测，均为 `不确定`）：

| 思路 | 依据 | 风险 |
|---|---|---|
| 枚举输入/捕获驱动与进程（`EnumDeviceDrivers` 取驱动列表；WMI/SetupAPI 查镜像驱动或 IDD 设备；查 `TeamViewer_Service.exe`/`anydesk.exe`/`SunloginClient.exe`/`ToDesk_Service.exe`/`sunshine.exe`/`parsecd.exe`） | `EnumDeviceDrivers` 真实存在（psapi.h），但 **Windows 11 24H2 起需要 `SeDebugPrivilege` 才返回有效地址**，否则 `lpImageBase` 全是 NULL：<https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-enumdevicedrivers>。进程名清单来自各产品安装目录 `[推断]` | 进程名可伪装；镜像驱动方案在 Win10+ 已被 IDD 取代；`EnumDeviceDrivers` 不给驱动名，需再 `GetDeviceDriverBaseName` 才能匹配 |
| 检测 `DXGI_OUTPUT_DESC`/显示拓扑是否出现新的 IDD 显示器 | Parsec/VDD 类工具会加虚拟屏 <https://support.parsec.app/hc/en-us/articles/32381178803604-VDD-Overview-Prerequisites-and-Installation> | 只在“加虚拟屏”类工具成立 |
| 检测 `SM_REMOTECONTROL`（0x2001） | 官方语义是 RDS 会话被 shadow | 只覆盖 Windows 自带 shadow，不覆盖第三方 |
| 检测是否有进程持有 `DXGI` duplication / 连续编码 | 无官方文档 | 需自研，误报高 |
| 直接问用户 / 提供“远程模式”手动开关 | — | 最可靠 |

**落地代价（若只做 RDP）**：低（`GetSystemMetrics` + `GlassSessionId` 兜底 + `WTSClientProtocolType` 交叉验证）；**权限**：无；**签名**：无。
**落地代价（若要覆盖第三方）**：高且不确定，必须转向进程/驱动启发式，误报会直接导致“用户什么都没做窗口就乱跳”。

---

## 8. 服务 / 提权 helper

| 项 | 结论 | 依据 |
|---|---|---|
| 服务不能直接与用户交互 | **确定** | <https://learn.microsoft.com/en-us/windows/win32/services/interactive-services>：“Services cannot directly interact with a user as of Windows Vista”；“All services run in Terminal Services session 0”；`SERVICE_INTERACTIVE_PROCESS` 被 `NoInteractiveServices`（默认 1）禁用 |
| 官方推荐模式 | 服务 + 用户会话内 GUI 进程 + IPC（named pipe） | 同页：“Create a separate hidden GUI application and use the CreateProcessAsUser function … Design the GUI application to communicate with the service through some method of interprocess communication (IPC), for example, named pipes”；并提示 “IPC can expose your service interfaces over the network unless you use an appropriate access control list (ACL)” |
| 取调用方 PID | `GetNamedPipeClientProcessId(HANDLE Pipe, PULONG ClientProcessId)`（winbase.h，Vista+） | <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid> |
| 取调用方 exe 路径 | `QueryFullProcessImageNameW(hProcess, 0, buf, &size)`，需 `PROCESS_QUERY_LIMITED_INFORMATION` | <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew> |
| 校验 exe 签名 | `WinVerifyTrust(hwnd, &WINTRUST_ACTION_GENERIC_VERIFY_V2, &data)`；返回 **0** 才算成功（不要用 `SUCCEEDED`） | <https://learn.microsoft.com/en-us/windows/win32/api/wintrust/nf-wintrust-winverifytrust> |
| `CreateProcessAsUser` 的代价 | 需要 `SE_INCREASE_QUOTA_NAME`，token 不可指派时还需 `SE_ASSIGNPRIMARYTOKEN_NAME`；要交互必须传 `winsta0\default` 并**改 window station 与 desktop 的 DACL**；不加载用户 profile（`HKCU` 不可用，需 `LoadUserProfile`） | <https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw>（Remarks 全文） |
| 更简单的替代 | **计划任务 + 登录触发器 + `TASK_RUNLEVEL_HIGHEST`**（提权）；或用户态进程自启（不提权） | <https://learn.microsoft.com/en-us/windows/win32/taskschd/principal-runlevel>（`TASK_RUNLEVEL_HIGHEST` = 1；“If a task is registered using the Builtin\Administrator account or Local System/Local Service, RunLevel is ignored”）；触发器类型 <https://learn.microsoft.com/en-us/windows/win32/taskschd/task-triggers>（“when a user logs on to the local computer”） |

**推荐架构**：

```
[Windows 服务 (LocalSystem, Session 0)]  ← 仅做：开机、watchdog、崩溃恢复
        │  \\.\pipe\conflux-helper  (ACL: 仅本机、仅交互用户)
        │  校验: GetNamedPipeClientProcessId → QueryFullProcessImageNameW
        │        → 路径白名单 + WinVerifyTrust 验签
        ▼
[用户会话内 helper (Session N, 与桌面同 IL)]  ← 做全部 Win32 窗口/DPI/显示拓扑操作
```

- **不要**试图从服务里 `EnumWindows` 或 `SetWindowPos`：跨会话必失败（见第 1 条引文）。
- 若必须提权操作高 IL 窗口：让 helper 由**计划任务以 HIGHEST 运行**，而不是每次 UAC 弹窗；但注意高 IL 进程会被用户误认为危险，且高 IL 进程的 `EnumWindows` 会看到更多窗口。
- 若采用 UIAccess（Medium+，可操作 `osk.exe` 这类窗口）：必须 Authenticode 签名 + 装在 `Program Files` + manifest `uiAccess="true"`（第 1 条依据）。

**落地代价**：中（服务 + 安装器 + IPC 鉴权 + 会话切换处理）；**权限**：服务安装需管理员一次；**签名**：服务 exe 建议签名（SmartScreen/杀软），UIAccess 则强制签名。

---

## 9. 开机自启

| 方式 | 结论 | 限制 | 依据 |
|---|---|---|---|
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` | 可行（用户态、不提权） | 值长度 ≤ 260 字符；同键多值时顺序不确定；系统可能延迟启动；“The system does not provide guarantees about how promptly the programs in the Run key are run” | <https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys> |
| `HKLM\...\Run` / `HKLM\...\RunOnce` | 可行，需管理员写 | `HKLM\...\RunOnce` **只在 Administrators 组成员登录后执行** | 同上（Important 段） |
| 计划任务（登录触发） | 可行，**唯一能“提权且不弹 UAC”的自启方式** | 注册需管理员；`RunLevel=HIGHEST`；以 LocalSystem 注册时 RunLevel 被忽略 | <https://learn.microsoft.com/en-us/windows/win32/taskschd/principal-runlevel>、<https://learn.microsoft.com/en-us/windows/win32/taskschd/task-triggers> |
| `uap5:StartupTask`（UWP/MSIX） | 有条件可行 | 必须在 **MSIX 包 manifest** 里声明（`uap5:Extension/uap5:StartupTask`，Win10 1709+）；需要包标识（打包安装）；用户可在任务管理器关闭（`UserConfigurable`） | <https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap5-startuptask> |

**对“需要管理员权限的 helper”场景的结论**：

- **Run 键不行**：Run 键启动的进程继承登录用户的**标准（filtered）token**，不会提权 `[推断]`（文档只写“run when a user logs on”，未涉及 token 过滤）。验证方法：Run 键放一个只读 `\\.\PHYSICALDRIVE0` 的测试程序，看是否 `ERROR_ACCESS_DENIED`，或 Process Explorer 看 Integrity = Medium。
- **StartupTask 不行**：需要 MSIX 打包，且 MSIX 应用默认低权限/受 AppContainer 约束，装驱动/服务需额外 capability 与打包声明；`不确定` 能否随包安装 UMDF 驱动。验证方法：用 MSIX + `windows.deviceDriver` 扩展做最小验证（<https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap5-startuptask> 页未列出驱动相关扩展）。
- **正确选择**：**计划任务（登录触发 + HIGHEST）** 启动用户会话内的 helper；服务只负责 watchdog。二者都由安装器一次注册（需管理员）。

**落地代价**：低；**权限**：注册计划任务/服务需管理员；**签名**：无强制要求，但签名可避免 SmartScreen 与部分 EDR 告警。

---

## 10. 窗口状态细节

| 项 | 结论 | 依据 |
|---|---|---|
| `GetWindowPlacement().rcNormalPosition` 是“还原态”矩形 | 确定；`showCmd` 为 `SW_SHOWMAXIMIZED`/`SW_SHOWMINIMIZED`/`SW_SHOWNORMAL`；`flags` 读出时恒为 0 | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowplacement> |
| 坐标系是 **workspace 坐标**（非屏幕坐标），且**不能**传给 `SetWindowPos` | 确定：非 `WS_EX_TOOLWINDOW` 顶层窗口的 `ptMinPosition`/`ptMaxPosition`/`rcNormalPosition` 是 workspace 坐标；“Passing workspace coordinates to functions which expect screen coordinates (such as SetWindowPos) will result in the window appearing in the wrong location”，并举例“creep up the screen” | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement> |
| 最大化窗口的 `rcNormalPosition` | 保存的是**最大化之前**的还原矩形，因此跨屏搬最大化窗口应改它而不是 `SetWindowPos` | 同页（“The window's coordinates when the window is in the restored position”） |
| `SetWindowPlacement` 越界会被系统修正 | 确定：“If the information specified in WINDOWPLACEMENT would result in a window that is completely off the screen, the system will automatically adjust the coordinates so that the window is visible” | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowplacement> |
| `WM_GETMINMAXINFO` 的最小尺寸约束 | 由**目标应用自己的** `WindowProc` 处理，我们跨进程无法修改；`DefWindowProc` 在 `WS_OVERLAPPED`/`WS_THICKFRAME` 窗口收到 `WM_WINDOWPOSCHANGING` 时发出该消息 | <https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-getminmaxinfo>、<https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-windowposchanging>（“For a window with the WS_OVERLAPPED or WS_THICKFRAME style, the DefWindowProc function sends the WM_GETMINMAXINFO message … to validate the new size and position”） |
| → 后果 | `SetWindowPos` 可能被目标应用**静默夹取**，返回值仍为 TRUE；必须回读校验 | `[推断]`，验证方法：对记事本（可设最小尺寸的 Electron 应用）设 100×100，回读 `GetWindowRect` |
| Snap / snapped 窗口 | **没有**公开 API 可以“程序化 snap”；snapped 状态不体现在 `GetWindowPlacement` 里；判断是否 snapped 用 `IsWindowArranged` | <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindowarranged>（“A snapped window … is considered to be arranged”；“the showCmd member … can have a value of SW_SHOWNORMAL even if the window is arranged”；且该函数**无 header/lib**，需 `LoadLibrary`+`GetProcAddress`）；MS Q&A 结论：“when a window is snapped the snapped location is not reflected in the information returned by calling GetWindowPlacement. The rcNormalPosition RECT is unchanged after snapping.” <https://learn.microsoft.com/en-gb/answers/questions/1848799/getwindowplacement()-different-values-for-windows> |
| `SetWindowPos` 后是否脱离 snap | `不确定`。文档未描述；实际表现依应用与 Windows 版本而异 | 验证方法：Win11 上把记事本 snap 到左半屏，外部 `SetWindowPos` 移动后调用 `IsWindowArranged`，观察是否变 FALSE |
| `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` 公开可用 | **是**，`shobjidl_core.h`，Win10+，`CoCreateInstance(CLSID_VirtualDesktopManager)`；另有 `GetWindowDesktopId`、`MoveWindowToDesktop` | <https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager>、<https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ivirtualdesktopmanager-iswindowoncurrentvirtualdesktop> |

**实践结论**：

- 快照要存**两套**：`WINDOWPLACEMENT`（用于同屏恢复/还原态）+ `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` 的物理矩形（用于跨屏缩放换算）+ `IsWindowArranged`/`IsZoomed`/`IsIconic` 三态（用于判断能不能 `SetWindowPos`）。
- 恢复顺序：先 `ShowWindow(SW_RESTORE)`（若 maximized）→ `SetWindowPos` 到目标物理矩形 → 若原本 maximized 再 `ShowWindow(SW_MAXIMIZE)`。全程 `SWP_NOZORDER | SWP_NOACTIVATE`。
- 被 snap 的窗口：只能还原成“普通窗口 + 相同矩形”，无法还原 snap 关系；且移动会大概率解除 snap 分组。→ 产品文案应承认这一点。

**落地代价**：中；**权限**：无；**签名**：无。

---

## 11. 自我纠正的应用（Electron / Chromium）

| 项 | 结论 | 依据 |
|---|---|---|
| Chromium 会**主动改回**外部给的窗口位置 | **有代码级证据** | Chromium `ui/views/win/hwnd_message_handler.cc`，符号 `HWNDMessageHandler::OnWindowPosChanging(WINDOWPOS*)`：当“同一显示器 + (maximized bounds 不正确 \|\| fullscreen \|\| work area 变化)”且 `!IsWindowArranged(hwnd())` 时，它直接**覆写** `window_pos->x/y/cx/cy` 为 `expected_maximized_bounds`/`monitor_rect`/`AdjustToFit(work_area)`，并设置 `ignore_window_pos_changes_ = true` 忽略紧随其后的 `SetWindowPos` |
| 同上，源码位置 | 通过 jsDelivr 镜像读取的 `chromium/chromium@main` 同路径文件，`OnWindowPosChanging` 在约 3109 行，`ignore_window_pos_changes_` 赋值在约 3216 行 | 镜像：`https://cdn.jsdelivr.net/gh/chromium/chromium@main/ui/views/win/hwnd_message_handler.cc`（原始位置 `https://chromium.googlesource.com/chromium/src/+/main/ui/views/win/hwnd_message_handler.cc`） |
| 多显示器 + 不同缩放时 Electron 自己定位就错 | **有 issue 证据** | `electron/electron` issue #10862 “Per monitor DPI awareness causes issues with window positioning and sizing”（open，2017 起）：“If you have one monitor with non-100% scaling and one monitor with 100% scaling both setSize and setPosition do not act as expected (they always set incorrectly by a factor exactly equal to the scale factor of the screen)”；后续评论给出 workaround：连续多次 `setBounds` 直到稳定 |
| 外部 `SetWindowPos` 对 Electron 窗口**无效** | **有实证** | StackOverflow 77078240 “SetWindowPos doesn't work with VSCode windows”：`SetWindowPos` 返回 `True` 但窗口不动；作者发现 NirSoft GUIPropView 能移动，说明是子 HWND/宿主窗口结构问题 |
| 结论 | 有条件可行：需要“写后校验 + 重试” | — |

**建议实现**：

1. 记录操作前的 `GetWindowRect`/`DWMWA_EXTENDED_FRAME_BOUNDS`。
2. `SetWindowPos`。
3. 延时 50–150ms 回读；若偏离 > 2px，**最多重试 2–3 次**（Electron issue #10862 里被广泛采用的 workaround 就是重复 set）。
4. 对 `IsZoomed` 为真的窗口**不要**直接 `SetWindowPos`（Chromium 的 `OnWindowPosChanging` 会覆写），走 `ShowWindow(SW_RESTORE)` → 移动 → `SW_MAXIMIZE`。
5. 对 `IsWindowArranged` 为真的窗口，先接受“会脱 snap”的事实，再移动。
6. 对全屏（`WS_POPUP` + 覆盖整屏）窗口跳过，避免触发 Chromium 的 `fullscreen_without_hack` 分支。

**落地代价**：中（重试/校验状态机 + 每窗口状态机，避免与用户操作打架）；**权限**：无；**签名**：无。

---

## 风险最高的三条

### 1. 第 7 条：第三方远控基本不会置位 `SM_REMOTESESSION` —— 产品核心前提在 Windows 上不成立

`SM_REMOTESESSION`（0x1000）与 `WTSClientProtocolType==2` 只反映“本进程在不在 Terminal Services 客户端会话里”，由 RDP 创建会话时决定。TeamViewer/AnyDesk/RustDesk/ToDesk/向日葵/Parsec/Sunshine 都是**附着在已有 console 会话上做捕获与注入**：AnyDesk 官方文档甚至要求“该用户必须已登录该会话（通常需要先开 RDP）”；RustDesk 源码里 `WTSClientProtocolType` 只用于**挑选**会话，从不写入；Sunshine 全仓库没有 `SM_REMOTESESSION`。因此对绝大多数真实用户，`SM_REMOTESESSION` 永远是 0，检测永远不会触发。
更糟的是：**RDP 场景下产品逻辑本身是错的**——RDP 会话有独立的虚拟显示，物理副屏不参与，“把窗口搬到被捕获的屏”没有意义。
→ 必须把“检测”重新定义为“用户显式声明 / 进程与驱动启发式 / 与远控软件集成”，并且不能把 RDP 当成主场景。

### 2. 第 5 条：禁用显示器在远程会话里直接 `ERROR_ACCESS_DENIED`，且失败后可能永久黑屏

`SetDisplayConfig`/`QueryDisplayConfig` 的官方返回码明确列出：进程不在 console 会话（包括“running on a remote session”）时返回 `ERROR_ACCESS_DENIED`。这意味着**一旦检测到远程访问，我们恰好失去了改显示拓扑的能力**；必须依赖 console 会话里的另一个进程（服务 + IPC），而服务又不能直接操作用户桌面（Session 0 隔离）。
同时 `SDC_APPLY` 不写数据库，进程崩溃/断电会让机器停在“少一块屏”的拓扑上；恢复调用本身也可能返回 `ERROR_BAD_CONFIGURATION`（需 `SDC_ALLOW_CHANGES`）。对普通用户而言，“屏幕没了且恢复不了”是卸载级事故。
→ 禁用显示器不应作为默认路径；默认只做窗口搬迁（零副作用）。若做，必须有独立 watchdog 与“上次未恢复”持久标记。

### 3. 第 6 条：虚拟显示器驱动必须微软签名，ARM64 事实上只有 WHQL 一条路

Win10 1607 起“Windows will not load any new kernel mode drivers which are not signed by the Microsoft through the Hardware Dev Center”，虚拟驱动同样适用。attestation 签名虽然够用（EV 证书 + Partner Center，免费但需审核），但**不能发布到 Windows Update 面向零售用户**，也不能用于 Windows Server 2016+。ARM64 更严：VDD issue #465 的 `setupapi.dev.log` 显示证书链完全可信、TrustedPublisher 检查通过，仍被 Code Integrity 以 `0x800b0109` 拒绝导入，社区结论是 ARM64 无法关闭 Secure Boot/开 testsigning。
→ 这条会把产品从“桌面工具”变成“带驱动的系统软件”，成本量级完全不同（HLK 测试、EV 证书年费、Partner Center 流程、安装器、蓝屏责任）。而且窗口搬迁**根本不需要** IDD，应彻底解耦。

---

## 附：本文件引用到的全部页面

官方（learn.microsoft.com）：
`winuser/nf-winuser-enumwindows`、`nf-winuser-getwindowthreadprocessid`、`nf-winuser-getwindowrect`、`nf-winuser-getdpiforwindow`、`nf-winuser-adjustwindowrectexfordpi`、`nf-winuser-setprocessdpiawarenesscontext`、`nf-winuser-setwindowpos`、`nf-winuser-showwindow`、`nf-winuser-getwindowplacement`、`nf-winuser-setwindowplacement`、`nf-winuser-getmonitorinfow`、`nf-winuser-enumdisplaymonitors`、`nf-winuser-enumdisplaydevicesw`、`nf-winuser-querydisplayconfig`、`nf-winuser-setdisplayconfig`、`nf-winuser-changedisplaysettingsexw`、`nf-winuser-changewindowmessagefilterex`、`nf-winuser-getsystemmetrics`、`nf-winuser-iswindowarranged`、`nf-winuser-physicaltologicalpointforpermonitordpi`、`ns-winuser-windowplacement`、`ns-winuser-monitorinfo`、`ne-windef-dpi_awareness`（经 hidpi 页引用）、`hidpi/dpi-awareness-context`、`hidpi/wm-dpichanged`、`hidpi/high-dpi-desktop-application-development-on-windows`、`hidpi/high-dpi-improvements-for-desktop-applications`、`gdi/wm-displaychange`、`gdi/hmonitor-and-the-device-context`、`winmsg/wm-settingchange`、`winmsg/wm-getminmaxinfo`、`winmsg/wm-windowposchanging`、`devio/wm-devicechange`、`dwmapi/nf-dwmapi-dwmgetwindowattribute`、`dwmapi/ne-dwmapi-dwmwindowattribute`、`wingdi/ns-wingdi-displayconfig_target_device_name`、`wingdi/ns-wingdi-displayconfig_path_info`、`wingdi/ns-wingdi-displayconfig_path_target_info`、`wtsapi32/nf-wtsapi32-wtsquerysessioninformationa`、`wtsapi32/ne-wtsapi32-wts_info_class`、`termserv/detecting-the-terminal-services-environment`、`shellscalingapi/nf-shellscalingapi-getdpiformonitor`、`shellscalingapi/ne-shellscalingapi-monitor_dpi_type`、`shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager`、`shobjidl_core/nf-shobjidl_core-ivirtualdesktopmanager-iswindowoncurrentvirtualdesktop`、`winbase/nf-winbase-getnamedpipeclientprocessid`、`winbase/nf-winbase-queryfullprocessimagenamew`、`wintrust/nf-wintrust-winverifytrust`、`psapi/nf-psapi-enumdevicedrivers`、`nf-winuser-monitorfromwindow`、`nf-winuser-iszoomed`、`processthreadsapi/nf-processthreadsapi-createprocessasuserw`、`services/interactive-services`、`taskschd/task-triggers`、`taskschd/principal-runlevel`、`setupapi/run-and-runonce-registry-keys`、`winauto/uiauto-securityoverview`、`troubleshoot/.../ui-automation/uipi-issues`、`uwp/api/windows.devices.display.core.displaymanager`、`uwp/schemas/appxpackage/uapmanifestschema/element-uap5-startuptask`
驱动/签名：`windows-hardware/drivers/display/indirect-display-driver-model-overview`、`drivers/display/ccd-apis`、`drivers/dashboard/code-signing-attestation`、`drivers/dashboard/driver-signing-offerings`、`drivers/dashboard/code-signing-reqs`、`drivers/install/kernel-mode-code-signing-requirements--windows-vista-and-later-`、`drivers/install/inf-version-section`

真实仓库 / 社区实证：
`rustdesk/rustdesk`（`src/platform/windows.cc`: `is_rdp_session_by_protocol`、`get_current_session`）、`LizardByte/Sunshine`（`src/platform/windows/display_base.cpp`、`src/platform/windows/misc.cpp`）、`chromium/chromium`（`ui/views/win/hwnd_message_handler.cc`: `HWNDMessageHandler::OnWindowPosChanging`）、`electron/electron` issue #10862、`VirtualDrivers/Virtual-Display-Driver`（README + issue #465 + 文档站 `development/driver-signing`）、`nomi-san/parsec-vdd`、StackOverflow 77078240 / 29142491、Microsoft Q&A 1848799 / 2152111、AnyDesk 官方 Windows Server 文档、TeamViewer 官方 Windows Server 文档、Parsec 官方 VDD 文档。
