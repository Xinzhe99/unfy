# macOS 技术事实核查（Conflux）

核查日期：2026-10-21。证据分三级：
- **[官方]** developer.apple.com 文档页 / 公开 SDK 头文件（phracker/MacOSX-SDKs 镜像的 Apple 头文件，路径写明）。
- **[源码]** 真实开源仓库的具体文件 + 符号名（yabai / AeroSpace / Chromium / Rectangle / Hammerspoon）。
- **[不确定]** 无直接证据，给出验证方法。

SDK 头文件镜像根：`https://github.com/phracker/MacOSX-SDKs`（内容为 Apple 原版头文件，仅作可点击定位用；关键 API 均已同时在 developer.apple.com 上核对存在性）。

---

## 汇总表

| # | 主题 | 结论 | 需要授权 | 主要风险 |
|---|---|---|---|---|
| 1 | AX 窗口枚举/移动 | 可行 | Accessibility(TCC) | 非标准窗口不可控；MAS 沙箱基本不可行 |
| 2 | 窗口标识 | 有条件可行 | Accessibility | `kAXWindowNumberAttribute` **不存在**；CGWindowID 仅会话内唯一 |
| 3 | 显示器身份 | 有条件可行 | 无 | 序列号可为 0；应改用 UUID |
| 4 | 显示器变更通知 | 可行 | 无 | 回调不在主线程；Tauri 需切主线程 |
| 5 | 原生全屏 Space | 不可行（直接迁移） | Accessibility | AX 看不到其他 Space 的窗口；全屏窗口需先退出全屏 |
| 6 | 虚拟显示器 CGVirtualDisplay | 私有 API，不可上架 MAS | 私有 API | App Store 必拒；公证未获官方保证；随系统版本失效 |
| 7 | 远程/被捕获检测 | **部分可行；"谁在录屏"不可行** | Screen Recording（仅读窗口名） | 无公开 API 查询"谁在捕获" |
| 8 | 防休眠 | 可行 | 无 | 断言类型已更名，旧名 deprecated |
| 9 | 权限模型/TCC | 有条件可行 | 签名证书 | ad-hoc 签名导致授权每次重建丢失 |
| 10 | 窗口状态重建 | 有条件可行 | Accessibility | 尺寸不精确，需二次设置；zoom 无 AX 属性 |
| 11 | 更新分发 | 可行 | Developer ID + 公证 | arm64 必须至少 ad-hoc 签名 |

---

## 1. 窗口枚举与控制

| 项 | 内容 |
|---|---|
| 结论 | **可行**（对"标准可移动窗口"）；对非标准窗口不可行 |
| 依据 | [官方] `kAXWindowsAttribute`："An array of accessibility objects representing this application's windows."（https://developer.apple.com/documentation/applicationservices/kaxwindowsattribute ）<br>[官方] SDK `AXUIElement.h`：`extern AXUIElementRef AXUIElementCreateApplication(pid_t pid);`（"Creates and returns the top-level accessibility object for the application with the specified process ID."）<br>[官方] `AXUIElementSetAttributeValue` 文档存在（https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue ）<br>[官方] SDK `AXUIElement.h`：`extern AXError AXUIElementSetAttributeValue(AXUIElementRef, CFStringRef, CFTypeRef)`；`AXUIElementCopyAttributeValue`；`AXUIElementIsAttributeSettable`（`.../HIServices.framework/Versions/A/Headers/AXUIElement.h`）<br>[官方] SDK `AXValue.h`：`AXValueCreate(AXValueType, const void*)`、`kAXValueTypeCGPoint=1`、`kAXValueTypeCGSize=2`<br>[官方] `AXAttributeConstants.h`：`kAXPositionAttribute` = "AXPosition"，注明 *"Writable? Generally no. However, some elements that can be moved by the user through direct manipulation (like windows) should offer a writable position attribute."*；`kAXSizeAttribute` 同 |
| 落地代价 | 低-中：FFI 绑定 3 个函数 + 2 个常量；每次操作前用 `AXUIElementIsAttributeSettable` 探测（yabai 的 `window_ax_can_move` / `window_ax_can_resize` 就是这个模式，`asmvik/yabai/src/window.c`） |
| 权限/签名 | 必须 Accessibility（TCC）。检测用 `AXIsProcessTrustedWithOptions`，提示键 `kAXTrustedCheckOptionPrompt`（SDK `AXUIElement.h` 注释："Prompting occurs asynchronously and does not affect the return value."） |
| 哪些应用拒绝 | [源码] Magnet 官方 FAQ："Magnet can not do anything with non-standard windows used by some developers… Adobe Creative Suite & Creative Cloud apps older than 2017 version is not supported"（https://magnet.crowdcafe.com/faq.html ）；[官方] `AXUIElementSetAttributeValue` 会返回 `kAXErrorAttributeUnsupported` / `kAXErrorIllegalArgument`（SDK 注释）。**没有** Apple 公开的"哪些 App 拒绝"清单 |
| 沙箱/公证 | 公证不限制 AX 使用（公证=自动恶意代码扫描，见第 6 节依据）；**沙箱**是另一回事：App Review 2.4.5(i) 要求 MAS 应用必须沙箱（https://developer.apple.com/app-store/review/guidelines/ ）。沙箱进程能否持有 Accessibility 授权 **不确定**；验证方法：用沙箱+MAS 签名的空壳 App 调 `AXIsProcessTrusted()`，看能否在"辅助功能"里被勾选 |
| 备注 | 沙箱版 Tauri 若走 MAS 路线，本产品核心功能（移动其他进程窗口）大概率不可行；建议走 Developer ID 直发 |

