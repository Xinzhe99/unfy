# 独立事实核查（spec 的对抗性复核）

这三份文件不是产品文档，是**用来否决规格里编造 API 的证据库**。规格由 `gpt-6-astra` 在 2026-10-21 生成，随后由三个独立核查 agent 分别对 Windows / macOS / Linux+检测 做逐条事实核对。规格中凡是与本文冲突的，以本文为准。

| 文件 | 覆盖 | 结论密度 |
|---|---|---|
| [windows.md](windows.md) | 窗口枚举/跨进程操作、DPI 与坐标、显示器身份、拓扑通知、禁屏、IDD 签名、远程会话检测、服务/helper、自启、窗口状态、应用自我纠正（11 项） | 6 项有条件可行，1 项对第三方工具**不可行**，4 项可行 |
| [macos.md](macos.md) | AX 控制、窗口标识、显示器身份、拓扑回调、原生全屏 Space、CGVirtualDisplay、被捕获检测、防休眠、TCC、状态重建、更新分发（11 项） | 2 项不可行/必拒，5 项有条件可行，4 项可行 |
| [linux-detection.md](linux-detection.md) | X11 EWMH、多屏几何、Wayland、evdi、Linux 会话检测、自启、打包、**逐工具端口/日志**、反查捕获者、虚拟输入、回弹（11 项） | Wayland 通用方案**不可行**；三平台均**无法**反查谁在捕获 |

## 三条会改变产品定义的结论

1. **没有任何公开 API 能查询"谁在远程/正在捕获哪块屏"**（三平台一致）。`SM_REMOTESESSION` 只对 RDP/shadow 有效，TeamViewer/AnyDesk/RustDesk/Parsec/Sunshine/ToDesk/向日葵 **都不置位**。因此"自动检测当前主显示器并搬运"必须重新表述为：**用户指定目标屏 + 可验证的入站信号 + 热键纠正**，且默认目标 = 主屏只是一种约定，不是"检测到远程屏"。
2. **macOS 的虚拟显示器是私有 API**，App Review 2.5.1 必拒，公证不保证长期可用；**Windows 的 IDD 必须微软签名**（EV + attestation，且 ARM64/Server 实际要 WHQL）；**Linux Wayland 无通用跨窗口控制**。三者的虚拟屏都无法形成对称承诺 → 只能是 M4 独立组件，不能进 v1。
3. **恢复正确性比迁移更难**：Chromium 会主动覆写外部给的窗口位置；`WINDOWPLACEMENT.rcNormalPosition` 是 workspace 坐标不能直接给 `SetWindowPos`；X11 reparenting WM 下 `GetGeometry` 返回父窗口坐标。这三条都会导致"看起来成功、实际错位"，必须写后回读 + 有界重试。

## 使用方式

- 实现阶段每个平台适配器开工前，先读对应文件的"必须删除/改写的 API 名"表。
- 规格里的 `[待验证]` 标记与本文的 `不确定` 是同一件事的两个视角；关闭一个标记必须同时更新两处。
- 本文只覆盖技术事实，不评价产品价值。产品取舍见 [../plan.md](../plan.md)。
