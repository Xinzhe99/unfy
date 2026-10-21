# Linux 窗口控制与远程检测：事实核查

> 核查范围：brief 的 11 项。每条给 结论 / 依据 / 落地代价 / 备注。
> 依据只写实际读过的页面或真实仓库文件 + 符号名。凡是没读到官方依据的，写 `不确定` + 验证方法。
> 核查日期 2026-10-21。

## 0. 速览

| # | 主题 | 结论 |
|---|---|---|
| 1 | X11 窗口枚举与控制 | 有条件可行（必须走 EWMH client message / ConfigureRequest，WM 可拒绝） |
| 2 | 多显示器几何 | 可行（RandR 1.5 Monitors）；`_NET_WORKAREA` 单屏可靠、多屏不可靠 |
| 3 | Wayland | 不可行（通用移动 API 不存在）；KWin/GNOME 适配器 有条件可行 |
| 4 | evdi / 虚拟显示器 | 有条件可行（内核模块 + root）；Wayland 下 `不确定` |
| 5 | Linux 远程会话检测 | 不可行（无"正在被控制"信号）；只能进程/端口/会话枚举 |
| 6 | 开机自启 | 可行（XDG autostart 与 systemd user unit 二选一，语义不同） |
| 7 | 打包与更新 | 有条件可行（只有 AppImage 能自更新；Flatpak 在 Wayland 下不可用） |
| 8 | 各工具可检测信号 | 见 §8 表；会话级检测只有 TeamViewer/AnyDesk 有官方日志依据 |
| 9 | 反查"谁在截屏" | 不可行（三平台均无 API）；替代信号见 §9.3 |
| 10 | 虚拟输入检测 | 有条件可行（能判"注入/虚拟设备"，不能判"远程"） |
| 11 | 应用自我纠正（回弹） | 不确定（无公开证据支持"移动后回弹"，有"启动时恢复"证据） |

---

## 1. X11 窗口枚举与控制

| 项 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| 枚举窗口 | 可行 | EWMH §3.2 `_NET_CLIENT_LIST` / `_NET_CLIENT_LIST_STACKING`：两者都是 WM 管理的窗口数组，"SHOULD be set and updated by the Window Manager"。URL: https://specifications.freedesktop.org/wm/latest/ar01s03.html | 低。`x11rb`/`xcb` 读 root 属性即可；但 spec 是 SHOULD，缺失时需回退到 `QueryTree` 递归 |
| 边框尺寸 | 可行 | EWMH §5.17 `_NET_FRAME_EXTENTS, left, right, top, bottom`："The Window Manager MUST set ... to the extents of the window's frame"。URL 同上 ar01s05.html | 低。用于把 client 坐标换算成 frame 坐标 |
| 状态位 | 可行（只读可靠） | EWMH §5.7 `_NET_WM_STATE` 原子表含 `MAXIMIZED_VERT/HORZ`、`FULLSCREEN`、`HIDDEN`、`FOCUSED`；"The Window Manager MUST keep this property updated"。同一节明确：`_NET_WM_STATE_HIDDEN` 是 WM 的函数，"if an Application asks to toggle ... the Window Manager should probably just ignore the request" | 低。改状态必须发 `_NET_WM_STATE` client message 到 root，不能直接写属性 |
| 桌面归属 | 可行 | EWMH §5.5 `_NET_WM_DESKTOP`：改桌面需发 client message 到 root；"The Window Manager MUST keep this property updated on all windows" | 低 |
| 工作区/当前桌面 | 可行 | EWMH §3.9 `_NET_WORKAREA`（"MUST be set by the Window Manager upon calculating the work area for each desktop"）、§3.6 `_NET_CURRENT_DESKTOP`（"MUST be set and updated"） | 低 |
| 移动他人窗口 | 有条件可行 | `XConfigureWindow`/`XMoveResizeWindow` man page：若窗口 override-redirect 为 False 且他人已在父窗口选 `SubstructureRedirectMask`，服务器**只产生 ConfigureRequest，"no further processing is performed"** —— 即 WM 可以不执行。URL: https://www.x.org/releases/X11R7.6/doc/man/man3/XConfigureWindow.3.xhtml | 中。必须改用 EWMH §4.2 `_NET_MOVERESIZE_WINDOW`（"Pagers wanting to move or resize a window may send ... instead of using a ConfigureRequest"），且带 `StaticGravity` 才能精确含装饰定位 |
| `XMoveResizeWindow` vs `XConfigureWindow` | 前者是后者掩码的语法糖 | man page：`XMoveResizeWindow` "changes the size and location ... without raising it"；`XConfigureWindow` 用 `XWindowChanges` 可同时改 x/y/width/height/border_width/sibling/stack_mode | 无差异（内部同一请求），差别只在能否一次改 stacking |
| WM 配合 | 必需 | ICCCM "Redirection of Operations"：可被重定向的请求是 `MapWindow`、`ConfigureWindow`、`CirculateWindow`；"a client cannot assume that any redirectable request is actually performed" | — |
| 坐标陷阱（reparenting） | 真实存在 | ICCCM：`ConfigureWindow` 的 x/y 在**新父窗口**坐标系解释，"Clients should use the root coordinate space for these requests"；`GetGeometry` 返回的是父窗口坐标，"not directly useful after a reparent operation"；合成 `ConfigureNotify` 用 root 坐标，真实 `ConfigureNotify` 用父窗口坐标（"coordinates in real ConfigureNotify events are in the parent's space; in synthetic events, they are in the root space"） | 中。读几何必须用 `TranslateCoordinates` 或 `_NET_FRAME_EXTENTS` + synthetic 事件，不能直接信 `GetGeometry` |
| GNOME/KDE/XFCE 下直接操作 | 有条件可行（同上路径） | 三家都是 EWMH 兼容 reparenting WM；只要走 `_NET_MOVERESIZE_WINDOW` + 监听 `ConfigureNotify` 即可。ICCCM 还规定："Clients must not respond to being moved by attempting to move themselves to a better location"（对我们有利） | 中 |

