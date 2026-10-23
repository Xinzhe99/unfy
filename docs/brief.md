# Brief：Unfy（暂定名）产品与技术方案

## 你的角色与任务

你是一位同时具备 **产品经理 + 跨平台桌面系统工程师 + 商业化负责人** 能力的技术合伙人。
目标：为一个全新产品写出可直接进入实现的产品规格、技术设计与路线图。**只写文档，不写代码。**

写完后必须把内容落盘为以下文件（Markdown，简体中文，术语保留英文）：

- `docs/spec/README.md` —— 索引 + 一页纸摘要（问题、方案、差异化、里程碑）
- `docs/spec/product-spec.md` —— PRD
- `docs/spec/technical-design.md` —— 技术设计与各平台实现细节
- `docs/spec/decisions.md` —— ADR 列表（每个决策：背景 / 选项 / 取舍 / 结论 / 何时重审）
- `docs/spec/roadmap.md` —— 里程碑、范围切分、商业化、分发、风险登记册
- `docs/spec/competitive.md` —— 竞品与替代方案分析、差异化定位

## 背景

用户（客户）的痛点：

> 我在一台有 3 台显示器的电脑上工作，但经常用只有 1 台显示器的笔记本通过远程软件（TeamViewer / AnyDesk / RustDesk / Parsec / ToDesk / 向日葵 / NoMachine / VNC / 甚至 RDP）连回这台电脑。
> 远程软件里只能看到一块屏，其他屏上的窗口看不见，于是必须不停地在远程软件里"切换显示器"或在主机前手动把窗口拖过来。
> 我希望装一个工具：一旦检测到"有人在远程我"，就自动把其他显示器上的窗口迁移到被远程的那块屏上，并重排成可用布局；远程断开后自动恢复原来的位置。
> 需要无感自动更新、可配置开机自启、现代化设置界面。最终做成成熟商业软件。

仓库当前为空（greenfield），仅有一个 initial commit。工作目录名 `Unfy`。

## 已确定的技术前提（不需要你再论证，直接基于此设计）

- 桌面端框架：**Tauri 2 + Rust core + React/TypeScript + Tailwind + shadcn/ui**（如需不同结论，必须在 ADR 中说明理由与代价）。UI 面向现代桌面软件审美，深色/浅色、i18n（zh-CN / en 起步）。
- 单仓库、单进程 UI + Rust 核心；需要时增加特权 helper 进程/服务。
- 目标平台：Windows 10/11（x64/arm64）、macOS 13+（Universal）、Linux X11 优先（Wayland 降级）。
- 更新：Tauri updater + 签名校验 + 灰度通道；Windows 代码签名、macOS notarization、Linux AppImage/deb/rpm。

## 必须直面的技术现实（请逐条给出方案与取舍，不要回避）

### 1. 远程拓扑有两种，问题不同，能力要分开

1. **镜像型（mirror）**：TeamViewer / AnyDesk / RustDesk / Parsec / Sunshine / NoMachine / VNC 等，捕获主机**当前物理会话**的某一块或全部显示器。多显示器桌面被单屏客户端看到 → 需要把窗口迁移到被捕获的那块屏。这是主场景。
2. **会话型（session）**：RDP / xrdp / VNC 登录这类，远程会话自带显示拓扑。窗口通常已在本会话内，但应用**持久化的窗口位置**（尤其跨多屏保存的坐标）可能落在当前不存在的显示器上 → 需要"越界窗口救援（off-screen rescue）"。这个能力与迁移共用同一个 Window Engine。

### 2. "远程软件正在捕获哪块屏" 是不可直接查询的（最大风险）

多数远程工具不暴露"当前捕获源"。请设计分层策略：

- 默认目标 = 主显示器（可配置：主屏 / 指定屏 / 虚拟屏）。
- 热键循环目标屏 + 托盘可视化提示。
- 规则包可为特定工具声明默认捕获屏/捕获模式的启发式。
- **旗舰方案：虚拟显示器**（Windows IddCx/IDD 驱动、macOS `CGVirtualDisplay` 私有 API、Linux evdi），让远程工具捕获一块专用于远程的虚拟屏，物理屏完全不受影响。代价：Windows 驱动需 EV 证书 + attestation 签名、macOS 私有 API 不可上架 MAS、Linux 需 DKMS 内核模块。请给出"分阶段引入"的方案与法律/成本评估。
- **备选方案："强制单屏"**：会话期间通过 CCD API（Windows）/ CGDisplay（macOS）/ xrandr（X11）临时禁用副屏，使远程端只看到一块屏。请评估副作用（本机用户失去屏幕、远程工具显示器列表缓存、热插拔事件、恢复失败风险），并决定是否作为可选高级模式。

