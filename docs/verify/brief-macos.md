# 验证任务：macOS 平台技术事实核查

你是独立事实核查员。产品背景（仅作上下文）：

Unfy 是一个跨平台桌面工具，检测到本机被远程访问时，把副屏上的窗口迁移到被捕获的那块屏，断开后恢复。技术栈 Tauri 2 + Rust。你的任务**只做 macOS 部分的事实核查**，产出带证据的结论，供我们否决规格文档里编造的 API。

## 输出

写一个文件：`docs/verify/macos.md`。简体中文，表格优先，每条结论必须给出：

- 结论（可行 / 有条件可行 / 不可行 / 不确定）
- 依据（官方文档 URL 或真实开源项目文件路径 + 具体符号名）
- 落地代价
- 是否需要特殊授权（Accessibility / Screen Recording / 签名 / 公证 / 私有 API）
- 无法确认的，写 `不确定`，并给出验证方法

## 必须核查的条目

1. **窗口枚举与控制**：`AXUIElementCreateApplication` + `kAXWindowsAttribute`、`AXUIElementSetAttributeValue(kAXPositionAttribute/kAXSizeAttribute, AXValueCreate)`。确认：需要 Accessibility（TCC）授权；能操作哪些应用的窗口、哪些拒绝（有无公开证据）；沙箱化/公证应用能否申请该权限。
2. **窗口标识稳定性**：`kAXWindowNumberAttribute`(CGWindowID)、`CGWindowListCopyWindowInfo` 的 `kCGWindowNumber`/`kCGWindowOwnerPID`/`kCGWindowLayer`/`kCGWindowBounds`。给出跨重启的窗口标识方案（注意 CGWindowID 是进程内递增的）。
3. **显示器身份**：`CGGetActiveDisplayList`、`CGDisplayCreateUUIDFromDisplayID`、`kCGDisplaySerialNumber`/`kCGDisplayVendorNumber`/`kCGDisplayModelNumber`。确认哪个可跨重启稳定；`CGDisplayIsMain` 语义；`NSScreen.deviceDescription[kNSScreenNumber]`。
4. **显示器配置变更通知**：`CGDisplayRegisterReconfigurationCallback`、`NSApplication.didChangeScreenParametersNotification`。确认回调在专用 run loop 线程上的要求（Tauri 里的坑）。
5. **原生全屏 Space 与窗口迁移**：macOS 原生 fullscreen 窗口在独立 Space，`AXUIElementSetAttributeValue(kAXPositionAttribute)` 对其是否无效（要有证据）；`CGWindowListCopyWindowInfo` 能否看到其他 Space 的窗口；"Displays have separate Spaces" 打开/关闭时的差异。
6. **虚拟显示器**：`CGVirtualDisplay`（私有 API，`CGVirtualDisplayDescriptor`/`CGVirtualDisplaySettings`/`CGVirtualDisplayMode`）。确认：真实存在的头文件/项目；能否被远程软件枚举捕获；能否走公证（notarization 允许私有 API 吗？App Store 呢？）；风险（系统更新失效概率）。
7. **远程会话/远程控制检测**：`CGSessionCopyCurrentDictionary` 的 `CGSSessionScreenIsLocked`/`kCGSSessionOnConsoleKey`；ScreenCaptureKit `SCShareableContent` 能否看到正在录制/被捕获（能否反查"谁在录屏"——注意这是关键：macOS 是否有 API 能查询当前正在被屏幕捕获的显示器/窗口？查 ScreenCaptureKit 与 TCC 相关 API）；`kCGWindowName` 需要 Screen Recording 权限吗。
8. **防止屏幕休眠 / 电源断言**：`IOPMAssertionCreateWithName`（`kIOPMAssertionTypeNoDisplaySleep`）。
9. **权限模型**：TCC 授权持久性（重装、改 bundle id、ad-hoc 签名后失效）；`AXIsProcessTrustedWithOptions`；`SMAppService`（macOS 13+）注册登录项与 daemon 的 API 名称；特权 helper 的 `SMJobBless` 是否被废弃。
10. **窗口状态重建**：最大化（zoom button / `kAXFullscreenAttribute`）、最小化 `kAXMinimizedAttribute`、窗口动画导致 `AXUIElementSetAttributeValue` 尺寸不精确（有无公开 issue 证据，常见 workaround 是设置两次）。
11. **更新与分发**：Tauri updater 在 macOS 的公证要求；Universal binary 支持；无签名应用是否可以被 Tauri updater 更新。

## 事实纪律

- 只写你有证据的内容。禁止凭印象编造 API 名。所有 API 名必须在官方文档或真实代码中出现。
- 优先 developer.apple.com 官方文档；其次真实开源仓库源码。
- 每条给出的 URL 必须是你实际读过的页面（可以读取后摘录关键句）。
