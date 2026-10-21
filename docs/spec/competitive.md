# Conflux 竞品与替代方案分析

版本：规格基线 0.1；核验日期：2026-10-21。本文只把官方产品文档、官方支持页与项目维护者仓库作为事实依据；能力差异是基于这些资料的产品判断，**不是已经跑过的横向测试结果**。Conflux 列中的内容均为待实现/待验收的设计目标，商业价格与范围以 [roadmap.md](roadmap.md) 为准。

## 1. 比较口径与市场边界

核心任务是：已有多屏主机、单屏远程客户端，在获授权的同一用户会话内，让需要的窗口进入用户选择的可见区域，远程结束后按用户改动与当前拓扑安全恢复。另一任务是在 RDP/xrdp/虚拟桌面会话中救援落在不存在显示器坐标上的窗口。它们不能用“多屏支持”一个标签代替。

| 维度 | 本文判断的内容 | 不能从官方页面推导出的结论 |
|---|---|---|
| 捕获选择 | 客户端能否切换显示器、并排/多窗口查看，主机能否指定流来源 | 客户端有切屏按钮，不等于第三方应用能实时查询当前捕获屏 |
| 窗口控制 | 分区、热键搬窗、保存/载入位置、自动化触发 | 有窗口布局不等于能识别入站远程、自动恢复用户手动修改后的状态 |
| 恢复闭环 | 开始前持久快照、断开去抖、冲突保留、崩溃/热插拔/更新恢复、错误可见 | 没有在引用页面描述，不等于竞品“绝对做不到”；未验证部分标注 `[待验证]` |
| 显示拓扑 | 物理显示器配置、虚拟屏、分辨率自适应、独立会话 | 有虚拟屏不等于已有窗口可以同时留在物理屏和虚拟屏，也不等于本机桌面完全不受影响 |
| 商业成本 | 许可结构、配置/维护成本、用户是否已经购买 | 不用不同币种、促销、席位或年度/买断价格做无上下文“更便宜”比较 |

本轮无任何竞品被判定为“不具备崩溃恢复”，除非有直接证据；没有资料的条目统一为“本文未验证”，在第 6 节指定实测方法。工具类别不是固定拓扑：例如 NoMachine 或 VNC 的物理桌面与虚拟会话模式应分别评估。

## 2. 定位矩阵

