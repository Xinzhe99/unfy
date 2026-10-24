# 05 · 平台能力、设置、升级与发布规格

状态：待实施。不能继续把「存在代码」「通过某个库的类型检查」「完成当前平台桌面验收」视为同一支持等级。

## 1. 当前平台事实

| 能力 | Windows 当前 | macOS 当前 | Linux 当前 |
|---|---|---|---|
| 本机显示器、普通窗口控制 | 原生后端存在，用户报告多项不可用；本次只读探针/界面有观察 | AX/Quartz 后端存在，但有静态类型和行为阻断，未真机验收 | 没有 X11/Wayland 后端 |
| 目标截图/编号 | 有实现，实际 UI 能显示 | 返回空图/Unavailable | 同样不可用 |
| 应用选择列表 | 有实现 | 恒为空 | 恒为空 |
| 远程自动检测 | 进程/网络/会话等信号存在，可靠连接能力不足 | 直接返回空观测 | 直接返回空观测 |
| 崩溃恢复 | 有 journal，但存在 02 所列缺陷 | 持久 AX 指针身份无效，错误分类会丢责任 | FakeBackend 不代表真实恢复 |
| 自启 | 插件接入，错误未传给 UI | LaunchAgent 插件接入，未原生验收 | 插件接入不代表窗口功能支持 |
| 签名发行/真实更新 | 当前配置无 key/endpoint；历史安装记录不等于当前可更新发布 | 未签名公证与原生更新验收 | 没有 Linux 发布目标 |

证据：`apps/desktop/src-tauri/src/platform.rs:7–60,116–184`、`crates/detect/src/lib.rs:34–57`、`tauri.conf.json:30–61`、平台后端源码、`.github/workflows/ci.yml`。现有 Windows 程序运行时状态与源码缺陷对应；其他平台结论主要来自静态审查，不冒充本次编译或运行结果。

## 2. 能力模型与诚实降级

移除 unsupported 平台生产路径中的 `FakeBackend::with_two_displays()`。Fake 只用于测试；产品运行时不能返回两块虚构屏幕或“成功整理 0 个”。

对真实功能返回 `available | permission_required | unsupported | temporarily_unavailable`，必要时附原因：enumerate_windows、move_windows、restore_identity、display_topology、display_capture、identify_displays、application_picker、remote_lifecycle、autostart、updater。

所有 command、托盘、快捷键、自动任务与 UI 共享能力准入；不能仅在 React 禁用按钮。Unsupported 返回结构化错误、不创建 journal、不记录成功活动。授权不足时保留已有记录，先提示授权，再允许对应操作；不能声称未授权仍能执行依赖 AX 的预览。引导也必须先显示必要权限，不能提前 return 掩盖权限提示。

目标绑定保存 identity_quality；弱身份只在已确认的当前拓扑有效。屏幕编号不是稳定 ID。桌面截图/编号不可用时提供真实设备名称、分辨率与位置示意，允许用户确认，不要求为了基本移动先实现截图功能。

## 3. 设置一致性与升级

落点：`crates/settings/src/lib.rs`、`commands.rs:953–1166,1733–1768`、`lib.rs` 初始化、`session.rs` 自启/热键。

### 3.1 单一有效配置

实现唯一 `engine_config_from_settings`（名称可沿仓库习惯），启动、保存、重置全部调用。禁止硬编码第二份默认值。公开界面值和当前有效配置 revision 必须一致。

保存流程：克隆旧 Settings → 在候选副本合并 patch → 全量 validate → 形成所需 OS 副作用 → 写临时文件并 flush/可靠替换 → 发布内存设置及 engine/detector 同代配置。任一验证/文件写失败保持旧有效值。不得先改共享 Settings 再发现非法字段。

OS 副作用（自启、快捷键）不能与文件写声称天然原子：记录 desired/actual/error，执行后读取实际结果；若需要补偿但失败，返回明确不一致状态，不能提示全部成功。通过同一协调入口避免与当前窗口事务交错。

活动事务期间改变布局/排除规则：对当前前像和已提交计划无影响；新配置在下一次显式整理/新窗口纳入时生效，UI 说明生效范围。更换目标走 03 的 retarget 命令，不由普通 save_settings 偷偷搬窗。关闭自动化仅停止后续自动动作，不先恢复窗口。