---

## 2. 窗口标识稳定性

| 项 | 内容 |
|---|---|
| 结论 | **有条件可行**：会话内用 `kCGWindowNumber`；跨重启必须自建映射 |
| 依据 | [官方] SDK `CGWindow.h`：*"The window ID, a unique value within the user session representing the window."*；`kCGWindowNumber`/`kCGWindowOwnerPID`/`kCGWindowLayer`/`kCGWindowBounds` 均为 32-bit signed CFNumber；`kCGWindowBounds` 原点在**主显示器左上角**（`.../CoreGraphics.framework/Versions/A/Headers/CGWindow.h`）<br>[官方] `CGWindowListCopyWindowInfo`："Generates and returns information about the selected windows in the current user session."（https://developer.apple.com/documentation/coregraphics/cgwindowlistcopywindowinfo(_:_:) ）<br>[官方] Required Window List Keys：`kCGWindowNumber` 等 8 个键保证存在（https://developer.apple.com/documentation/coregraphics/required-window-list-keys ）；`kCGWindowName`/`kCGWindowOwnerName` 在 Optional 键中<br>[源码] AX→CGWindowID 的**唯一**现实手段是私有函数：`extern AXError _AXUIElementGetWindow(AXUIElementRef ref, uint32_t *wid);`（`asmvik/yabai/src/misc/extern.h`；`rxhanson/Rectangle/Rectangle/Rectangle-Bridging-Header.h` 同） |
| **证伪** | 规格里的 `kAXWindowNumberAttribute` **不存在**：在 SDK 10.15 / 11.3 的 `AXAttributeConstants.h` 中检索 `AXWindowNumber` 结果为 0；`accessibility-sys` 0.2.0 的 `attribute_constants.rs` 也没有。**不要写这个常量名** |
| 落地代价 | 中：需要 `(pid, CGWindowID, bundleID, 窗口标题, 首次出现时的 bounds)` 组合指纹 + 持久化；CGWindowID 会复用 |
| 权限/签名 | 枚举本身不需要 TCC；但 `kCGWindowName` 需要 Screen Recording（见第 7 节） |
| 备注 | 跨重启"还原到同一窗口"本质不可靠；建议只在**同一次会话内**保证精确恢复，重启后降级为"按 App + 标题模糊匹配" |

---

## 3. 显示器身份

| 项 | 内容 |
|---|---|
| 结论 | **有条件可行**：用 `CGDisplayCreateUUIDFromDisplayID` 作为稳定 ID；序列号只能作辅助 |
| 依据 | [官方] `CGGetActiveDisplayList`："Provides a list of displays that are active for drawing."（https://developer.apple.com/documentation/coregraphics/cggetactivedisplaylist(_:_:_:) ）；SDK 注释："The first display returned in the list is the main display (the one with the menu bar)."<br>[官方] `CGDisplayCreateUUIDFromDisplayID(_ displayID: UInt32) -> Unmanaged<CFUUID>!`，macOS 10.13+，框架 **ColorSync**（https://developer.apple.com/documentation/colorsync/cgdisplaycreateuuidfromdisplayid(_:) ）；反向 `CGDisplayGetDisplayIDFromUUID(_ uuid: CFUUID!) -> UInt32`（https://developer.apple.com/documentation/colorsync/cgdisplaygetdisplayidfromuuid(_:) ）<br>[官方] `CGDisplaySerialNumber`：若厂商未编码序列号返回 `0x00000000`，若无显示器返回 `0xFFFFFFFF`，"a serial number is meaningful only in conjunction with a specific vendor and product or model"（https://developer.apple.com/documentation/coregraphics/cgdisplayserialnumber(_:) ）<br>[官方] `CGDisplayIsMain`："Returns a Boolean value indicating whether a display is the main display."；`CGMainDisplayID` 注释："The main display is the display with its screen location at (0,0) in the global display coordinate space."（https://developer.apple.com/documentation/coregraphics/cgdisplayismain(_:) 、.../cgmaindisplayid() ）<br>[官方] `NSScreen.deviceDescription`：用 key `@"NSScreenNumber"` 取 `CGDirectDisplayID`（https://developer.apple.com/documentation/appkit/nsscreen/devicedescription ）<br>[源码] yabai 用 UUID 做显示器主键：`display_uuid()` / `display_id()`（`asmvik/yabai/src/display.c`） |
| 证伪 | 规格里的 `kCGDisplaySerialNumber` / `kCGDisplayVendorNumber` / `kCGDisplayModelNumber` **是函数不是键**：正确名字为 `CGDisplaySerialNumber(display)`、`CGDisplayVendorNumber(display)`、`CGDisplayModelNumber(display)`。真正的键名是 IOKit 的 `kDisplaySerialNumber`（`"DisplaySerialNumber"`）、`kDisplayVendorID`、`kDisplayProductID`（`.../IOKit.framework/Versions/A/Headers/graphics/IOGraphicsTypes.h`） |
| 落地代价 | 低：一次枚举 + UUID 字符串化 |
| 权限/签名 | 无 |
| 备注 | `CGDisplayIsMain` 语义 = 菜单栏所在显示器（全局坐标 (0,0)），**不是**"笔记本内屏"，也不是"被捕获的屏" |