| 产品 / 替代方案 | 已核验的产品中心与平台信息 | 与目标任务重叠的现有能力 | 对 Conflux 的竞争/合作意义 | 官方证据与待验证项 |
|---|---|---|---|---|
| **DisplayFusion** | Windows 多显示器与窗口管理 | Window Position Profiles、Monitor Profiles、Triggers、脚本/函数；可由窗口或进程等事件驱动移动与布局 | 最接近“用户自行组合自动化”的替代方案；不能把它描写成只有壁纸或手动搬窗。已有用户可能只需一套可靠配置，无需新产品 | [位置配置说明](https://www.displayfusion.com/HelpGuide/WindowPositionProfiles/)、[Triggers](https://www.displayfusion.com/Features/Triggers/)、[完整触发事件](https://www.displayfusion.com/HelpGuide/Triggers/)。远程开始/结束识别、journal/用户冲突语义 `[待验证]`：按第 6 节搭建官方能力组合实测 |
| **MaxTo** | 区域划分、窗口管理；官网在核验日已宣传 Windows 和 Mac 版本，不能沿用“仅 Windows”的旧判断 | 屏幕区域、预设与多显示器管理；官网介绍按应用记忆位置、将新窗口放回及恢复布局，同一许可覆盖 Mac/Windows | 日常布局和记忆恢复与 Pro 模板直接重叠；跨平台本身不足以形成独特优势 | [MaxTo 官网/功能与平台](https://maxto.net/)。Mac 与 Windows 功能/权限等价性、远程检测及恢复闭环 `[待验证]`：安装核验日版本逐场景对照，不以官网概述代替测试 |
| **AquaSnap** | Windows 的 docking、tiling、snapping、stretching 等窗口交互 | 手动分区、联动调整、快捷键与多屏相关功能 | 对只需要把几个窗口手动排好的用户，简单交互可能比持续检测更合适；Conflux 不与它争夺全部日常窗口手势 | [AquaSnap 功能](https://www.nurgo-software.com/products/aquasnap)、[版本/授权](https://www.nurgo-software.com/pricing/aquasnap)。自动远程事务与断连恢复 `[待验证]`：配置/实测验证，不能写成已证实缺失 |
| **BetterDisplay** | macOS 显示管理；包括 HiDPI、虚拟屏、显示配置、PIP/streaming、断开/重连及集成能力 | 已有虚拟屏和显示配置能力，可与远程工具配合；提供 CLI/集成入口 | 对需要虚拟 Mac 屏幕或分辨率调整者是强替代/可能集成伙伴；Conflux 不应以“macOS 虚拟屏首次出现”为卖点 | [维护者仓库](https://github.com/waydabber/BetterDisplay)、[集成文档](https://github.com/waydabber/BetterDisplay/wiki/Integration-features%2C-CLI)。联合使用时捕获源绑定与应用窗口冲突恢复 `[待验证]`：按具体版本、签名身份及远程工具测试；不默认打包或重签第三方软件 |
| **Parsec VDD / Parsec 虚拟显示** | Parsec 自有远程链路的虚拟屏能力；官方说明 Windows 需 VDD，Mac host 的虚拟显示不需同一 Windows 驱动 | Warp/Teams 条件下可在符合条件的 Parsec 连接中增加虚拟屏；Windows VDD 文档明确其不是任意持久虚拟屏；还存在受条件限制的免费无屏 fallback | 已付费 Parsec 用户可能已经满足需求，Conflux 不应重复收费承诺同一体验；多工具混用和原位恢复策略才是待证明的增量 | [VDD 条件与安装](https://support.parsec.app/hc/en-us/articles/32381178803604-VDD-Overview-Prerequisites-and-Installation)、[fallback/权限条件](https://support.parsec.app/hc/en-us/articles/32361388811284-VDD-Troubleshooting)。第三方工具可复用范围 `[待验证]`：查许可并实测，不宣称该驱动通用可嵌入 |
| **Virtual Display Driver** | 本文特指 **VirtualDrivers/Virtual-Display-Driver** 维护者项目，不泛指所有同名驱动；项目用于 Windows 虚拟显示器 | 虚拟监视器、可配置分辨率/刷新率等，面向 streaming/headless 场景 | 对愿意维护驱动和显示设置的用户是可组合的替代组件；Conflux M4 的增量应是可信分发、恢复、工具绑定、支持与企业管理 | [项目与许可证](https://github.com/VirtualDrivers/Virtual-Display-Driver)。具体发行二进制的签名、重分发许可、目标 OS/架构/Secure Boot 条件 `[待验证]`：锁定 release 检查签名与许可、零售系统测试；仓库 MIT 不自动证明所有附带二进制都可重分发 |
| **Sunshine / Moonlight** | Sunshine 为 Moonlight 提供自托管 streaming host；Moonlight 提供客户端 | Sunshine 官方配置包括显示来源选择和前后准备动作，文档也有切换流显示器的操作；可由熟练用户组成自动化方案 | 如果用户愿意统一远程链路并维护配置，可能不需要 Conflux；同时是较适合探索显式绑定/生命周期集成的候选。不要把成熟自托管方案当作仅游戏工具 | [Moonlight 官方介绍](https://moonlight-stream.org/)、[Sunshine 配置](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)、[Sunshine 使用说明](https://docs.lizardbyte.dev/projects/sunshine/master/md_docs_2getting__started.html)。Conflux 只读检测/集成的稳定版本契约 `[待验证]`：锁定发布版本；不能因某版本有 hook 就称所有安装都已自动兼容 |
| **Duet** | 设备协作、远程桌面、扩展/镜像显示，覆盖其官方列出的多种设备 | 远程访问，以及将其他设备变为显示器；有镜像/扩展模式选择 | 是可能更换用户工作流的完整替代，不能仅归为 iPad 副屏软件；适合“增加可用屏幕”而非“把已有窗口收拢”偏好的用户 | [Duet 产品](https://www.duetdisplay.com/)、[镜像/扩展说明](https://www.duetdisplay.com/help-center/duet-is-set-to-mirror-and-i-want-to-switch-to-extend-desktop)。各 OS/套餐远控支持矩阵和恢复语义 `[待验证]`：按购买地区/套餐与主从设备组合验证 |
| **spacedesk** | 网络/USB 显示扩展体系，主机 Driver 与 Viewer 分离；官方手册包含 Windows 主机，并新增 Mac 主机章节 | 将其他设备作扩展/镜像显示器，能改变主机显示拓扑 | 对局域网增加屏幕/复用设备是替代；对已有远程工具的窗口闭环属于组合组件，不能由“虚拟显示”推导完整恢复能力 | [官方手册](https://manual.spacedesk.net/)、[显示设置说明](https://manual.spacedesk.net/Various.html)、[下载与用途](https://www.spacedesk.net/download/)。Mac 主机版本成熟度、各连接方式和恢复行为 `[待验证]`：按手册对应发行版本实测，不默认与 Windows 等价 |
| **远程工具自带显示器切换/多窗口** | TeamViewer、AnyDesk、RustDesk、ToDesk、向日葵、NoMachine、RealVNC 等均有不同形式的多屏相关 UI | 保留原主机布局，通过选择/缩放/多窗口访问远端屏幕；某些产品已附带虚拟屏或分辨率恢复 | **最重要的默认替代**：无需安装新工具、不引入自动搬窗。偶尔远程或希望物理桌面保持不变的用户，应优先考虑这些现成功能 | 逐工具证据见第 3 节。是否有外部查询 API、当前捕获源可见性均须另验证 |
| **RDP / xrdp / 会话型 VNC** | 会话拓扑由远程登录与客户端显示配置决定，不应强行按镜像捕获逻辑解释 | 会话单屏/多屏、动态分辨率等因实现而异；许多窗口天然已在会话内 | 核心增量是越界救援及恢复，而非“把 console 三屏都搬到 RDP”；不提供跨用户/跨会话移动窗口 | [Microsoft RDP 显示属性](https://learn.microsoft.com/en-us/azure/virtual-desktop/rdp-properties)、[RealVNC Virtual Mode 说明](https://help.realvnc.com/hc/en-us/articles/360002321317-What-control-do-I-have-over-monitors-and-resolution)。xrdp/其他 VNC 实现细节 `[待验证]`：锁定服务端与桌面会话测试 |

## 3. 远程工具自带切屏与会话能力

下表证明“已有可选方案”，不证明 Conflux 能检测连接或读取当前捕获屏。产品有官方 API/参数才按该契约集成；仅有客户端按钮时提供用户操作引导，不做未经验证的隐藏参数、注册表或日志路径假设。

| 工具 | 官方确认的替代能力 | 对单屏用户的实际取舍（产品推断） | 第一手来源 / Conflux 侧待验证 |
|---|---|---|---|
| TeamViewer | 官方 Classic 文档描述切换指定屏、显示所有屏、分标签/独立会话窗口 | 切换可保留本机原布局；在小屏展示所有屏的可读性须按分辨率验证 | [多显示器支持](https://www.teamviewer.com/en-us/global/support/knowledge-base/teamviewer-classic/remote-control/in-session-features/use-multi-monitor-support/)。该证据明确针对 Classic；当前其他产品线 UI 与能力 `[待验证]` |
| AnyDesk | 工具栏选择显示器；官方说明多窗口/多显示器显示方式，且分辨率自适应可以在会话结束后恢复 | 原生视图已经覆盖一部分“看不见窗口”的问题；分辨率恢复不是按用户改动恢复每个应用窗口 | [多显示器操作](https://support.anydesk.com/how-do-i-work-with-multiple-displays)、[显示设置](https://support.anydesk.com/display)。外部捕获源查询与自动入站检测 `[待验证]` |
| RustDesk | 官方高级设置列出显示器工具栏；维护者发布记录含多显示器捕获相关改进 | 可以先评估原生显示器 UI；开源便于验证实现，但不等于承诺稳定的第三方 IPC | [显示相关设置](https://rustdesk.com/docs/en/self-host/client-configuration/advanced-settings/)、[维护者发布记录](https://github.com/rustdesk/rustdesk/releases)。各平台/版本切屏与虚拟屏组合 `[待验证]`：以发行包实测 |
| Parsec | 官方虚拟显示文档与 Windows 应用说明描述虚拟屏、Privacy Mode、fallback、连接断开时的部分显示设置恢复 | 现有 Parsec 用户先验证自带能力；Privacy Mode 会改变物理显示器状态，不能作为“本机完全不受影响”的证据 | [VDD](https://support.parsec.app/hc/en-us/articles/32381178803604-VDD-Overview-Prerequisites-and-Installation)、[Windows 应用说明](https://support.parsec.app/hc/en-us/articles/32381199341716-Parsec-App-for-Windows)。同一窗口用户改动冲突恢复 `[待验证]` |
| ToDesk | 官方 FAQ 描述按屏幕序号切换、移动端切换以及购买多屏插件后同时展示双屏 | 单次切屏可能已足够；插件与套餐可与 Conflux 一并评估，而不是假定只能人工在主机拖窗 | [官方“被控双屏怎么切换”](https://todesk.com/faq/26.html)。当前插件/系统版本与外部可检测信号 `[待验证]`：官方客户端实测 |
| 向日葵 | 官方帮助说明 iOS/Android 主控可切换多屏 Windows 主机；官网还宣传双屏控双屏与隐私屏 | 官方现有能力需要按主控/被控平台组合测试；不能用移动端教程概括全部桌面端 | [多屏切换帮助](https://sunlogin.oray.com/news/1410.html)、[当前官网能力](https://sunlogin.oray.com/)。现有桌面端 UI、套餐和可检测入站信号 `[待验证]` |
| NoMachine | 官方说明可选择一屏或同时看全部，并有快捷键；Linux 企业虚拟桌面模式具有不同拓扑语义 | 物理桌面模式切屏是直接替代；虚拟桌面模式应转成会话内救援测试 | [切换显示器](https://www.nomachine.com/support/documents/switching-the-view-between-multi-monitors-during-a-remote-desktop-session)、[多显示器模式](https://kb.nomachine.com/AR06U01201)。Conflux 不跨物理/虚拟会话复制窗口 |
| VNC（以 RealVNC 为明确样本） | RealVNC 桌面 Viewer 可选择一屏/全部，多窗口模式有版本前提；官方另提供部分 Windows 版本的虚拟屏 | “VNC”是家族，不是一种统一 UI 或授权模式；不要向所有 VNC 用户承诺相同支持 | [多屏支持](https://help.realvnc.com/hc/en-us/articles/360006483577-Working-with-multiple-monitors-in-RealVNC-Connect)、[虚拟屏](https://help.realvnc.com/hc/en-us/articles/31739688741405-Adding-Virtual-Displays)。TigerVNC/其他发行版的镜像与登录模式 `[待验证]` |
| Windows RDP | 官方支持显示数量、指定客户端显示器、窗口/全屏与动态分辨率等属性 | 它配置远程会话使用的客户端显示拓扑，不是直接浏览 console 每块物理屏；不承诺迁移另一个会话中的应用 | [RDP 支持属性](https://learn.microsoft.com/en-us/azure/virtual-desktop/rdp-properties)。实际 OS、客户端、会话恢复与 persistent rect 行为须在支持矩阵逐项验证 |
| macOS Screen Sharing | Apple 官方说明被共享 Mac 有多个显示器时可选择查看哪一个 | 既有切屏可避免额外窗口重排；Conflux 的增量仍需在 AX 权限与 Space 边界内证明 | [Apple 屏幕共享](https://support.apple.com/en-sg/guide/mac-help/mh14066/mac)。普通模式与其他共享模式的检测信号、session 标识 `[待验证]` |
| Sunshine / Moonlight | 官方文档给出显示来源选择及切换串流显示器的操作 | 用户可固定捕获源配合手动/配置化布局；这是比“猜当前捕获源”更确定的部署方式 | [配置](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)、[使用说明](https://docs.lizardbyte.dev/projects/sunshine/master/md_docs_2getting__started.html)。新旧配置字段/平台值有差异，集成参数一律按固定版本验证后才进入规则 |

第 3 节所引用页面没有形成“读取所有远控工具当前捕获屏”的统一协议。**本规格不将未找到该协议等同于证明永远不存在**；实现时若工具提供正式接口，可通过单独适配器与测试提升等级。在此之前，目标选择显示“配置目标”或“推测目标”，不能写“已检测到远端正在看此屏”。

## 4. 商业与采用成本比较

本轮价格不做促销数字排行。竞争评估需要同时考虑买断/订阅、使用人数、设备数、商业使用条款、升级覆盖、用户已经付出的费用与配置成本。以下授权结构有官方来源；实际采购金额和地区税额以购买时结账为准，不把本文作为报价单。

| 方案 | 已核验的收费/许可结构 | 对 Conflux 定价与采用的启示 |
|---|---|---|
| DisplayFusion | 官方 Pro 页面列有单机、个人多机、Site、Enterprise 等许可，并说明长期版本权益。[购买说明](https://www.displayfusion.com/Purchase/Purchase/Personal/More) | 通用窗口工具存在长期授权参照。Conflux 年订阅必须通过持续规则适配与支持证明价值，不能仅靠一套静态布局收费 |
| MaxTo | 核验日官网同时列年付、一次性与企业方案，且区分个人/受管工作设备。[官网方案](https://maxto.net/) | 一次购买与年费是有效对照；设备数、工作使用范围和未来升级权必须在 Conflux 结账前说明 |
| AquaSnap | 官方区分免费个人使用与 Professional；标准许可描述为单电脑终身授权并可换机转移。[版本与价格](https://www.nurgo-software.com/pricing/aquasnap) | 免费/商业边界有市场先例，但 Conflux 不因此把安全恢复收费；换设备自助迁移应避免客服负担 |
| BetterDisplay | 官方条款描述一次购买覆盖相应主要版本且至少一年更新，不是订阅；商用与非商用存在许可条件。[许可条款](https://betterdisplay.pro/license/) | 买断+更新期有可理解的先例；也说明必须精确区分“继续运行已许可版本”与“取得所有未来版本” |
| Parsec 虚拟显示 | 虚拟屏条件依赖 Warp/Teams 与连接身份；免费 fallback 条件单列。[VDD](https://support.parsec.app/hc/en-us/articles/32381178803604-VDD-Overview-Prerequisites-and-Installation)、[fallback](https://support.parsec.app/hc/en-us/articles/32361388811284-VDD-Troubleshooting) | 已付费用户的边际成本可能为零；不能用一个额外虚拟屏承诺说服所有 Parsec 用户重复付费 |
| Virtual Display Driver | 指定维护者项目公开源代码与许可证；实际维护/签名/支持不是免费成本。[项目](https://github.com/VirtualDrivers/Virtual-Display-Driver) | Conflux 不能把开源组件本身描述为独占技术；商业价值要落在验证、恢复、部署与长期维护 |
| Sunshine / Moonlight | 官方定位开源自托管 host/client 方案。[Moonlight](https://moonlight-stream.org/)、[Sunshine 文档](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html) | 现金价格之外还要测量配置/维护时间；熟练用户可接受自建，不能用“需要配置”当作必然缺点 |
| Duet | 官方包含多个工作流；本轮未完成各套餐在各地区的同口径结账核验。[官方产品](https://www.duetdisplay.com/) | 精确当前价格/权益 `[待验证]`：由商业负责人记录主控/被控平台、套餐、币种、税额与日期后比较；不得沿用历史价格 |
| spacedesk | 官方区分非商业私人许可与商业许可，并区分现行/实验/旧 Viewer；商业使用不能简单概括为全部免费。[许可说明](https://www.spacedesk.net/licensing/) | 比较必须落到实际 Viewer/商店与用途，不能把“免费下载”作为企业使用成本为零的证据 |
| 自带切屏 / 系统会话 | 能力与套餐/平台有关；已安装的远程工具常是用户首先会尝试的办法 | 用户没有增加自动搬窗工具的义务。获客文案应先帮助其判断是否真的有重复切屏与恢复成本 |

Conflux 首发建议 US$29/年、3 台个人主机，免费保留完整安全底座；US$59 买断+12 个月功能更新仅作为后续需求试验。它是定价假设，不代表已经比任何上述产品更有价值。用户每月远程次数、手动切屏次数、找窗/恢复耗时与付费意愿须共同验证，不能直接把减少点击折算为可实现收入。

## 5. “为什么选择 Conflux”的可检验表述

| 待证明差异 | 具体产品机制 | 验证指标 / 边界 | 对应竞争响应 |
|---|---|---|---|
| 跨远程工具的一致窗口工作流 | 工具适配器输出统一证据；同一快照/布局/恢复引擎；切换工具仍保留操作习惯 | 只列已验工具/版本；低置信度提示/手动；不保证自动获得捕获源 | 对仅使用一种已满足需求远程工具的用户，明确承认原生功能可能足够 |
| 可理解的自动化 | 展示“为何触发”、目标屏来源、dry-run、暂停/恢复与安全降级 | 自动动作误报、人工修正率、误搬/主机小时；不是不可解释的进程白名单搬窗 | 与 DisplayFusion/脚本组合比较实际配置步骤与维护时间，不只比较功能勾选 |
| 恢复是核心契约 | 持久 journal、会话隔离、手动改动冲突保留、热插拔安全替代、更新前恢复 | 恢复 ≥99.9% 的合格窗口安全终态；精确原位/保留用户修改/救援分别统计；运行中失败不得剔除 | 不无证据断言竞品恢复不可靠；用同一故障注入工作负载测量 |
| 无额外管理员组件的 v1 | 普通用户窗口控制；权限不够时解释并跳过 | 提权/受保护窗口不承诺；macOS 用户完成 Accessibility 授权；Wayland 没有通用控制 | 比较真实安装摩擦，不把“不安装 helper”宣传成能控制所有窗口 |
| 许可与安全分离 | 免费检测、基础迁移/布局、恢复与救援；授权失败不锁死桌面 | 离线/到期/退款/卸载场景的安全终态测试 | Pro 收费来自可交付高级模板/规则/仲裁；不得人为制造恢复风险促使升级 |
| 企业可运维性（M4） | 策略来源可见、离线授权、受管更新、审计、独立驱动包 | 设计伙伴 30 天、部署升级卸载、权限与驱动审查通过 | 与已有远控套件企业功能组合比较，避免堆建无需求的管理控制台 |

建议对外定位文案（经实际支持矩阵约束后使用）：**“在被控电脑上，把选定窗口整理到远程可见的目标屏；结束后按你的改动安全恢复。”** 紧邻展示“目标屏默认主屏，可手动切换；自动检测以兼容性列表为准”。不使用“全平台自动”“不会改变本机工作”“任何窗口都能移动”等绝对承诺。

### 5.1 目标客户与应优先使用替代方案的人群

| 用户需求 | 建议的评估起点 | Conflux 的进入条件 |
|---|---|---|
| 经常在多屏主机与单屏客户端之间切换，混用多种远控软件 | Conflux Free + 已有远控工具 | 真实减少找窗/布局恢复耗时，并且误搬可接受；不更换传输链路 |
| 偶尔远控，切一两次屏即可 | 远程工具原生切屏/显示全部 | 只有其反复找窗问题经测试仍存在时再采用自动化 |
| 主要痛点是日常分区/窗口手势 | MaxTo、AquaSnap、DisplayFusion | 不把非远程布局需求硬拉进检测产品；Conflux 模板应与已有管理器避免同时抢写 |
| macOS HiDPI、亮度、虚拟显示、PIP | BetterDisplay 与现有远控能力 | Conflux 只解决窗口事务，不重复承诺显示色彩/亮度工具的完整能力 |
| 已使用 Parsec/Warp/Teams，虚拟屏足够 | 优先验证 Parsec 自带 VDD/虚拟显示 | 若需要多工具统一/特殊恢复语义，再测 Conflux 增量 |
| 希望物理桌面完全不变且远程有另一个独立桌面 | 独立远程登录会话/虚拟工作站等体系 | 不承诺复制同一个真实窗口到两套独立桌面；Conflux 仅在其会话内救援 |
| 愿意维护显示驱动、自托管与脚本 | Virtual Display Driver + Sunshine/Moonlight 等明确组合 | 只有配置维护和恢复支持确有成本，才有专业服务/Pro 价值 |
| Wayland 桌面，需要任意应用跨屏自动搬移 | 先评估 compositor 原生功能或专用适配器 | v1 仅检测/通知，不以付费版承诺平台做不到的事 |

## 6. 竞争验证计划与证据缺口

M0 验证技术可行性，M2 才做有条件一致的横向测试。测试使用本来就允许安装/远控的实验账户与设备，不抓取生产屏幕或敏感文档。厂商条款、试用/商业授权须由测试负责人核实；不绕过授权来完成比较。

### 6.1 统一试验台

| 场景 | 固定条件 | 比较观测 | 合格性与记录要求 |
|---|---|---|---|
| 三屏 → 单屏 → 三屏 | 主机三屏含混合 DPI/负坐标；单屏客户端；同一组 20 个普通窗口；固定输入任务 | 首次配置步骤/时间、开始操作时间、切屏次数、窗口可访问率、恢复耗时 | 每方案至少 10 名目标用户或受控 30 次循环；区分熟练与初学用户，不以作者熟悉度作为结果 |
| 远程期间手动改动 | 用户拖动/缩放/最大化/新建/关闭各若干窗口 | 哪些窗口恢复、哪些保留，用户能否解释结果 | 预先声明期望；不能把“全都强制放回”默认计成恢复成功 |
| 断网与重连 | 短断、长断、连续连接、第二工具连接、切换捕获屏 | 重排次数、重复快照、目标振荡、人工干预 | 各方案使用其正常支持配置；自带切屏无需搬窗也可能更符合用户偏好 |
| 故障与拓扑 | 迁移中杀进程、重启、显示器拔插、目标消失、更新替换 | 恢复入口、可见窗口比例、原位与救援比例、支持需求 | 保留故障原始计数；没支持某场景标未覆盖，不杜撰失败 |
| session 救援 | RDP / xrdp / 一种会话型 VNC，当前会话存在越界持久坐标 | 救援成功、错误会话写入、对当前布局干扰 | 与 mirror 分开报告；禁止将不同会话窗口搬来制造“覆盖率” |
| 多管理器共存 | Conflux 与用户选择的 DisplayFusion/MaxTo/AquaSnap 等分别共存 | 反复移动、自我纠正、热键冲突、CPU、用户排除能力 | 默认单一窗口管理拥有者；冲突时暂停规则，而不是无限重试 |
| 购买与运维 | 同地区、同用户数/主机数、含税价格；首次安装/激活/取消/换机 | 总成本、权限摩擦、自助支持成功率 | 标注版本/日期/税/促销；结果与产品收费假设分开，不用抓到的过期折扣作结论 |

### 6.2 待验证登记

| ID | 待验证问题 | 方法 | 截止 / Owner | 未得到答案时的决定 |
|---|---|---|---|---|
| C01 | DisplayFusion 组合能否满足同等远程检测与恢复语义 `[待验证]` | 使用官方 Triggers/Profiles 配置、回放/故障测试；必要时向厂商确认正式扩展点 | M2 / Windows 负责人 | 不宣称其做不到；定位侧重可用性、已验证闭环与跨工具支持 |
| C02 | MaxTo Mac/Windows 功能与权限行为等价性 `[待验证]` | 记录当前发行版与许可，在两个平台跑相同任务 | M2 / 产品+macOS | 不以“只有我们跨平台”销售 |
| C03 | BetterDisplay/第三方虚拟屏与 Conflux 并用时的窗口和显示恢复 `[待验证]` | 用户安装的正版工具做明确目标屏实验；检查现有虚拟屏不被卸载/误删 | M4 / macOS+驱动负责人 | 不自动绑定/接管第三方显示，不打包重签其软件 |
| C04 | 远控工具是否有正式当前捕获源/生命周期接口 `[待验证]` | 官方 API/SDK 文档、厂商书面答复、版本化集成 POC；源码存在内部函数不算公开契约 | M0 首轮、每次规则更新 / 检测负责人 | 用户指定屏与手动触发；未知版本提示 |
| C05 | 精确日志路径、注册表路径、进程名、CLI/安装参数 `[待验证]` | 对每个工具 OS/版本/安装形态在干净机器查官方文档并观测真实产物；脱敏 fixture 留证 | 对应规则发布前 / 平台负责人 | 不进生产默认规则；本比较不臆造路径或参数 |
| C06 | 虚拟驱动二进制商业再分发/签名/升级责任 `[待验证]` | 锁定 release 的 LICENSE、SBOM、签名链、厂商许可与目标 OS 实测 | M4 立项 / 法务+驱动负责人 | 只提供用户自行安装的兼容性指引或自研；不预售集成能力 |
| C07 | 用户实际愿付多少、是否比原生切屏节省时间 `[待验证]` | ≥30 名目标用户访谈，M2 对照任务，M3 cohort 转化与退款分析 | M2–M3 / 产品商业 | 保留 Free、调整价值与价格，不制造免费版缺陷促转化 |
| C08 | 竞品当前精确总价、地区商用权益 `[待验证]` | 官网/应用内实际结账前核验方案、币种、税、设备数、升级权；不实际付款即可记录报价 | 每次采购或对外价格比较前 / 商业负责人 | 不发布定量“最便宜”结论 |

## 7. 竞争策略的停止条件

如果原生远控或现成窗口工具组合在目标用户测试中已经同样方便、恢复足够可靠且配置成本低，Conflux 应收窄到已有工具未满足的明确任务，或调整为集成/企业支持服务；不能用“还有更多平台”掩盖无需求。M2 的关键决策证据是任务完成时间、错误与信任、恢复正确性、持续使用和付费意愿，而不是功能表中勾选数量。

若某主要工具新增稳定的原生窗口收拢/恢复能力，应更新兼容性与比较页，评估集成而非自动与其抢写窗口。若 M0 不能建立任何可复现的镜像自动检测，必须把“自动远程窗口整理”视为未成立假设；按路线图保留手动/提示产品边界，并重新验证商业需求。