### 3. 窗口迁移引擎的工程细节（请给出算法与伪代码级描述）

- 枚举：Win32 `EnumWindows` / macOS AXUIElement / X11 EWMH `_NET_CLIENT_LIST`。
- 过滤：不可见、`WS_EX_TOOLWINDOW`、零尺寸、owner 弹窗、IME/输入法候选、系统外壳（Shell_TrayWnd、桌面、任务栏）、本应用自身、全屏游戏/DRM 独占、UWP 框架窗口。
- 坐标与 DPI：Windows Per-Monitor DPI Awareness V2、物理像素 vs DIP、`GetDpiForWindow`、跨 DPI 迁移时的尺寸换算；macOS points vs pixels、Retina 缩放；X11 无缩放但需处理 Xinerama/RandR 布局。
- 状态：最大化 / 最小化 / 全屏（Windows `WS_MAXIMIZE`、macOS 原生 fullscreen Space）/ 无边框全屏 / 置顶 / 跨虚拟桌面（Windows IVirtualDesktopManager、macOS Spaces）。迁移前先 `Restore`，迁移后按策略重建状态。
- 提权窗口：Windows UIPI 阻止低完整性进程操作高完整性窗口 → helper 服务方案（Named Pipe + 调用方签名校验 + 最小操作面）。评估 helper 是否 v1 必需。
- 失败模式：`SetWindowPos` 被拒、应用自我纠正（如 Electron 记住位置）、WM 最小尺寸约束、动画闪烁（`SWP_NOREDRAW`/`SWP_ASYNCWINDOWPOS`）、上百窗口的性能预算（p95 < 300ms）。
- 布局引擎：不是简单堆叠。按目标屏分辨率与窗口数量生成槽位（网格 / 主次分区 / 保留相对顺序），保持焦点窗口与 z-order 语义，支持用户自定义模板；窗口数与分辨率不匹配时的降级策略。

### 4. 恢复的语义与正确性

- 快照：`(稳定窗口标识, 显示器身份, rect(DIP), 状态, 时间戳)`。显示器身份用 EDID/device path，不用索引。
- 持久化 + 崩溃恢复：写盘 journal（原子写），进程被杀/断电后下次启动可恢复；"退出前必须先恢复"。
- 冲突策略：会话期间用户手动移动过的窗口，恢复时是否覆盖？给出默认策略与配置项。
- 抖动：断开后延迟 N 秒再恢复（避免重连抖动）；连续远程会话合并。
- 不变量：任何路径（含 panic、更新重启、注销、显示器热插拔）都不得让窗口永久留在不可见区域。

### 5. 检测子系统（决定产品成败）

- 信号源：OS 会话标志（`SM_REMOTESESSION` / `WTSQuerySessionInformation` / `CGSessionCopyCurrentDictionary`）、进程存在、监听/已建立 TCP 连接、日志尾部正则、窗口类名/标题、虚拟输入设备、屏幕锁定/会话切换事件。
- **规则包**：签名 JSON 规则（工具名、平台、信号组合、权重/置信度、去抖、默认目标屏），可独立于客户端热更新 + 灰度。给出 DSL schema 草案与几个真实工具的规则示例（TeamViewer / AnyDesk / RustDesk / Parsec / Sunshine / NoMachine / Windows RDP / macOS Screen Sharing），并明确标注哪些字段是 `[待验证]`。
- 误报代价高（用户在本地正常工作时窗口被搬走）→ 置信度阈值、双信号确认、冷却期、"干跑模式"（只记录不动作）用于规则调试。
- 降级路径：托盘一键手动触发、全局热键、按进程白名单"该进程连接时总是触发"。
- 逐工具给出检测可靠性评级（高/中/低）与理由。

### 6. 平台差异与不可行边界

- macOS：TCC Accessibility 授权与重装/换签名后的失效；原生 fullscreen 在独立 Space 不可迁移；`AXUIElementSetAttributeValue` 被应用拒绝；`Displays have separate Spaces`；`CGVirtualDisplay` 的私有 API 风险；SMAppService 开机自启与特权 daemon。
- Linux：X11 可行；**Wayland 无通用跨窗口控制** → 明确降级策略（KWin scripting / GNOME Shell extension 适配器、仅通知、仅会话型场景），不要承诺做不到的事。
- Windows：Session 0 隔离、UAC、多用户快速切换、RDP 会话与 console 会话的差异、Windows 11 圆角/贴靠布局（Snap Layouts）与 `WM_GETMINMAXINFO`。
- 每个平台给出"支持矩阵"（能力 × 平台 × 状态：支持/部分/不支持）。