---

## 4. 显示器配置变更通知

| 项 | 内容 |
|---|---|
| 结论 | **可行**；但必须处理线程模型 |
| 依据 | [官方] `CGDisplayRegisterReconfigurationCallback(_ callback: CGDisplayReconfigurationCallBack?, _ userInfo: UnsafeMutableRawPointer?) -> CGError`（https://developer.apple.com/documentation/coregraphics/cgdisplayregisterreconfigurationcallback(_:_:) ）；SDK `CGDisplayConfiguration.h` 明确：*"Callbacks are invoked when the app is listening for events, **on the event processing thread**, or from within the display reconfiguration function…"*，且"Before display reconfiguration, a callback fires…flag set to `kCGDisplayBeginConfigurationFlag`"；"After display reconfiguration…all display state … will be up to date"<br>[官方] `NSApplication.didChangeScreenParametersNotification`："Posted when the configuration of the displays attached to the computer is changed." 并明确 *"The system posts this notification on the main actor."*（https://developer.apple.com/documentation/appkit/nsapplication/didchangescreenparametersnotification ）<br>[官方] Tauri `AppHandle::run_on_main_thread`（https://docs.rs/tauri/latest/tauri/struct.AppHandle.html#method.run_on_main_thread ） |
| 落地代价 | 低：注册一次；回调里只投递事件，不做事 |
| 权限/签名 | 无 |
| 备注 | **Tauri 的坑**：`CGDisplayRegisterReconfigurationCallback` 回调不保证在主线程（且**只有 App 正在事件循环里收事件时**才会被调用，见头文件原文），回调里禁止改显示配置、禁止 longjmp。Rust 侧需 `Send` 化后 `run_on_main_thread` 处理；`begin` 与 `end` 两次回调要按 flag 去重，否则会触发两次迁移 |

---

## 5. 原生全屏 Space 与窗口迁移

| 项 | 内容 |
|---|---|
| 结论 | **不可行**（"把原生全屏窗口原样移到另一块屏"）。可行路径只有：先置 `AXFullScreen=false` 退出全屏 → 移动 → 需要时再置回；或直接跳过并提示用户 |
| 依据（全屏在独立 Space） | [官方] Apple 归档文档 *Implementing the Full-Screen Experience*：*"When the user chooses to enter full-screen mode, Cocoa **dynamically creates a space** and puts the window into that space."*（https://developer.apple.com/library/archive/documentation/General/Conceptual/MOSXAppProgrammingGuide/FullScreenApp/FullScreenApp.html ）<br>[源码] Hammerspoon `hs.spaces.windowsForSpace` 注释：*"due to the way Accessibility objects work, **only those window IDs that are present on the currently visible spaces** will be finable with `hs.window` or exist within `hs.window.allWindows()`"*（`Hammerspoon/hammerspoon/extensions/spaces/libspaces.m`） |
| 依据（逐 API：能否看到其他 Space 的窗口） | **AX 路径：不能**。AX 只能拿到当前可见 Space 上窗口的 `AXUIElement`（Hammerspoon 注释同上）。<br>**CGWindowList 路径：能，但必须用 `kCGWindowListOptionAll`**——`optionOnScreenOnly` 只返回当前可见窗口（"List all windows that are currently onscreen."），`optionAll` 则"including both onscreen and offscreen windows"（https://developer.apple.com/documentation/coregraphics/cgwindowlistoption/optiononscreenonly 、.../optionall ）；xcap 的提交 *"feat(macos): include off-screen windows from other Spaces"* 正是把 `OptionOnScreenOnly` 换成 `OptionAll` 以"see windows on other macOS desktops/Spaces"（nashaofu/xcap commit `59302730`，`src/macos/impl_window.rs`）。<br>**注意**：`optionAll` 拿到的是"有字典但可能 `kCGWindowIsOnscreen` 缺失/为 false"的条目，且**没有 AXUIElement 与之对应**，因此"看得见 ≠ 能操作"。<br>**按 Space 精确枚举**需要私有 SkyLight（`SLSCopyWindowsWithOptionsAndTags` + `SLSWindowQueryWindows`，`asmvik/yabai/src/space.c`） |
| 依据（AX 位置对全屏窗口无效） | 无 Apple 明文。间接：[官方] `kAXPositionAttribute` 注明 *"Writable? Generally no."*；[源码] AeroSpace 在全屏窗口上**跳过**设 frame（`Sources/AppBundle/tree/MacWindow.swift`，`unhideFromCorner()` 的 `switch` 里 `.macosNativeFullscreenWindow` 分支为 `break`），并把原生全屏窗口放进独立容器 `.macosFullscreenWindowsContainer`（同文件 `layoutReason = .macos(prevParentKind:)` 注释："macOS native fullscreen, minimize, or hide"）；yabai 在 `window_manager_apply_rules_to_window` 中对 `WINDOW_FULLSCREEN` 直接 `goto out` 不做布局（`asmvik/yabai/src/window_manager.c`）<br>→ **强间接证据**；"设置被静默忽略"这一点 **不确定**，验证方法：对 `AXFullScreen=true` 的窗口调 `AXUIElementSetAttributeValue(kAXPositionAttribute)`，记录返回的 `AXError` 并回读 `kAXPositionAttribute`，对比是否变化 |
| Displays have separate Spaces | [官方] `NSScreen.screensHaveSeparateSpaces`："reflects whether the 'Displays have separate Spaces' option is enabled in Mission Control system preference."（https://developer.apple.com/documentation/appkit/nsscreen/screenshaveseparatespaces ）<br>[源码] AeroSpace 文档：开启时"moving windows between monitors causes windows to move between different Spaces which is not correctly handled by the public APIs AeroSpace uses, apparently, these APIs are not aware about Spaces existence"（`AeroSpace/docs/guide.adoc`）；关闭时副作用是"第二台显示器黑屏"（同文件对比表） |
| 落地代价 | 中-高：需要全屏检测（`kAXFullScreenButtonAttribute` 存在，但布尔态属性 `AXFullScreen` 属**未公开常量**，yabai 自己定义 `const CFStringRef kAXFullscreenAttribute = CFSTR("AXFullScreen");`，见 `asmvik/yabai/src/window.h`；AeroSpace 用 `Ax.isFullscreenAttr`，`Sources/AppBundle/tree/MacApp.swift`） |
| 权限/签名 | Accessibility |
| 备注 | 产品策略建议：检测到 `AXFullScreen==true` 或 `space_is_fullscreen` 时**跳过并提示**，不要尝试强行迁移。`kAXFullscreenAttribute` 不在公开 SDK（10.15/11.3 头文件均无），只能硬编码字符串 `"AXFullScreen"` |

