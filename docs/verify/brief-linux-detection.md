# 验证任务：Linux 与检测子系统技术事实核查

你是独立事实核查员。产品背景（仅作上下文）：

Unfy 是一个跨平台桌面工具，检测到本机被远程访问时，把副屏上的窗口迁移到被捕获的那块屏，断开后恢复。技术栈 Tauri 2 + Rust。你的任务**只做 Linux 与"远程检测"部分的事实核查**，产出带证据的结论，供我们否决规格文档里编造的 API。

## 输出

写一个文件：`docs/verify/linux-detection.md`。简体中文，表格优先，每条结论必须给出：

- 结论（可行 / 有条件可行 / 不可行 / 不确定）
- 依据（官方文档 URL 或真实开源项目文件路径 + 具体符号名）
- 落地代价
- 无法确认的，写 `不确定`，并给出验证方法

## Linux 部分

1. **X11 窗口枚举与控制**：EWMH `_NET_CLIENT_LIST`/`_NET_CLIENT_LIST_STACKING`、`_NET_FRAME_EXTENTS`、`_NET_WM_STATE`（`_NET_WM_STATE_MAXIMIZED_VERT/HORZ`、`_NET_WM_STATE_FULLSCREEN`、`_NET_WM_STATE_HIDDEN`）、`_NET_WM_DESKTOP`、`_NET_WORKAREA`、`_NET_CURRENT_DESKTOP`；`XMoveResizeWindow` 与 `XConfigureWindow` 的差异；需不需要 WM 配合。确认：在 GNOME/KDE/XFCE 下用 Xlib/XCB 直接操作他人窗口是否可行，以及 ICCCM 的 `configure` 重定向（reparenting WM）导致的坐标陷阱（相对 frame 还是 client）。
2. **多显示器几何**：xrandr 的 `XRRScreenResources`/`XRRGetMonitors`；`_NET_WORKAREA` 在多屏下是否可靠（已知很多 WM 不更新，需证据）；`XineramaQueryScreens` 现状。
3. **Wayland**：确认（a）没有通用 API 允许一个应用移动另一个应用的窗口；（b）KWin scripting 与 GNOME Shell extension 作为 adapter 的可行性、维护成本、GNOME 45+ 扩展 API 变动；（c）`wlr-foreign-toplevel-management` 协议能否移动窗口（我怀疑只能控制状态，不能移动）；（d）截图/捕获协议对窗口几何的可见性。
4. **evdi / 虚拟显示器**：确认 evdi 是内核模块、需要 DKMS 与 root、在 Wayland（需要 wlroots 支持或 DisplayLink）下的限制；能否被 RustDesk/VNC 捕获。
5. **远程会话检测（Linux）**：X11 下 `X11/Xlib` 的 `XScreenSaverQueryExtension`/`XScreenSaverQueryInfo` 能否判断"正在被远程控制"；`loginctl`/`systemd-logind` 的 `Session.Type`(`x11`/`wayland`/`tty`)、`Remote` 属性（`org.freedesktop.login1.Session` 的 `Remote` 布尔值）——确认这个属性的语义（是否只对 systemd 登录会话有效）；xrdp 会话的 `Xvfb`/`Xorg` 显示号特征。
6. **开机自启**：XDG autostart `.desktop` 规范（`~/.config/autostart`）、systemd user units 的差异。
7. **打包与更新**：AppImage 的自更新限制、deb/rpm 的签名与 apt/dnf 仓库、Flatpak 沙箱对窗口控制的影响（确认 Flatpak 能否操作外部窗口：X11 下 `--socket=x11` 可以，Wayland 下不行）。

## 检测子系统部分（跨平台）

8. **各远程工具的可检测信号**：TeamViewer / AnyDesk / RustDesk / Parsec / Sunshine / NoMachine / ToDesk / 向日葵 / Windows RDP / macOS Screen Sharing / VNC(RealVNC/TigerVNC)。对每个工具给出：进程名（Windows/macOS/Linux）、可选的日志路径、是否有本地监听端口（默认端口号）、是否有官方文档说明端口、能否从日志判断"会话开始/结束"。
   - **必须真实**：端口号要有官方文档依据；日志路径不确定就写不确定。
   - 特别核查：RustDesk 的默认端口（21115-21119）、AnyDesk（7070）、TeamViewer（5938）、Parsec（8000-8040? 需核实）、Sunshine（47984-48010）、NoMachine（4000）、VNC（5900+）。
9. **ScreenCaptureKit / Windows Graphics Capture / PipeWire 端口**：能否反查"当前哪些进程正在捕获屏幕"。macOS 上 `SCShareableContent` 是否有 API 返回正在被捕获的内容；Windows 上 `GraphicsCaptureItem`/`IsBorderRequired` 无此能力；Linux PipeWire 的 screencast portal 是否有 `Session`/`Stream` 属性暴露（`org.freedesktop.portal.ScreenCast` 的 `AvailableSourceTypes`/`AvailableCursorModes` 之外有没有"当前活跃会话"查询）。结论很可能是"不能"，请确认并给出替代方案（进程/端口/日志）。
10. **虚拟输入设备检测**：Windows 上能否区分"远程注入的鼠标输入"与真实鼠标（`GetMessageExtraInfo` 的 `MOUSEEVENTF_...`/`0xFF515700` 签名？需要核实）；Linux evdev 上能否区分 uinput 虚拟设备；macOS `CGEventSource` 的 `kCGEventSourceStateHIDSystemState` 能否区分。
11. **窗口迁移后应用自我纠正**：有没有公开证据表明 Electron 应用会在外部移动窗口后回弹。

## 事实纪律

- 只写你有证据的内容。禁止凭印象编造 API 名、端口号、日志路径。所有内容必须在官方文档或真实代码中出现。
- 每条给出的 URL 必须是你实际读过的页面（可以读取后摘录关键句）。
- 不确定的端口号/路径一律写 `不确定` + 验证方法，不要猜。