数值输入由持久化返回值驱动；不能用 `defaultValue` 永久显示过期值。重置默认保留用户确认的目标和未完成 journal，应用同一配置转换；不隐式放弃正在整理的事务。

### 3.2 Schema 迁移

当前结构有 `deny_unknown_fields`，删除字段/枚举必须通过版本化迁移读取旧文件，不能让正常升级被当作 settings.json.invalid 并清空用户设置。

| 旧值/字段 | 新行为 |
|---|---|
| `layout_mode=default` / KeepLayout | 改用 01 的逐窗移入语义；当前事务不重新布局 |
| `primary_side` | 迁为默认移入，首次升级说明「主次排列已移除，下次整理保留窗口尺寸」；不重排当前桌面 |
| `grid` | 保留为明确高级排列 |
| `telemetry` | 删除无效偏好，不新增上传实现 |
| `last_refused_apps` | 从持久设置移除；逐窗操作结果承担失败信息，不自动排除 |
| `auto_detect` 弱证据级别 | 迁成手动/只显示候选证据；未经验证不保留自动移动授权；显示一次原因 |
| `remote_screens=all` | 保持不自动整理的原意，迁成手动策略；不得转成自动开启 |
| 手动 marker/deferred_restore | 取消无对应连接代次的运行状态；现有恢复记录保持 |
| 主题、语言、快捷键、排除规则、目标、自启选择 | 完整保留并验证 |

读取旧文件失败时保留原件、给出版本/字段原因；不要把合法旧 schema 与损坏混为一谈。迁移后再次启动幂等。

### 3.3 Conflux → Unfy 数据迁移

当前 `settings/src/lib.rs:476–508` 的回退只复制顶层文件，漏 journal 子目录，并因新目录存在永久跳过；这是恢复入口可达性缺陷。

改成显式启动阶段，放在日志目录创建与 settings/journal 打开之前：

1. 探测旧/新目录及迁移完成标记；路径解析本身不再搬数据。
2. 只有旧目录：优先原子 rename；失败则复制整个必要目录树到独立临时目标。设置和 journal 全部成功并验证后再提交目标目录。
3. 复制失败/中断：旧目录保持可用；临时目标不能被视为完成。下次可继续或重做，不因新目录存在跳过。
4. 两目录并存：不覆盖新设置；扫描两边的未解决 journal，按 tx_id 去重且检测内容冲突。未知冲突显示恢复问题，不能只选目录 mtime 较新者。
5. 未解决恢复记录迁移完成前禁止新整理；保留备份直到目标读取成功。已解决日志与普通日志的清理不能波及恢复记录。

验收必须覆盖嵌套 journal、rename 失败、任一文件复制失败、重启中断和双目录并存。历史一次成功 rename 的实测只证明那条路径。

## 4. macOS 修复与发布前提

当前发现：

- `settings/src/lib.rs:438–450` 的 HOME 是 String 却调用 `.join`；`mac/tests/geometry.rs:46–63` 构造缺 `min_size/focused`。这些是静态可见编译问题，本次未跑 mac 编译。
- `mac/src/ax.rs:101–168,218–225` 以裸 CF 指针缓存/持久化窗口；跨线程所有权与 Send 要求未设计。不得裸加 `unsafe impl Send` 消除编译问题。
- `ax.rs:319–324,356–369` 把属性读取错误变成 Gone；`400–405` 把布尔属性作为 CFNumber 读取；Copy/get ownership 不匹配；超时只设 application 引用。
- `mac/src/displays.rs:28–113` 的缓存不失效，`prime_work_areas`/`invalidate` 未接入；工作区回退整个 bounds；基于 `NSScreen.mainScreen` 翻转坐标会随焦点屏改变参考。
- 枚举未排除 Unfy；missing subrole 被当作 standard；can_resize 没有被引擎使用。

实施：