---

## 6. 虚拟显示器（CGVirtualDisplay，私有 API）

| 项 | 内容 |
|---|---|
| 结论 | **私有 API 真实存在**；技术上可创建虚拟屏；**App Store 必拒（2.5.1）；公证大概率可过但未获官方保证**；跨系统版本失效风险高 |
| 依据（真实存在） | [源码] Chromium `ui/display/mac/test/virtual_display_util_mac.mm`：注释 *"These interfaces were generated from CoreGraphics binaries."*，声明 `@interface CGVirtualDisplay`、`CGVirtualDisplayDescriptor`、`CGVirtualDisplaySettings`、`CGVirtualDisplayMode`，方法 `- (instancetype)initWithDescriptor:`、`- (BOOL)applySettings:`；`CGVirtualDisplayMode` 的 `- (id)initWithWidth:height:refreshRate:`；`CGVirtualDisplaySettings` 属性 `modes`、`hiDPI`、`rotation`（https://raw.githubusercontent.com/chromium/chromium/main/ui/display/mac/test/virtual_display_util_mac.mm ）<br>[源码] 第三方反向头文件 `CGVirtualDisplayPrivate.h`（peetzweg/opendisplay，`Mac/CGVirtualDisplayPrivate.h`），头注释："Private API: capped at 60 Hz and may break across macOS versions — personal/sideloaded use only."<br>[源码] 商用先例：domdomegg/displayoverride-mac README "Uses the private `CGVirtualDisplay` API from CoreGraphics"（https://github.com/domdomegg/displayoverride-mac ） |
| 依据（公证规则） | [官方] *Notarizing macOS software before distribution*：*"Notarization of macOS software is **not App Review**. The Apple notary service is an **automated system** that scans your software for malicious content, checks for code-signing issues…"*；列出的硬性要求只含：代码签名、Developer ID 证书、Hardened Runtime、secure timestamp、无 `get-task-allow`、10.9+ SDK、entitlements 格式 —— **不含"仅公开 API"这一条**（https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution ）<br>→ "公证不因私有 API 而拒绝"是**推断**：文档未承诺、也未禁止；notary 是否在扫描中检测私有符号，Apple 未公开说明。**不确定**，验证方法：用一个仅调用 `CGVirtualDisplay` 的最小 App 走 `notarytool submit --wait`，检查 notary log 是否有相关 issue |
| 依据（App Store） | [官方] App Review Guidelines **2.5.1**：*"Apps may only use public APIs and must run on the currently shipping OS."*（https://developer.apple.com/app-store/review/guidelines/ ）→ CGVirtualDisplay 必拒；且 2.5.19 禁止 hidden/undocumented features |
| 落地代价 | 高：ObjC 运行时桥（`NSClassFromString("CGVirtualDisplay")` + `msg_send!`），无头文件无符号，只能字符串/选择器调用；Chromium 代码里已带兼容补丁——*"macOS 14 expects different virtual displays to have different serial numbers"*、*"macOS 14 expects a non-zero vendorID"*、`g_need_display_removal_workaround` |
| 权限/签名 | 私有 API；Developer ID + 公证（技术上可行）；MAS 不可行 |
| 风险 | 每个 macOS 大版本都可能改私有类/属性；且**本产品根本不需要虚拟屏**（迁移窗口到被捕获的物理屏即可），引入私有 API 只增加拒审与崩溃面 |
| 备注 | **建议：不采用**。若未来要做"投屏给远程端"，用公开 `CGDisplayStream`/ScreenCaptureKit 采集，而不是伪造显示器 |