### 7. 特权 helper 与安全

- 是否需要、何时需要；三平台的服务/守护进程形态与安装方式；IPC 协议；调用方鉴权；最小权限；卸载清理；企业环境（无管理员权限时降级运行）。
- 安全威胁模型：本地提权、恶意进程伪造 IPC、更新包投毒、规则包投毒；对应缓解措施。

### 8. 更新与分发

- 无感更新：下载 → 校验签名 → 静默安装 → 重启；**重启前必须先恢复窗口**；更新失败回滚；灰度与 kill switch；崩溃循环保护。
- 各平台安装包形态与签名要求、CI/CD（GitHub Actions 矩阵 + 公证 + 签名 + 发布）、渠道（官网、winget、Homebrew Cask、Flathub 可行性、Microsoft Store 可行性）。
- 企业批量部署（MSI/PKG/deb、静默安装参数、组策略/MDM、配置下发）。

### 9. 商业化

- 免费版 / Pro 的功能切分（建议：自动检测与基础迁移免费，虚拟显示器/布局模板/多远程并发/企业策略为 Pro）。
- 定价与授权模型（订阅 vs 买断+更新期）、License 服务与离线宽限、设备数/席位、退款与税务（Paddle/Lemon Squeezy 作为 MoR）。
- 试用、转化路径、官网landing 要点、支持成本控制（诊断包、日志脱敏）、隐私政策（明确不采集屏幕内容）。
- 竞品：DisplayFusion、MaxTo、AquaSnap、BetterDisplay、Parsec VDD、Virtual Display Driver、Sunshine/Moonlight、Duet/Spacedesk、各远程软件自带的"切换显示器"。给出定位矩阵与"为什么是我们"。

### 10. 质量与测试

- 平台抽象层 + Fake 实现，使状态机/布局/恢复逻辑可无头单测。
- 集成测试：Windows 测试宿主进程造窗口；macOS 用 `CGVirtualDisplay` 造多屏；Linux Xvfb + Xinerama。
- CI 矩阵与不可自动化部分的人工 smoke checklist。
- 指标：迁移成功率、恢复成功率、误报率、p95 延迟、内存/CPU 空闲占用（目标：空闲 < 1% CPU、< 120MB RSS）、崩溃率。
- 可观测性：本地结构化日志 + 诊断导出（脱敏）、可选匿名遥测事件表。

## 交付要求

1. **PRD**：人物画像、JTBD、端到端用户旅程（首次安装/授权向导 → 日常零打扰 → 远程中 → 断开恢复 → 卸载）、完整功能清单（P0/P1/P2）、每个 P0 功能的验收标准（可测试的 Given/When/Then）、设置界面信息架构与关键文案（中文）、托盘状态机、空/错/权限缺失状态、非目标（明确不做什么）。
2. **技术设计**：模块边界与目录结构、核心状态机（Idle / Armed / Detecting / Migrating / Active / Restoring / Degraded / Error）与转换条件、数据结构（Rust 类型草案）、规则包 JSON Schema、窗口迁移与恢复算法伪代码、三平台 API 清单（真实存在的 API 名称，不确定就标 `[待验证]`，禁止编造）、性能与资源预算、日志与遥测 schema。
3. **路线图**：M0 技术验证（2 周，验证三平台窗口控制 + 检测可行性）→ M1 内测 MVP → M2 公测 → M3 商业化发布 → M4 虚拟显示器/企业版。每个里程碑给出范围、退出标准、风险。
4. **决策记录**：至少覆盖——框架选型、是否 v1 引入虚拟显示器、helper 是否 v1、Wayland 策略、检测优先级（规则包 vs 内置）、更新机制、授权模型、免费/付费边界。
5. **风险登记册**：概率/影响/缓解/触发条件/负责人角色。

## 写作与事实纪律

- 简体中文，工程化表达，表格优先，禁止营销话术和空话。
- 任何你无法确认的 API、注册表路径、日志路径、参数名，必须标 `[待验证]`，并给出验证方法。**绝对不要编造。**
- 不要输出代码实现（可以有类型草案、伪代码、schema、目录树）。
- 对"做不到"的部分要明确说做不到，并给出降级方案；这比画出完美蓝图更有价值。
- 假设读者是资深工程师和产品负责人，不需要科普。