1. AX 引用放在明确的单一工作线程，使用 owned/retained 包装；跨线程传值快照、命令和结果。NSScreen/AppKit 查询仍在主线程；不让整个 GUI 等待无响应应用。
2. 使用 CFBoolean/CFNumber/AXValue 各自类型检查；未知不当 false。每个被访问的对象有有界 messaging timeout，读取/写入错误保留真实类型。
3. 同次运行建立稳定的逻辑窗口表；重枚举不换身份。跨运行无法证明连续性则待确认，不能用 AX 地址或模糊标题自动恢复。
4. 启动主线程采集工作区并原子发布 topology revision；显示重配置、分辨率/排列、Dock 工作区变化使缓存失效；若继续轮询，必须实际刷新而非永远返回缓存。
5. 坐标统一称「平台桌面单位」：WindowsPhysicalPx、MacPoints（未来 X11Px）；journal/DTO 明确 coordinate_space。mac 的几何 scale=1，Retina backing_scale 单独仅用于截图/分辨率展示。避免 point→再乘 Retina 比例。AppKit 翻转以全局主参考坐标为准，不随焦点 `mainScreen` 改变。
6. 默认普通移动采用经过 AX 兼容验证的 size→position→size，MoveOnly 不写 size；读回确认。全屏保持跳过；只处理本次可确认的 Space 范围，不用私有 API 强制跨 Space。
7. 正常应用名与排除键分离：显示本地化名称，规则用 bundle ID/稳定 app identity；完善真实应用选择器，不能要求用户在 mac 输入 `.exe`。
8. 截图/屏幕编号可作为独立可用性增强；未实现时真实降级。Accessibility 与 Screen Recording 是两种能力，移动不应为了截图强索屏幕录制权限。

必须原生验收：Apple Silicon 与 Intel 发布架构、两屏混合 Retina、负坐标/上下排列、Dock/菜单栏、热插拔、全屏/最小化/固定尺寸、无响应 app、拒绝/授权/撤权/重新授权、多次预览后恢复、强杀后恢复审查。签名相同身份升级后 TCC 行为实测；不能把“换证书”简单等同必然丢授权。

## 5. Linux 范围

原始跨平台目标保留；既有 `docs/plan.md` 已将 Linux 后移。本轮先移除生产 Fake、修正能力和文案，不把新增 Linux 窗口后端夹进 Windows 核心修复。

后续 X11 实现包须独立完成：EWMH 客户窗口清单与桌面归属 → RandR 稳定显示器/工作区 → frame extents/root 坐标归一 → `_NET_MOVERESIZE_WINDOW` 与状态请求 → 写后实际几何确认 → XID 生命周期/进程身份 → journal/退出屏障共用 → 至少两个实际 WM 验收。具体 EWMH 能力在 WM 不支持时返回 Unsupported，不假成功。Wayland 当前没有通用跨应用位置控制；不以 XWayland 进程存在宣称整个桌面支持，不新增 GNOME/KWin 插件或驱动来扩大本轮范围。

对外支持名单：Windows 通过本轮门禁后才标可用；macOS 通过独立门禁后再发布；Linux 未实现、Wayland 不承诺通用搬窗。这样保留长期目标，也不让未完成平台冒充已交付。

## 6. 自启、更新、安装/卸载

### 自启

复用 Tauri autostart 插件，返回 desired/actual/error，错误不吞；用户开启后在已安装路径实际登录验证 `--tray` 静默启动。已存在但 exe 路径过期要修复，不能只比较 enabled 布尔值。

改名升级核查当前用户 Run/login item，只有确认属于旧 Conflux 安装的条目才移除；保存自启偏好，保证最多一个有效启动项。不能删除同名但路径不属于产品的第三方条目。mac 目前是 LaunchAgent，文档不得写成已实现 SMAppService。

### 更新

- 「检查更新」只查询，返回 up_to_date/available/unavailable/failed；不会自行安装重启。
- 开启自动更新可后台检查、下载并验签；用户正在整理/远程工作时推迟安装。安全空闲时按既有自动更新偏好安装；手动「安装并重启」也必须通过 02 屏障。
- 状态实际反映 checked/downloading/verified/deferred/installing/restart_failed；未下载不说“已下载”。安装前重新检查全部恢复义务和维护代次。
- 签名不符、下载中断、安装失败保留当前程序/设置/journal；重启失败有明确恢复入口。不能把插件返回 Ok 当作新程序确实运行。
- 上次 deferred 在恢复义务清零后的安全空闲点再评估，不要求必须下次启动才能发现更新；仍不得在活跃远程工作中突然重启。
- 未配置 key/endpoint 的开发构建禁用自动检查/安装，保留明确说明；不填假地址或临时公钥当正式交付。

### 发行链