---

## 7. 远程会话 / 远程控制检测（关键条目）

| 项 | 内容 |
|---|---|
| 结论 | **"谁在录屏/哪块屏正被捕获" —— 公开 API 不存在，不可行**。可用的只有"会话/锁屏/在控制台"这类粗粒度信号 + 进程/端口启发式 |
| 依据（会话字典，公开） | [官方] `CGSessionCopyCurrentDictionary() -> CFDictionary?`（https://developer.apple.com/documentation/coregraphics/cgsessioncopycurrentdictionary() ）；键仅 5 个：`kCGSessionUserIDKey`、`kCGSessionUserNameKey`、`kCGSessionConsoleSetKey`、`kCGSessionOnConsoleKey`、`kCGSessionLoginDoneKey`（SDK `CGSession.h`；文档 https://developer.apple.com/documentation/coregraphics/window-server-session-properties ） |
| 证伪 | `CGSSessionScreenIsLocked` **不是公开键**：SDK `CGSession.h` 的键列表里没有它（只有上面 5 个），developer.apple.com 的 *Window Server Session Properties* 也没有。它属于未文档化键，且社区报告在不同版本上时有时无（StackOverflow "Querying whether a mac is in the locked state using Objective-C"：*"using a CGSSessionScreenIsLocked key fails because it is not part of the dictionary's entries"*）。**不能作为规格里的保证** |
| 依据（ScreenCaptureKit 语义） | [官方] `SCShareableContent`："An instance that represents a set of displays, apps, and windows **that your app can capture**"，属性只有 `windows` / `displays` / `applications`（https://developer.apple.com/documentation/screencapturekit/scshareablecontent ）→ 它是"可捕获对象清单"，**不含任何"当前是否正被他人捕获"的字段**<br>[官方] `SCDisplay` 只有 `displayID`、`frame`、`width`、`height`（https://developer.apple.com/documentation/screencapturekit/scdisplay ）→ **没有** `isCaptured`<br>[官方] `SCWindow.isActive`："A Boolean value that indicates if the window is currently streaming."（https://developer.apple.com/documentation/screencapturekit/scwindow/isactive ）——语义是"该窗口正在被（某路）流传输"，Apple 未声明它可用于反查第三方录制者，且不覆盖显示器级捕获<br>[官方] `SCStream.isCapturing`：只描述**你自己的** stream（"whether **this** stream is currently capturing screen content"，https://developer.apple.com/documentation/screencapturekit/scstream/iscapturing ）<br>[官方] 系统提示是 UI 层：Apple 支持文档 *'Your screen is being observed'* —— 只在锁屏显示提醒，无 API（https://support.apple.com/en-us/120315 ） |
| 结论展开 | macOS **没有** iOS 那种 `UIScreen.isCaptured` 的等价物 **[INFERENCE，基于符号缺失]**。`isCaptured` 属 UIKit（iOS 11+，文档标注到 27.0 结束），Mac Catalyst 上虽有同名属性，但原生 AppKit 的 `NSScreen`/`NSApplication` 在文档与 SDK 头文件中均无对应符号（https://developer.apple.com/documentation/uikit/uiscreen/iscaptured ）。因此"检测到本机正在被远程访问/被录制"只能做**启发式**： |
| 可行替代方案 | ① 进程名单：`NSRunningApplication` / `proc_listpids` 匹配已知远控（TeamViewer/Screens/AnyDesk/Chrome Remote Desktop/Parsec/VNC），可配 `lsof -i` 端口（5900 等）；代价低、误报高<br>② `CGSessionCopyCurrentDictionary` 的 `kCGSessionOnConsoleKey`（false = 会话不在控制台，快速用户切换/远程登录的强信号）<br>③ 显示拓扑变化：远控/无头会话常见"出现 `virt` 显示"——Chromium 的 `IsRunningHeadless()` 用 `CGDisplayModelNumber(display) != 0x76697274`（4CC `'virt'`）判断（`ui/display/mac/test/virtual_display_util_mac.mm` 的 `IsRunningHeadless()`；`remoting/host/curtain_mode_mac.cc` 同名函数）；这只是"疑似虚拟/无头"，不能证明有人在看<br>④ 屏幕锁定：`CGSessionCopyCurrentDictionary` + 通知 `com.apple.screenIsLocked`（未文档化，不确定）或直接看 `loginwindow` 是否有窗口 |
| `kCGWindowName` 与 Screen Recording | **需要** Screen Recording（二手证据）：Apple 论坛回复 *"Access to the kCGWindowName string seems to be gated by the Screen Recording user data protection."*（https://developer.apple.com/forums/thread/126860 ；该页有机器人校验，本条目引用的是搜索引擎对该页的索引摘录，未逐字读原页）。间接支持：[官方] `kCGWindowName` 文档注明 *"few applications set the Quartz window name"*，且 `kCGWindowName`/`kCGWindowOwnerName` 只在 Optional Window List Keys 中（https://developer.apple.com/documentation/coregraphics/kcgwindowname ）。权限查询用 [官方] `CGPreflightScreenCaptureAccess()` / `CGRequestScreenCaptureAccess()`（https://developer.apple.com/documentation/coregraphics/cgpreflightscreencaptureaccess() ）。<br>**验证方法**：未授予屏幕录制权限时枚举 `CGWindowListCopyWindowInfo(kCGWindowListOptionAll)`，统计其他进程窗口的 `kCGWindowName` 是否全为 nil；授权后重跑对比。 |
| 落地代价 | 低（启发式）/ 不可行（精确检测） |
| 权限/签名 | 若读窗口标题：Screen Recording（TCC，用户必须手动勾选，且 macOS 15 起频繁弹确认） |
| 备注 | **规格里必须删掉"查询哪块屏正在被捕获"这类 API 描述**。产品应改为"用户选择/规则指定要迁移到哪块屏"，而不是"自动识别被捕获的屏" |