**备注**：`_NET_WM_STATE_MAXIMIZED_*` 与 `_NET_WM_STATE_FULLSCREEN` 在迁移场景要**先清除**再移动，否则 WM 会把新坐标重新钳回原显示器（最大化几何由 WM 持有，不在 `_NET_FRAME_EXTENTS` 里）。这点是 [INFERENCE]，需在 GNOME/KDE 实机验证。

---

## 2. 多显示器几何

| 项 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| `XRRScreenResources` | 可行 | randrproto §RRGetScreenResources："returns the list of outputs and crtcs connected to the screen"；若只要当前配置用 `RRGetScreenResourcesCurrent`。URL: https://gitlab.freedesktop.org/xorg/proto/xorgproto/-/raw/master/randrproto.txt | 低 |
| `XRRGetMonitors` | 可行（首选） | randrproto 1.5 引入 Monitor："a rectangular subset of the screen which represents a coherent collection of pixels presented to the user"；`RRGetMonitors(window, get_active)` | 低。libXrandr 需 ≥1.5 |
| `_NET_WORKAREA` 多屏可靠性 | 不可靠 | spec 本身只定义**每个 desktop 一个**矩形（§3.9 "Contains a geometry for each desktop"），没有 per-monitor 概念；社区提案 `_NET_WORKAREAS_Dn` 的存在即证据：https://mail.gnome.org/archives/wm-spec-list/2018-December/msg00000.html（"existing properties can not be used to set struts on edges between two monitors"）。GTK 后来才加 `_NET_WORKAREAS_Dn` 支持（gtk wip/muktupavels/work-areas 提交） | 低。**不要**用 `_NET_WORKAREA` 做 per-monitor 决策，改用 `RRGetMonitors` + `_NET_WM_STRUT_PARTIAL` |
| `XineramaQueryScreens` | 已被取代 | randrproto §1.5.1："Xinerama's information now comes from the Monitors instead of directly from the CRTCs"；Monitor 标记为 Primary 的排第一 | 低。仅在无 RandR 1.5 的老环境回退 |

---

## 3. Wayland（重点）

### 3.1 结论表