当前 `.github/workflows/ci.yml:3–7` 不监听 tags，bundle 却按 tags 判断；bundle 未安装 Bun，build 脚本调用 bun test；产物缺失仅 warn、未上传 `.sig` 或生成清单。`tauri.conf.json` key/endpoint 为空且没有 updater artifact 配置。

修正清单：

1. native crate/二进制/dependency 按目标隔离，Windows-only crate 不破坏 mac/Linux workspace 检查。Mac 检查包含 desktop/settings/tests，不能只 check platform-mac 库。
2. 普通 CI 使用锁定工具链/依赖；发布监听 `v*` tag 或明确手工发布。所有 job 安装实际依赖，避免重复前端全量测试多次。
3. 生成与平台/架构一致的安装包、更新包、签名及清单；缺任何必需产物 hard fail。Universal 包对应正确 Darwin 更新键。
4. Windows Authenticode、mac Developer ID/notarization/stapling、updater 签名分别验证，不把其中一种当成其他签名。秘密只在 CI secret 中，文档不得填真实私钥。
5. 在干净用户环境完成旧版安装→运行/自启→保留配置升级→重启→恢复记录可读→卸载；验证安装器关闭进程前已经满足恢复屏障。未解决时提示退出/保留，而非强杀后删 journal。
6. 程序回退路径与 settings/journal schema 兼容性一起验证；未实测回退不能宣传自动回滚。

本次没有证书、真实更新源和其他平台机器的发行验收结果。后续缺这些时可以完成本地实现及离线坏签名/失败处理，但必须保留发行门禁，不能写成已可无感更新。

## 7. 本地诊断、资源和维护性

保留 `capabilities/default.json` 的窄 IPC 与现有 CSP；不给前端增加任意命令、任意文件系统或 HWND 操作。诊断采用白名单字段：版本/平台、脱敏 topology、能力、操作/原因计数、最多 50 条事件；不包含屏幕图像、窗口标题、文档路径、原始进程命令行或完整设备序列号。逐窗应用名默认在本地结果页显示；导出需预览内容，不自动上传。

`open_path` 只需打开 app 的 config/diagnostics 等固定位置，改成位置枚举，移除不必要的任意字符串路径入口；不扩大权限。

活动历史继续最多 200 条；未解决 journal 不受普通日志淘汰策略影响。当前日志注释写每文件 2MiB，实际只有按日轮转和最多五文件（`lib.rs:191–225`）。实现所声明的大小上限或改为诚实的容量配置；错误风暴去重，不能每轮采样刷同一错误。保留 worker guard 在正常退出 flush，而非靠 forget 永久泄漏生命周期。

排除页不得抓屏；隐藏窗口停所有无用轮询/预览/缩略图；状态从短锁快照读取；避免每窗口全桌面枚举。以实际进程树测后台开销，不用前端 DOM 测试或单次 N 窗口运行宣称性能达标。

## 8. 验收

P-01 合法旧设置升级保留目标/规则/快捷键；非法 patch 或写入失败不改变有效配置。重置后立即行为和重启后相同。

P-02 数据迁移故障和双目录并存都不丢未解决 journal；重启可继续，已有新设置不覆盖。

P-03 自启注册/查询失败有真实状态；旧 Run 项升级清理仅限产品；登录只启动一次。

P-04 所有入口对 unsupported/权限不足一致；Linux 不显示假屏或成功记录；mac 不展示不可用自动能力。

P-05 Mac 完整目标/测试构造通过原生检查，再执行上述多屏/权限/身份矩阵。无真机证据维持未验证。

P-06 坏签名、中断、延期、下载期间新事务、安装/重启失败均不绕过屏障；真实旧包→新包升级后确认新版本进程与设置/journal。

P-07 tag 流程可达；没有签名/包/清单必需项时发布失败；不能发布假成功空产物。

P-08 诊断不泄漏标题/截图/路径；隐藏后无缩略图采集，历史与日志容量有界；恢复记录不被容量清理误删。

参考平台契约：[Windows ShowWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow)、[虚拟桌面成员查询](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ivirtualdesktopmanager-iswindowoncurrentvirtualdesktop)、[DISPLAYCONFIG_MODE_INFO](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_mode_info)、[NSScreen.main](https://developer.apple.com/documentation/appkit/nsscreen/main)。API 文档不是本项目运行验收。