---

## 8. 防屏幕休眠 / 电源断言

| 项 | 内容 |
|---|---|
| 结论 | **可行** |
| 依据 | [官方] SDK `IOPMLib.h`：`IOReturn IOPMAssertionCreateWithName(CFStringRef AssertionType, IOPMAssertionLevel AssertionLevel, CFStringRef AssertionName, IOPMAssertionID *AssertionID)`，注释 *"No special privileges are necessary to make this call - any process may activate a power assertion."*<br>[官方] `kIOPMAssertPreventUserIdleDisplaySleep`（`CFSTR("PreventUserIdleDisplaySleep")`）"Prevents the display from dimming automatically"；**`kIOPMAssertionTypeNoDisplaySleep` 已 deprecated in 10.7**，注释"Please use assertion type `kIOPMAssertPreventUserIdleDisplaySleep` instead"<br>[官方] 远程场景专用：`IOPMAssertionDeclareUserActivity(CFStringRef AssertionName, IOPMUserActiveType userType, IOPMAssertionID *AssertionID)`，`IOPMUserActiveRemote` = "Remote User connected to the system"；`IOPMDeclareNetworkClientActivity`；`kIOPMAssertNetworkClientActive`<br>（均在 `.../IOKit.framework/Versions/A/Headers/pwr_mgt/IOPMLib.h`） |
| 落地代价 | 极低：创建 + `IOPMAssertionRelease` |
| 权限/签名 | 无 |
| 备注 | 规格若写 `kIOPMAssertionTypeNoDisplaySleep` 应改为 `kIOPMAssertPreventUserIdleDisplaySleep`；远程会话场景更合适的是 `IOPMAssertionDeclareUserActivity(..., kIOPMUserActiveRemote, ...)`（带超时，需定期续期） |

---

## 9. 权限模型（TCC 持久性 / SMAppService / SMJobBless）

| 项 | 内容 |
|---|---|
| 结论 | **有条件可行**：TCC 授权绑定"代码身份"（designated requirement），必须用稳定证书签名 |
| 依据（TCC 持久性） | [官方] TN3127 *Inside Code Signing: Requirements*：*"macOS solves this problem by recording your app's DR in its database of apps authorized to access the microphone. Each time your app tries to access the microphone, macOS checks that this version of the app satisfies the original DR."*；*"**Unsigned code has no DR. Ad hoc signed code, called Sign to Run Locally by Xcode, has a DR but it's tied to that specific version of the code. In both cases macOS can't reliably track the identity of the code.**"*；Developer ID 的 DR 形如 `anchor apple generic and identifier "…" and certificate leaf[...] … Team ID`（https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements ）<br>[源码/实证] trycua/cua `libs/cua-driver/scripts/README.md`：*"macOS Accessibility and Screen Recording grants are tied to an app's designated requirement. An ad-hoc signature uses a `cdhash` requirement that changes on every rebuild, so its grants do not survive the next local install."*；Rectangle README 同现象："an ad-hoc signed build needs its permission granted again after rebuilding" |
| 依据（检测） | [官方] `AXIsProcessTrustedWithOptions(CFDictionaryRef)` + `kAXTrustedCheckOptionPrompt`（SDK `AXUIElement.h`；文档 https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions ） |
| 依据（SMAppService） | [官方] `SMAppService`，macOS 13.0+："use `SMAppService` to register and control `LoginItems`, `LaunchAgents`, and `LaunchDaemons`"；`register()` / `unregister()`；`class var mainApp`；`loginItem(identifier:)`（plist 需在 `Contents/Library/LoginItems`）；`agent(plistName:)`（`Contents/Library/LaunchAgents`）；`daemon(plistName:)`（`Contents/Library/LaunchDaemons`）；`SMAppService.Status` = `notRegistered` / `enabled` / `requiresApproval`（https://developer.apple.com/documentation/servicemanagement/smappservice 、.../register() 、.../status-swift.enum 、.../daemon(plistname:) ） |
| 依据（SMJobBless） | [官方] `SMJobBless` 可用区间标注为 **`macOS: 10.6.0 - 13.0.0`**，即 macOS 13 起为 deprecated（https://developer.apple.com/documentation/servicemanagement/smjobbless(_:_:_:_:) ）；仍要求 `SMPrivilegedExecutables` / `SMAuthorizedClients`、helper 放 `Contents/Library/LaunchServices` |
| 落地代价 | 中：首次运行引导用户去"系统设置 → 隐私与安全性 → 辅助功能"勾选；`SMAppService` 需 `requiresApproval` 状态 UI |
| 权限/签名 | Developer ID 证书（否则每次更新丢授权）；MAS 沙箱与 AX 的兼容性不确定（见 #1） |
| 备注 | 只要 bundle id / Team ID 稳定，**重装/更新后 TCC 授权可保留**；改 bundle id 或换签名证书 = 全新身份 = 必须重新授权。**本产品不需要 root helper**，无需 `SMJobBless` |