| 子问题 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| (a) 通用 API 移动他人窗口 | **不可行** | xdg-shell 里 `xdg_toplevel.move`/`resize` 的定义是 "Start an interactive, **user-driven** move of the surface"，只能作用于**自己的** surface 且需用户动作；`set_maximized`/`set_fullscreen`/`set_minimized` 同样只针对自身。URL: https://gitlab.freedesktop.org/wayland/wayland-protocols/-/raw/main/stable/xdg-shell/xdg-shell.xml | 无（协议层不存在） |
| (a') 甚至连"读全局坐标"都不行 | 不可行 | Electron 官方文档（Linux/Wayland 小节）："it is generally not possible to programmatically resize windows after creation, or to position, move, focus, or blur windows without user input"；`win.getBounds()` "On Wayland, this method will return `{ x: 0, y: 0, ... }` as introspecting or programmatically changing the global window coordinates is prohibited"。URL: https://www.electronjs.org/docs/latest/api/browser-window | — |
| (c) `wlr-foreign-toplevel-management` 能否移动 | **不能**（只能改状态） | 协议 XML 的 request 全集：`set_maximized` / `unset_maximized` / `set_minimized` / `unset_minimized` / `activate` / `close` / `set_rectangle` / `set_fullscreen` / `unset_fullscreen`。**没有** set_position。`set_rectangle` 是"the app using this protocol"上报自己 surface 的位置提示（surface-local 坐标），不是"把它移到哪"。URL: https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/raw/master/unstable/wlr-foreign-toplevel-management-unstable-v1.xml | — |
| (d) 捕获协议能否看到窗口几何 | 能（但只对捕获方） | `org.freedesktop.portal.ScreenCast` 的 `Start()` 返回 stream 属性含 `position (ii)`（"in the compositor coordinate space"）、`size (ii)`、`source_type`、`pipewire-serial`。URL: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html。Mutter 侧 `org.gnome.Mutter.ScreenCast.Stream.Parameters` 也暴露 `position`/`size`/`output-name`（data/dbus-interfaces/org.gnome.Mutter.ScreenCast.xml） | 低，但仅用于"我知道被捕获区域在哪"，不能反推谁在捕获（见 §9） |

### 3.2 适配器可行性

| 适配器 | 结论 | 依据（符号级） | 维护成本 |
|---|---|---|---|
| KWin scripting | 有条件可行 | `src/window.h`：`Q_PROPERTY(KWin::RectF frameGeometry READ frameGeometry WRITE moveResize NOTIFY frameGeometryChanged)`，即 **frameGeometry 可写**，写即调 `moveResize(const RectF&)`（同文件 `void moveResize(const RectF &geometry);`）。Workspace 侧有 `sendClientToScreen(KWin::Window *client, KWin::Output *output)` 与 `QList<KWin::Window*> stackingOrder`。URL: https://develop.kde.org/docs/plasma/kwin/api/ 、https://invent.kde.org/plasma/kwin/-/raw/master/src/window.h | 中。注入方式：D-Bus 接口 `org.kde.kwin.Scripting`，对象 `/Scripting`，方法 `loadScript(filePath, pluginName)` + `start()`（src/scripting/scripting.h: `Q_CLASSINFO("D-Bus Interface", "org.kde.kwin.Scripting")`、`Q_SCRIPTABLE Q_INVOKABLE int loadScript(...)`、`Q_SCRIPTABLE void start()`；scripting.cpp: `registerObject("/Scripting", ...)`）。脚本随 KWin 版本走，API 有 since 标注（如 `isEffectActive` 标 6.0），需按 Plasma 大版本回归 |
| GNOME Shell extension | 有条件可行 | `Meta.Window.move_frame`：`META_EXPORT void meta_window_move_frame(MetaWindow*, gboolean user_op, int root_x_nw, int root_y_nw)`（src/meta/window.h:192；实现 src/core/window.c:4614，内部组 `META_MOVE_RESIZE_MOVE_ACTION | META_MOVE_RESIZE_CONSTRAIN`）。另有 `meta_window_move_to_monitor(MetaWindow*, int monitor)`（window.h:198）。文档：https://mutter.gnome.org/meta/method.Window.move_frame.html | 中高。需打包成扩展、用户手动安装/启用，且随 Shell 版本重写 |

**GNOME 45+ API 变动证据（维护成本不是猜测）**：

- GNOME 45：强制 ESM，`imports.*` 全部失效；`metadata.json` 的 `shell-version` 只能写 45（"Your extension modules won't be compatible with older GNOME Shell versions"）。URL: https://gjs.guide/extensions/upgrading/gnome-shell-45.html
- GNOME 46：`ExtensionState` 枚举值改名（`ENABLED`→`ACTIVE`、`DISABLING`→`DEACTIVATING` 等）、`Clutter.Container` 移除、`Meta.Barrier.display` 弃用。URL: https://gjs.guide/extensions/upgrading/gnome-shell-46.html
- GNOME 48：`Meta.get_window_actors` 等一批 `Meta` 函数被移到 `Meta.Compositor.*`（`Meta.get_window_actors` → `Meta.Compositor.get_window_actors`），旧名直接删除。URL: https://gjs.guide/extensions/upgrading/gnome-shell-48.html

→ 一年一次大版本、每次都有破坏性改动；`Meta.Window` 的方法虽然稳定（`move_frame` 自 3.x 存在），但**枚举窗口的入口**（`get_window_actors`）刚在 48 被搬走。

### 3.3 备注
Wayland 下唯一"不靠 compositor 私有接口"的官方通道是 portal：`org.freedesktop.portal.RemoteDesktop` 提供输入注入（`NotifyPointerMotion`/`NotifyKeyboardKeycode`/`ConnectToEIS`）与 `ScreenCast` 组合，**但不提供窗口移动**。URL: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html。所以"把窗口搬到主屏"在 Wayland 上只能：KWin 脚本 / GNOME 扩展 / 让用户自己搬。

---

## 4. evdi / 虚拟显示器

| 项 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| 是否内核模块 | 是 | README："The Extensible Virtual Display Interface (EVDI) is a Linux® kernel module"。URL: https://raw.githubusercontent.com/DisplayLink/evdi/devel/README.md | — |
| 需要 DKMS | 是 | `module/dkms.conf`：`PACKAGE_NAME="evdi"`、`AUTOINSTALL=yes`、`BUILT_MODULE_NAME[0]="evdi"`、`DEST_MODULE_LOCATION[0]="/kernel/drivers/gpu/drm/evdi"`。URL: https://raw.githubusercontent.com/DisplayLink/evdi/master/module/dkms.conf | 中。每次内核升级需重新编译，失败即无虚拟屏 |
| 需要 root | 是 | 内核模块加载（`insmod`/`modprobe`）本身需 CAP_SYS_MODULE；README 只说"kernel module"，未给权限矩阵 → 权限细节标 `不确定`，验证方法：非 root 执行 `modprobe evdi` 观察 `EPERM` | 高（对 Tauri 应用的安装器/提权流程是硬约束） |
| X11 下表现 | 可行 | README："displays can be controlled by standard tools, eg. `xrandr` or display settings applets in graphical environments eg. Unity, Gnome or KDE" | 低 |
| Wayland 下限制 | `不确定` | 官方文档只提 xrandr/DE applet（X11 语境）。wlroots 系需 compositor 主动枚举新 connector；GNOME/KDE 的 Wayland 会话对动态 DRM connector 的处理未见官方说明。验证方法：在 GNOME Wayland 与 KDE Wayland 各跑一次 `modprobe evdi`，看 `kscreen-doctor -o` / `gnome-randr` 是否出现新输出；wlroots 侧用 `wlr-randr` | 高（若需支持 Wayland，等于自研 compositor 集成） |
| 能否被 RustDesk/VNC 捕获 | `不确定` | RustDesk 在 Linux 的捕获后端未在官方文档列明；VNC 服务器（TigerVNC/x11vnc）抓的是 X screen，evdi 在 X11 下会成为一个 screen 区域，理论上可见。未找到官方陈述。验证方法：装 evdi → 创建虚拟屏 → 用 x11vnc 抓 `:0` 看是否包含该区域；RustDesk 侧看其 `capture` 选项（Sunshine 官方有 `capture = wlr/kms/kwin/x11` 枚举，可作对照：https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html） | 中 |

**备注**：DisplayLink 官方把 evdi 定位为"不是完整 DisplayLink 驱动"（README 原文 "EVDI is not a complete driver for DisplayLink devices"），且 README 明确"communication between the EVDI kernel module and the wrapper libevdi library is not access-controlled or authenticated"——若我们依赖它，等于把未认证的内核通道引入攻击面。

---

## 5. 远程会话检测（Linux）

| 信号 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| `XScreenSaverQueryExtension` / `XScreenSaverQueryInfo` | **不能**判断"正在被远程控制" | 官方 man page：`XScreenSaverQueryInfo` 只返回 `state`（`Off`/`On`/`Disabled`）、`kind`（`Blanked`/`Internal`/`External`）、`til_or_since`、`idle`（"milliseconds since the last input was received from the user on any of the input devices"）。无任何"谁在输入"信息。URL: https://man.archlinux.org/man/Xss.3.en.txt | 低，但用途仅限"用户是否在忙" |
| `logind` `Session.Remote` | 语义是"会话是本地还是远程"，**不等于**"屏幕正被捕获" | 官方：`Remote` "encodes whether the session is local or remote"；`RemoteHost`/`RemoteUser` 仅在远程会话非空；`Type` ∈ `unspecified|tty|web|x11|mir|wayland`；`Class` ∈ `user|greeter|lock-screen`。URL: https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html | 低。只对 systemd-logind 注册的会话有效（SSH 登录是 `Type=tty` + `Remote=true`，会误报） |
| xrdp 会话显示号特征 | 可识别（有条件） | xrdp 的 sesman 默认 `x11_display_offset = 10`（sesman/config.c:263 `se->x11_display_offset = 10;`），即会话 X display 从 `:10` 起；`x_server_running_check_ports()` 检查 `/tmp/.X11-unix/X%d`、`/tmp/.X%d-lock`、`59%2.2d`/`60%2.2d`/`62%2.2d`（session.c:167-204）。xrdp 默认监听 TCP 3389（`xrdp.ini` man page："The default port for RDP is 3389"）。URL: https://raw.githubusercontent.com/neutrinolabs/xrdp/master/sesman/session.c 、https://manpages.ubuntu.com/manpages/noble/man5/xrdp.ini.5.html | 中。xrdp 会话使用独立 X server（代码注释里是 `Xorg/X11rdp/Xvnc`，session.c:424），**不是**物理桌面 —— 所以"把窗口搬到被捕获的屏"对 xrdp 语义不同，需单独分支 |
| Xvfb / Xorg 显示号 | `不确定` | 未找到"xrdp 必用 Xvfb"的官方依据；session.c 的注释只列 `Xorg/X11rdp/Xvnc` 三种 xserver 路径。验证方法：在目标发行版装 xrdp 后 `ps -ef | grep -E 'Xvfb|Xorg|Xvnc'` 与 `ls /tmp/.X11-unix/` | — |

**结论**：Linux 上没有"我正在被远程控制"的系统级布尔值。可行的替代信号是 §8 的进程/端口 + §9 的捕获枚举，`logind` 只作为辅助（区分 tty 远程登录）。

---

## 6. 开机自启

| 方式 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| XDG autostart `.desktop` | 可行 | 规范：目录为 `$XDG_CONFIG_DIRS/autostart`，未设置时用户级是 `~/.config/autostart/`、系统级 `/etc/xdg/autostart/`；同名文件以更重要的目录为准；`Hidden=true` 时**必须**忽略；`OnlyShowIn`/`NotShowIn` 限定桌面；`TryExec` 不匹配则不启动。URL: https://specifications.freedesktop.org/autostart/latest/ | 低。缺点：只在 DE 登录后执行，无重启策略、无依赖 |
| systemd user unit | 可行（更强） | `systemd-xdg-autostart-generator` 会把 XDG autostart 文件转成 user service，由 `xdg-desktop-autostart.target` 拉起；`Type=`/`Restart=` 等语义见 systemd.service(5)。URL: https://www.freedesktop.org/software/systemd/man/latest/systemd-xdg-autostart-generator.html | 中。需 `systemctl --user enable`，且用户未登录时 `--user` 实例默认不运行（除非 `loginctl enable-linger`） |
| 差异要点 | — | autostart 是"DE 启动时跑一次"；user unit 是"受 systemd 监管的进程"，可 restart、可 `After=` 排序，但需要 enable 且依赖 linger 才能在无登录时运行 | — |

**建议**：X11 场景用 autostart（无侵入）；需要"远程会话中也要运行"时用 user unit + linger。二者不要同时注册，会双实例。

---

## 7. 打包与更新

| 项 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| AppImage 自更新 | 有条件可行（必须做） | 官方：可更新前提是把 update information 嵌进 AppImage（`appimagetool -u "zsync|https://.../*.zsync"`），或用 `AppImageUpdate`/`libappimageupdate`（`appimage::update::Updater`）。并明确：**electron-builder 产出的 AppImage "cannot be updated using the usual tools"**。URL: https://docs.appimage.org/packaging-guide/optional/updates.html | 中。Tauri 的 updater 插件在 Linux 只产出 AppImage + `.sig`（"On Linux, Tauri will create the normal AppImage inside target/release/bundle/appimage/"），deb/rpm 走不了这条链。URL: https://v2.tauri.app/plugin/updater/ |
| deb/rpm 签名与仓库 | 有条件可行 | Tauri 官方有 Linux 代码签名页（https://v2.tauri.app/distribute/sign/linux/）；仓库分发需自建 apt/dnf 源，Tauri 无内置支持 | 中高。要维护 repo + GPG key + 更新元数据 |
| Flatpak 沙箱对窗口控制的影响 | X11 可行 / Wayland 不可行 | 默认沙箱"No access to host services like X11"；`--socket=x11` 授予 X11 访问；`--socket=fallback-x11` 只在无 Wayland 时生效（"Show windows using X11, if Wayland is not available"）。URL: https://docs.flatpak.org/en/latest/sandbox-permissions.html | 中。Flatpak 版必须带 `--socket=x11`，且只在 X11/XWayland 会话下能操作外部窗口；纯 Wayland 会话里**任何**打包方式都不行（§3） |
| Wayland 下的通用结论 | 不可行 | 同上：`--socket=wayland` 只给"Show windows with Wayland"，协议层无移动他人窗口能力 | — |

---

## 8. 各远程工具的可检测信号

**纪律**：端口只写有官方文档的；`0`/随机端口的写"随机"。进程名无官方依据的一律 `不确定`。

| 工具 | 平台 | 进程名 | 默认端口（官方依据） | 日志路径 | 会话可检测性 |
|---|---|---|---|---|---|
| TeamViewer | Win | `不确定`（验证：`tasklist \| findstr /i teamviewer`） | **无入站端口需求**；出站 TCP/UDP **5938** 主、TCP 443 回退、TCP 80 最后手段（官方 KB 明确 "It does not require any inbound ports to be opened"） | `C:\Program Files (x86)\TeamViewer`（Full/Host）；QuickSupport 在 `%APPDATA%\TeamViewer`（官方 KB "Find your log files"） | **B+**：`Connections_incoming.txt` 列出所有成功连接；主日志可匹配 `CreatePassiveSession`（会话开始）/`SessionManagerDesktop::SessionTerminate: removing session with TVSessionID`（结束）。官方警告日志"designed for troubleshooting"、默认 1MB 滚动、且 "special training is required" |
| TeamViewer | Linux | `不确定`（`teamviewerd` 仅见于社区帖；验证：`systemctl status teamviewerd`） | 同上（官方端口表覆盖 Linux） | 无固定路径，官方只给 `sudo teamviewer ziplog` 打包 | B-（同上，但路径不确定） |
| AnyDesk | Win/mac/Linux | `不确定`（验证：`tasklist`/`ps`/活动监视器） | 直连 **TCP 7070**（官方 Settings 页："AnyDesk uses TCP port 7070 by default for direct connections"）；防火墙放行 TCP 80/443/**6568**、UDP **50001–50003**（Discovery，多播 239.255.102.18） | Win：`%APPDATA%\AnyDesk\ad.trace`、`%PROGRAMDATA%\AnyDesk\ad_svc.trace`；macOS：`~/.anydesk/anydesk.trace`、已安装 `/var/log/anydesk.trace`；Linux：同 macOS 路径 | **B**：`connection_trace.txt`（"same folder as the Configuration Files"）含连接时间与接受/拒绝；文件格式**未公开** → 解析稳定性 `不确定` |
| RustDesk | Win/mac/Linux | `不确定`（验证：`tasklist`/`ps -ef \| grep -i rustdesk`） | **21118** = "Direct IP access port" 默认值（官方 Advanced Settings：`direct-access-port` default `21118`，`direct-server` 默认 `N`）。服务端端口 21114–21119（官方 self-host："Core Ports: TCP 21114-21119, UDP 21116"） | `不确定`（官方文档未给客户端日志路径） | **C**：只能判"运行中"；会话级需自建日志采集 |
| Parsec | Win/mac/Linux | Linux 官方：`/usr/bin/parsecd`（官方 Linux 页 "File locations"） | **无固定端口**：`network_server_start_port` 默认 `0` = "pseudorandom number"；`network_client_start_port` 默认 `0`；STUN 服务器 UDP **3478**（官方 Components 页）。**brief 里"8000-8040"是错的** —— 8000 只是文档举例（"If you for example use port 8000 as the starting port..."） | Linux：`~/.parsec/log.txt`（官方）；配置 `~/.parsec/config.txt` | **C**：日志存在但无公开的会话开始/结束格式 |
| Sunshine | Win/mac/Linux | `不确定`（官方文档以 `sunshine` 命令调用；验证：`ps -ef \| grep sunshine`） | `port` 默认 **47989**（官方配置文档）；官方 Docker 说明另列 47984/47990/48010/47998/47999/48000/48002，其中仅 **47990** 明确为 Web UI（"The internal port must be 47990"），其余端口用途 `不确定` | Linux/macOS：`~/.config/sunshine`（配置目录；日志文件未在官方文档列明 → `不确定`） | **C**：无公开日志格式 |
| NoMachine | Win/mac/Linux | `不确定`（验证：`ps -ef \| grep -E 'nxserver|nxnode|nxd'`） | **TCP 4000** = nxd（官方 "Default ports used by NoMachine"："The nxd service listens by default on port 4000"）；v8+ UDP 4000（旧版 4011–4999）；Windows nxsshd **4022**（Linux/macOS 用系统 sshd 22）；nxhtd HTTPS **4443**（≤v8 为 4080）；display service = 6000+DisplayBase(1001)=**7001**；MDNS UDP **5353** | `不确定` | **C** |
| ToDesk | Win | 官方 CLI 页出现 `C:\Program Files\ToDesk\ToDesk.exe` | 无固定监听端口；官方只给**出站**放行：TCP 443、8080/8081/10888，UDP 10999，P2P 动态 UDP 20000–30000 | `不确定` | **C** |
| 向日葵 Sunlogin | Linux | 官方 Linux 页：`/usr/local/sunlogin/bin/sunloginclient` | 无官方监听端口文档（官方只给"telnet client.oray.net 443"连通性检查）→ `不确定` | 无固定路径；官方 FAQ 只说客户端菜单"日志"会打开日志文件夹 | **C** |
| Windows RDP | Win | `svchost`/TermService（服务名 `TermService`，官方 Q&A 提及） | TCP **3389** 默认，UDP 3389；可通过注册表 `HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp\PortNumber` 改 | Windows 事件日志 `Microsoft-Windows-TerminalServices-LocalSessionManager/Operational` | **A-**：`SM_REMOTESESSION`/`SM_REMOTECONTROL`（官方 GetSystemMetrics 文档）可直接判；事件日志有登录/重连/断开 |
| macOS Screen Sharing | macOS | `不确定`（`screensharingd` 仅见第三方资料） | TCP **5900**（Apple："a VNC server that is listening on TCP port 5900"）；High Performance 用 UDP **5900/5901/5902**（Apple 文档） | `不确定` | **B**：可用 `lsof -i :5900` + 连接状态判"有人在连"，无法判"在操作" |
| VNC（RealVNC/TigerVNC） | 跨平台 | `不确定` | TCP **5900**，第 N 个服务器 **5900+N**；HTTP applet **5800**；反向模式客户端 **5500**（RFC 6143 §2，IANA 只分配了 5900） | 服务器各异 → `不确定` | **B**：端口 + 是否有 ESTABLISHED 连接即可判"有会话"，内容不可知 |

**评级定义**：A=有官方系统级 API；B=官方日志/端口可判会话起止；C=仅能判"工具在运行"。
**注意**：除 TeamViewer/AnyDesk 外，没有任何一家给出稳定的"会话开始/结束"日志契约。把 B 当 A 用会在升级后静默失效。

---

## 9. 能否反查"当前谁在捕获屏幕"

### 9.1 三平台结论

| 平台 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| macOS ScreenCaptureKit | **不可行** | `SCShareableContent` 只提供 `displays`/`windows`/`applications` 与三个 `getExcludingDesktopWindows(...)` 工厂方法、`info(for:)`；**没有任何"当前谁在捕获"的属性**。`SCShareableContentInfo` 只有 `contentRect`/`pointPixelScale`/`style`（描述"我这条流"）。URL: https://developer.apple.com/documentation/screencapturekit/scshareablecontent 、https://developer.apple.com/documentation/screencapturekit/scshareablecontentinfo | — |
| Windows Graphics Capture | **不可行** | `GraphicsCaptureItem` 成员仅 `DisplayName`、`Size`、`Closed`、`CreateFromVisual`、`TryCreateFromDisplayId`、`TryCreateFromWindowId`。`IsBorderRequired` 是"我要不要给自己的捕获加边框"，不是查询他人。URL: https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureitem | — |
| Linux portal ScreenCast | **不可行（无查询接口）** | 接口属性只有 `AvailableSourceTypes`、`AvailableCursorModes`、`version`；`org.freedesktop.portal.Session` 只有 `Close()` 与 `Closed` 信号。**没有**"列出活跃会话"的方法。URL: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html 、.../doc-org.freedesktop.portal.Session.html | — |
| Linux（补充）Mutter 私有 ScreenCast | 不可依赖 | 该 XML 自述："This API is private and not intended to be used outside of the integrated system that uses libmutter. **No compatibility between versions are promised.**" URL: https://gitlab.gnome.org/GNOME/mutter/-/raw/main/data/dbus-interfaces/org.gnome.Mutter.ScreenCast.xml | — |

### 9.2 为什么"不能"是结构性的
捕获是**接收方**的行为：发送方（被捕获的机器）只看到"有个客户端在跟我说话"。三平台都只在**捕获发起方**侧有对象模型（SCStream / GraphicsCaptureSession / portal Session），而这些都是**按调用者隔离**的——D-Bus session 句柄按 `SENDER` 命名（`/org/freedesktop/portal/desktop/session/SENDER/TOKEN`，见 Session 文档），别人拿不到你的句柄列表。

### 9.3 替代信号集（建议实现顺序）

| 优先级 | 信号 | 平台 | 依据 | 可靠性 |
|---|---|---|---|---|
| P0 | 已知工具进程 + 其监听端口/ESTABLISHED 连接 | 全平台 | §8 表 | 高（对已知工具），漏报新工具 |
| P1 | PipeWire 节点枚举：`media.role=Screen` / `media.class=Video/Source` + `application.name` | Linux | `pw-dump` 输出"nodes, devices, modules, ports"（https://docs.pipewire.org/page_man_pw-dump_1.html）；键 `PW_KEY_MEDIA_ROLE`（取值含 `Screen`）、`PW_KEY_APP_NAME`、`PW_KEY_MEDIA_CLASS`（https://docs.pipewire.org/group__pw__keys.html） | 中：只覆盖走 PipeWire 的捕获；Sunshine 的 `kms`/`x11` 后端、x11grab 不出现 |
| P1 | 虚拟显示器是否存在（evdi/Idd/…） | 全平台 | §4 | 中：只能说明"有人开了虚拟屏" |
| P2 | 输入设备为虚拟设备（uinput 等） | Linux | §10 | 中：不能归因 |
| P2 | 屏幕是否在锁屏/无人在场（`XScreenSaverQueryInfo` idle） | Linux X11 | §5 | 低（辅助） |
| P3 | macOS 菜单栏"屏幕录制"指示器 / Windows 无对应 | macOS | 系统 UI，无 API | 不可自动化 |

**明确不建议**：hook 系统截屏 API、读 `pw-dump` 猜"谁在看"。前者在 macOS 需要 TCC 且会被 SIP 限制，后者只能猜。

---

## 10. 虚拟 / 注入输入检测

| 平台 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| Windows | 有条件可行（判"注入"，不判"远程"） | `MSLLHOOKSTRUCT.flags`：`LLMHF_INJECTED (0x1)` "Test the event-injected (from any process) flag"、`LLMHF_LOWER_IL_INJECTED (0x2)`。URL: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct | 中。需 `SetWindowsHookEx(WH_MOUSE_LL)` 全局钩子；且**硬件级方案（Parsec VUSB / 虚拟 HID 驱动）不置此位**——它们是真实 HID 设备，`LLMHF_INJECTED` 为 0 |
| Windows：`0xFF515700` 签名 | **不确定，不要用** | `GetMessageExtraInfo` 官方文档只说"The meaning of the extra information is device specific"、"application- or driver-defined value"，**没有任何 0xFF515700 的定义**。URL: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessageextrainfo | 不可依赖（未文档化的实现细节，随时变） |
| Linux | 有条件可行（能判"虚拟设备"，不能判"注入来源"） | uinput 创建的设备 sysfs 路径固定为 `/sys/devices/virtual/input/--NAME--`（`UI_GET_SYSNAME` 文档注释，include/uapi/linux/uinput.h）；`UI_DEV_SETUP` 可自定义 `name`/`id.vendor`/`id.product`，所以**名字可伪装**。URL: https://raw.githubusercontent.com/torvalds/linux/master/include/uapi/linux/uinput.h | 中。可枚举 `/sys/class/input/*/device` 是否在 `/sys/devices/virtual/`；libinput 侧只要求 `ID_INPUT*` udev 属性（https://wayland.freedesktop.org/libinput/doc/latest/device-configuration-via-udev.html），不区分虚拟/物理 |
| Linux X11：区分 XTest 注入 | **不确定** | XI2 的 `sourceid` 是"source device that originally generated the event"（XI2proto §DeviceEvent），XTest 注入的事件通常表现为 master device，**协议里没有"injected"标志**（只有滚动仿真用的 `XIPointerEmulated`/`XIRawEmulated`）。验证方法：用 `xdotool` 注入并在 XI2 raw 事件里打印 `sourceid`/`flags`，与真实鼠标对比 | 中 |
| macOS | 有条件可行（可判"哪个进程投递"） | `CGEventField.eventSourceUnixProcessID`：字段含"the event source Unix process ID"；`kCGEventSourceStateID` 亦作为字段可读（`CGEventField.eventSourceStateID`）。`kCGEventSourceStateHIDSystemState` 的语义是"combined state of **all hardware** event sources posting from the HID system"，是**创建事件源用的状态表**，不是检测器。URL: https://developer.apple.com/documentation/coregraphics/cgeventfield/eventsourceunixprocessid 、https://developer.apple.com/documentation/coregraphics/cgeventsourcestateid | 高。需辅助功能权限 + `CGEventTap`；且对端若用 driver-level（虚拟 HID）同样绕过 |

**统一结论**：三平台都只能回答"这条事件是不是软件投递的 / 来自哪个虚拟设备"，**都不能回答"是不是远程会话在操作"**。远程判定仍应回到 §8/§9 的进程-端口-捕获信号。

---

## 11. 窗口迁移后应用自我纠正（回弹）

| 项 | 结论 | 依据 | 落地代价 |
|---|---|---|---|
| 有公开证据表明 Electron 在外部移动后回弹？ | **不确定（未找到）** | Electron 官方文档只描述事件语义：`'move'` 事件在"the window is being moved to a new position"时触发；`'will-move'`（macOS/Windows）"only emitted when the window is being moved manually"，用 `setPosition`/`setBounds`/`center` 移动**不会**触发。没有"外部移动后自行回弹"的记载。URL: https://www.electronjs.org/docs/latest/api/browser-window | — |
| 相反方向的证据：启动时恢复几何 | 存在 | Qt 官方："On X11, this might not work because an invisible window does not have a frame yet... the window shifts towards the bottom/right corner" —— 这是**启动时**的几何恢复问题，不是运行时回弹。URL: https://doc.qt.io/qt-6/restoring-geometry.html。Chromium 侧 `chrome/browser/ui/window_sizer/window_sizer.cc` 负责"恢复上次持久化的 bounds"，同样是启动路径 | 低（只需在迁移后重新校验一次几何） |
| 唯一找到的"snap back"实例 | 条件性，且非 Linux 桌面 | Chromium commit 51b855c7 提交说明："Since widget bounds were updated using coordinates relative to the root window, windows on secondary displays would **snap back to the primary display** after resizing."（ChromeOS/Ash 场景，已修复）。URL: https://chromium.googlesource.com/chromium/src/+/51b855c765c5da89dad5ee3af9b0e138f7b75d08 | — |
| 规范层面 | 应用**不该**回弹 | ICCCM："If the window manager moves a top-level window without changing its size, the client will receive a synthetic ConfigureNotify event... **Clients must not respond to being moved by attempting to move themselves to a better location.**" URL: https://xorg.freedesktop.org/archive/X11R7.6/doc/xorg-docs/specs/ICCCM/icccm.html | — |

**落地建议**：不要为"回弹"设计补偿机制。改为迁移后 300–500ms 读一次 `_NET_FRAME_EXTENTS` + 实际几何，若与目标偏差 > 阈值则**记录并放弃**（避免与 WM 打架）。验证方法：在 GNOME/KDE/XFCE 各跑一遍，覆盖 Chromium 系（VS Code/Slack/Discord）、Firefox、LibreOffice、GIMP，观察 10s 内几何是否漂移。

---

## 风险最高的三条

1. **Wayland 根本没有"移动他人窗口"的协议能力，任何规格若写了"Wayland 下用 X 方法搬窗口"都是幻觉。** xdg-shell 只有 user-driven 的 `move`/`resize`；`wlr-foreign-toplevel-management` 只有状态类 request，没有 set_position；连 `getBounds()` 都返回 `{0,0}`。唯一出路是两套私有适配器（KWin 脚本 D-Bus `org.kde.kwin.Scripting` + GNOME 扩展 `Meta.Window.move_frame`），而 GNOME 45/46/48 连续三年破坏性改动（ESM 强制、`ExtensionState` 改名、`Meta.get_window_actors` 迁移）意味着**每个 GNOME 大版本都要重测并重发**。这是长期维护成本最高的一项，必须在产品层面先决定"Wayland 是否降级为提示用户手动搬"。

2. **"检测谁在截屏"在三平台都无 API，规格里任何 `SCShareableContent.isBeingCaptured`、`GraphicsCaptureItem.IsCaptured`、portal `GetActiveSessions` 之类都是编造的。** 已核对的真实成员：`SCShareableContent` 只有 displays/windows/applications；`GraphicsCaptureItem` 只有 DisplayName/Size/Closed 与三个工厂方法；ScreenCast 只有 AvailableSourceTypes/AvailableCursorModes/version，Session 只有 Close/Closed。必须把产品语义降级为"检测已知远程工具是否活跃 + 是否有虚拟屏/虚拟输入"，并明确写出**误报/漏报**的边界。

3. **端口与日志是最容易写错、也最容易静默失效的一层。** 已确认的纠偏：Parsec **没有** 8000–8040 默认端口（`network_server_start_port` 默认 0=随机，8000 只是文档示例）；TeamViewer **不需要任何入站端口**（5938 是出站）；AnyDesk 的 7070 只用于**直连**，防火墙放行表里根本没有 7070（是 80/443/6568 + UDP 50001–50003）；Sunshine 只有 `port=47989` 与 Web UI 47990 有官方定义，47984/47998–48000/48010 的**角色**在官方文档里未逐条说明。日志侧只有 TeamViewer（`Connections_incoming.txt` + `SessionTerminate` 标记）与 AnyDesk（`connection_trace.txt`，格式未公开）能到"会话级"，其余九项都只能判"进程在跑"。若规格把 C 级信号当 A 级用，上线后会出现"连上了但没迁移"的静默失败。