---

## 10. 窗口状态重建

| 项 | 内容 |
|---|---|
| 结论 | **有条件可行**；"最大化"在 macOS 上要用 zoom 按钮动作，不是 AX 属性 |
| 依据（最小化） | [官方] SDK `AXAttributeConstants.h`：`kAXMinimizedAttribute` = `"AXMinimized"`，*"Whether a window is currently minimized to the dock… Writable? Yes."*<br>[源码] yabai `window_is_minimized()` 用 `AXUIElementCopyAttributeValue(..., kAXMinimizedAttribute, ...)`（`src/window.c`）；AeroSpace `setNativeMinimized` → `window.set(Ax.minimizedAttr, value)`（`src/AppBundle/tree/MacApp.swift`） |
| 依据（全屏/zoom） | [官方] `kAXFullScreenButtonAttribute`（`"AXFullScreenButton"`）是**按钮元素**，"Writable? No."；SDK 中**没有** `AXZoomed`<br>[源码] xa11y 平台差异文档：*"macOS reads minimized and fullscreen (`AXMinimized` / `AXFullScreen`) on window-like elements; **there is no AX attribute for the zoom state** (`AXZoomed` is not declared in the SDK, and live windows answer `kAXErrorAttributeUnsupported` for it)"*；"Maximize is a separate operation and is not supported on macOS"（https://xa11y.dev/reference/platform-details ） |
| 依据（尺寸不精确 / 设两次） | [源码] yabai `window_manager_animate_window` 的 else 分支注释：*"Due to macOS constraints (visible screen-area), we might need to resize the window **before** moving it"* + *"**after** moving it"* → 实际调用顺序 `setSize → setPosition → setSize`（`asmvik/yabai/src/window_manager.c`）<br>[源码] AeroSpace `setFrame()` 注释：*"Set size and then the position. The order is important https://github.com/nikitabobko/AeroSpace/issues/143 / #335"*，实现同样 size→position→size（`Sources/AppBundle/tree/MacApp.swift`）<br>[源码] 两者都还加了"关动画"的未文档化 workaround：临时把 `kAXEnhancedUserInterface` 置 false（yabai `AX_ENHANCED_UI_WORKAROUND`，`src/misc/helpers.h`；AeroSpace `disableAnimations()`，注释 "Some undocumented magic"） |
| 落地代价 | 中：需要 settle 等待（回读 `kAXPositionAttribute`/`kAXSizeAttribute` 直到稳定），不能假定 set 后立即生效 |
| 权限/签名 | Accessibility |
| 备注 | 恢复策略应"幂等 + 可重试"：set 后回读校验，最多重试 2 次；不要把 `AXEnhancedUserInterface` 切换写进常规路径（副作用：会短暂改变目标 App 的 UI 响应行为） |

---

## 11. 更新与分发

| 项 | 内容 |
|---|---|
| 结论 | **可行**（Developer ID + 公证 + Tauri updater）；Universal binary 支持 |
| 依据（updater） | [官方] Tauri v2 Updater 文档：macOS 产物为 `myapp.app.tar.gz` + `.sig`；*"Tauri's updater needs a signature to verify that the update is from a trusted source. This cannot be disabled."*（minisign 公钥在 `tauri.conf.json`）（https://v2.tauri.app/plugin/updater/ ）<br>[源码] `tauri-apps/plugins-workspace/v2/plugins/updater/src/updater.rs`，`#[cfg(target_os = "macos")] impl Update::install_inner`：用 `GzDecoder` + `tar::Archive` 解包，`swap_bundle()` 单步替换 `.app`，失败回退两次 rename；`PermissionDenied` 时通过 AppleScript `do shell script … with administrator privileges` 提权；文档注释："**macOS / Linux:** You need to relaunch the app to run the newly install version" |
| 依据（公证） | [官方] 需 Developer ID 证书 + Hardened Runtime + secure timestamp；`altool` 自 2023-11-01 起被 `notarytool` 取代（https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution ）；Tauri 侧环境变量 `APPLE_CERTIFICATE`/`APPLE_ID`/`APPLE_API_KEY` 等，`--skip-stapling` 可跳过 stapling（https://v2.tauri.app/distribute/sign/macos/ ） |
| 依据（Universal） | [官方] Tauri CLI `-t/--target`：*"It must be one of the values outputted by `$rustc --print target-list` **or `universal-apple-darwin`** for an universal macOS application. Note that compiling an universal macOS application requires both `aarch64-apple-darwin` and `x86_64-apple-darwin` targets to be installed."*（https://v2.tauri.app/reference/cli/ ）<br>[官方] Apple *Building a universal macOS binary*（https://developer.apple.com/documentation/apple-silicon/building-a-universal-macos-binary ） |
| 无签名应用能否被 updater 更新 | **技术上能**：updater 只校验 minisign 签名（`verify()` → `install_inner()`），不校验 Apple 代码签名；解包后直接 `swap_bundle()` 替换 `.app`。但 Apple silicon 上可执行文件需要**至少 ad-hoc 签名**才能运行（Tauri 文档"Ad-Hoc Signing"一节即为此：*"This is useful on ARM (Apple silicon) devices, where code-signing is required for all apps from the Internet"*，https://v2.tauri.app/distribute/sign/macos/ ）；且未公证的包会被 Gatekeeper 拦截。**建议**：一律 Developer ID + 公证 |
| 落地代价 | 中：CI 需 macOS runner + 证书 + notary 凭据；Universal 构建时间约翻倍 |
| 权限/签名 | Developer ID Application 证书 + 公证；更新包必须重新签名+公证（否则下次启动 Gatekeeper 会拦） |
| 备注 | Tauri updater 替换的是 `.app` 本体 → **TCC 授权是否保留取决于新包的 DR 是否仍满足旧 DR**（同 bundle id + 同 Team ID 即可，见 #9）。若某次发版换了证书，用户需重新授权 |

---

## 风险最高的三条

1. **第 7 条被误当成可实现的"被捕获检测"**。macOS 没有任何公开 API 能回答"哪块屏/哪个进程正在被捕获"。`SCShareableContent` 只列"可捕获对象"，`SCWindow.isActive` 不是第三方录制反查，`SCDisplay` 没有 `isCaptured`。若规格按"自动识别被录制屏"写，整条产品逻辑会落空。**必须先改成用户指定目标屏 + 启发式触发（进程/端口/会话状态），并把误判作为常态处理。**
2. **第 6 条 CGVirtualDisplay 私有 API**。它真实存在（Chromium 在用），但：App Store 2.5.1 明文只许公开 API（必拒）；Chromium 自己的注释就写着 macOS 14 起要求 serial 唯一、vendorID 非零，还要 removal workaround——说明每个大版本都可能破。公证文档未把"仅公开 API"列为条件（公证≠App Review，只是自动扫描），但**这不等于 Apple 保证私有 API 能过公证**。**建议直接放弃，且本产品不需要虚拟屏。**
3. **第 9 条 TCC 与签名绑定**。ad-hoc 签名的 DR 绑定单次 cdhash，任何重建/更新都会让"辅助功能"授权静默失效（TN3127 明文 + Rectangle/cua 的实证）。若早期用 ad-hoc 开发、后期换 Developer ID，用户会遭遇"每次更新都要重新授权"。**必须第一天就用固定 Developer ID + 固定 bundle id 签名，并把"授权丢失"做成可自检 + 一键引导修复的路径。**

---

## 附：规格中必须删除/改写的 API 名

| 规格写法 | 事实 | 正确写法 |
|---|---|---|
| `kAXWindowNumberAttribute` | 不存在于公开 SDK（10.15/11.3 头文件、accessibility-sys 均无） | 私有 `_AXUIElementGetWindow(AXUIElementRef, uint32_t*)`，或用 `CGWindowListCopyWindowInfo` + pid/bounds 匹配 |
| `kCGDisplaySerialNumber` / `kCGDisplayVendorNumber` / `kCGDisplayModelNumber` | 是函数，不是字典键 | `CGDisplaySerialNumber(id)` / `CGDisplayVendorNumber(id)` / `CGDisplayModelNumber(id)`；IOKit 键为 `kDisplaySerialNumber`/`kDisplayVendorID`/`kDisplayProductID` |
| `CGSSessionScreenIsLocked` | 未文档化，不在 `CGSession.h` 键列表 | 只用 `kCGSessionOnConsoleKey` / `kCGSessionLoginDoneKey`；锁屏另用通知或 `loginwindow` 窗口判断（不确定） |
| `kAXFullscreenAttribute` | 不在公开 SDK | 硬编码 `CFSTR("AXFullScreen")`，并按"未公开、可能变"处理 |
| `kIOPMAssertionTypeNoDisplaySleep` | 10.7 起 deprecated | `kIOPMAssertPreventUserIdleDisplaySleep`，或 `IOPMAssertionDeclareUserActivity(..., kIOPMUserActiveRemote, ...)` |
| "查询正在录屏的显示器/进程" | 无此 API | 删除该需求，改启发式 |
