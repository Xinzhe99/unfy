# 当前实现审查记录

审查日期：2026-10-25。范围：布局/Windows 控制、事务/journal、检测/调度、设置/自启/更新、macOS/Linux 能力、真实 UI 与测试/发布证据。实施要求见同目录 01–06；本文件记录问题事实，不宣称已修复。

## 证据口径

- **现场**：打开已运行的 `unfy.exe`，实际查看四页和滚动，截图检查；运行现有 `target/debug/probe.exe` 的无参数只读枚举。没有执行整理/恢复、切换布局、改变用户设置或运行全套测试。
- **源码**：直接读取当前工作区实现；四个只读分工分别审查布局/Windows、恢复、检测/交付、跨平台，再由主审交叉核对核心缺陷。不是仅复述旧 spec。
- **静态推导**：例如 mac 条件编译类型错误、句柄复用后果、混合 DPI 风险，未冒充对应机器上实测。
- **用户事实**：默认整理效果不正确、很多窗口无法整理、流程不顺畅。无需再次搬动用户桌面才能承认这些问题。下表给出可证实的原因，不声称涵盖每个应用的全部失败原因。

现场记录：两块 1920×1080、100% 显示器，工作区各 1920×1032。只读 probe 报告总 HWND 521、visible 48、其自有过滤后的 candidates 6；这些是不同集合，不是“521 个用户窗口只支持 6 个”的成功率。probe 的 owner 过滤与产品不同，且包括 Unfy 自身，不能当作引擎覆盖率。使用现存二进制，未在本次重构建。

真实概览同时出现「就绪」「已收拢窗口 6」「还有 3 个窗口没能恢复原位」，只有整理/预览等入口。用户当前设置显示主次布局，不能把本次截图当成默认布局失败复现。源码中的预览重置状态能直接解释此矛盾。四页的截图含显示器画面，本次不把这些私有桌面图片写入仓库；证据保留为上述文字与本会话工具记录。

严重度：P0=可能丢恢复责任、错误操作对象、错误自动动作或发行阻断；P1=核心可用性/一致性问题；P2=清晰度、资源与维护性。P0/P1 均需要关闭，不能仅修视觉后宣布可用。

## 阻断与缺陷索引

| ID | 优先级/证据 | 问题与触发 | 代码定位 | 实施落点 |
|---|---|---|---|---|
| A01 | P0，现场+源码 | Active/Degraded/Error 打开预览后被改为 Idle；待恢复和入口消失 | `engine/src/lib.rs:545–569`；`commands.rs:269–293,1203–1236`；`Overview.tsx:108–126` | [02](02-session-and-recovery.md)、[04](04-ui-ux.md) |
| A02 | P0，源码 | 一窗快照失败、另一窗成功时，失败窗仍可能从原 placements 被提交 | `engine/src/lib.rs:757–818,867–900` | [01](01-gather-and-window-control.md)、02 |
| A03 | P0，源码 | intent/result/fsync/close 错误被忽略；清空前像或提前 take 事务 | `engine/src/lib.rs:873–986,1240–1267,1492–1501` | 02 |
| A04 | P0，源码 | 启动按 mtime 删除其他 journal，不证明已解决 | `engine/src/lib.rs:489–542` | 02 |
| A05 | P0，源码 | 坏 Begin/未来 schema/混合事务等可判 Clean；读查询写 corrupt 副本 | `journal/src/lib.rs:369–436`；`journal/src/record.rs:96–125` | 02 |
| A06 | P0，源码 | 重放不恢复 last_applied/终结状态；部分恢复后崩溃可能再次覆盖已保留改动 | `engine/src/lib.rs:468–478,1232–1257`；`journal/src/lib.rs:146–175` | 02 |
| A07 | P0，源码；复用后果待实测 | Windows 只核验 IsWindow、process_start=0；AX 地址作为持久 ID | `win/src/displays.rs:132–139,297–304`；`mac/src/ax.rs:218–225` | 02、[05](05-platform-settings-release.md) |
| A08 | P0/P1，源码 | retarget 关闭旧事务重建基准，原屏丢失时可能以临时回退位置覆盖原位 | `engine/src/lib.rs:1453–1488` | 02 |
| A09 | P0/P1，源码 | 恢复/救援只看 apply Ok 或仅 rect；mode/topmost/native placement 未完整恢复 | `engine/src/lib.rs:1039–1057,1297–1429` | 01、02 |
| A10 | P0，源码 | 维护检查后仍可开始整理；未读 journal 不在内存计数中；更新检查会安装 | `update.rs:99–197`；`tray.rs:554–588`；`engine/src/lib.rs:1217–1223` | 02、05 |
| A11 | P0，源码 | 目录迁移 fallback 只复制顶层，漏 journal；半成品新目录永久阻止重试 | `settings/src/lib.rs:476–508`；`desktop/lib.rs:217–219` | 05 |
| A12 | P1，源码/数学推导 | 默认按整个多屏外接矩形非等比压缩，目标已有窗口也重排 | `engine/src/lib.rs:607–631`；`layout/src/lib.rs:239–292` | 01 |
| A13 | P1，源码 | 重置保存 KeepLayout/15s，却将 live engine 写 Grid/0s | `settings/src/lib.rs:216–251`；`commands.rs:1733–1759` | 01、05 |
| A14 | P1，源码 | 大无边框窗误判全屏；DWM 失败也形成无边框证据；小分辨率全屏漏判 | `win/src/windows_impl.rs:190–206` | 01 |
| A15 | P1，源码 | owner 信息丢失，模态框独立布局，tool 提前过滤；无法保证整组可达 | `win/src/windows_impl.rs:103–170,210–220` | 01 |
| A16 | P1，源码 | 能力全部 true；忽略 can_resize；最小尺寸被 target clamp 压破；未知小窗被放大 | `windows_impl.rs:224–245`；`engine/src/lib.rs:661–678`；`layout/src/lib.rs:266–274` | 01 |
| A17 | P1，源码 | WinPlatform 未转发 async；默认重试没有等待、可三次写；每窗口重新枚举全桌面 | `win/src/lib.rs:71–102`；`platform/src/lib.rs:109–110`；`engine/src/lib.rs:780–794,1111–1147` | 01 |
| A18 | P1，源码 | 成功只比较 rect，与随后 observed 可矛盾；全部 unplaced 被空计划分支丢弃 | `engine/src/lib.rs:714–726,905–919,1043–1057` | 01、02 |
| A19 | P1，源码；跨环境影响待验 | 虚拟桌面只依赖 cloaked；DPI/约束单位含糊；CCD mode union 匹配不完整 | `windows_impl.rs:110–121`；`displays.rs:162–189,266–280` | 01 |
| A20 | P0，源码 | 进程/任意匹配端口/虚拟屏存在可授权自动移动，无法证明入站控制 | `detect/src/lib.rs:198–234`；`detect/src/platform.rs:112–195` | [03](03-detection-and-target.md) |
| A21 | P0，源码 | 查询错误当空、一次无证据即断开；配置降级也可制造结束 | `detect/src/platform.rs:77–79,141–153`；`detect/src/lib.rs:279–332` | 03 |
| A22 | P0/P1，源码 | 倒计时阻塞观测线程；重连取消受 cooldown 影响；拿锁前后有取消空窗 | `monitor.rs:37–65,198–250,442–488`；`commands.rs:674–706` | 03、02 |
| A23 | P1，源码 | UI/托盘/重启的 pause/deferred_restore 不一致；手动标记混淆整体 active | `tray.rs:424–445`；`commands.rs:1944–2018,2030–2043`；`desktop/lib.rs:138–150` | 03、04 |
| A24 | P1，源码 | 目标丢失默选主屏；普通点选只保存而 cycle 会整理且吞结果 | `engine/src/lib.rs:297–308`；`commands.rs:1140–1166,1780–1858` | 03、04 |
| A25 | P1，源码 | Settings 先改内存后 validate/save；自启错误吞掉，旧 Run 项迁移缺失 | `commands.rs:968–1082`；`session.rs:177–216` | 05 |
| A26 | P1，源码 | 自动路径隐去失败；Message 对象按 string 渲染；倒计时取消指向放弃记录 | `monitor.rs:191–195,254–266,409–416`；`App.tsx:307–439` | 02、03、04 |
| A27 | P1，现场+源码 | 错误主要引导全部排除；排除页误称不恢复；单屏直接推断无待整理窗口 | `Overview.tsx:233–273,367–431`；`Exclusions.tsx:62–65` | 04 |
| A28 | P1，源码 | 新越界窗口没有独立发现路径；拓扑观察与写者抢同锁、不能使旧批次失效 | `session.rs:231–302`；`engine/src/lib.rs:1383–1429` | 01、02 |
| A29 | P0 平台发布，静态 | mac HOME String.join、测试缺字段、AX 跨线程所有权缺口 | `settings/src/lib.rs:438–450`；`mac/tests/geometry.rs:46–63`；`mac/src/ax.rs:101–114` | 05 |
| A30 | P0/P1 mac，源码 | AX 读失败当 Gone；CF 类型/所有权/超时错误；显示缓存不刷新、work area 未接入 | `mac/src/ax.rs:123–168,319–405`；`mac/src/displays.rs:28–113` | 05、02 |
| A31 | P1 mac，源码 | points 与 pixel scale 混用；NSScreen.mainScreen 随焦点；自身窗口未排除 | `mac/src/displays.rs:59–68,154–172`；`mac/src/ax.rs:458–470` | 05 |
| A32 | P1，源码 | Linux 使用两假屏 FakeBackend；mac 缺截图/编号/应用列表/检测却展示通用引导 | `desktop/platform.rs:13–60,116–184`；`Onboarding.tsx:73–125` | 05、04 |
| A33 | P0 发布，源码 | tag job 无 tag 触发、bundle 缺 Bun、缺签名产物/清单；禁用真实 e2e | `.github/workflows/ci.yml:3–7,91–142,184–194`；`tauri.conf.json:57–61` | 05、06 |
| A34 | P1，用户约束+源码 | 托盘按 idle/active/error/paused 重染色，与“不变色”要求冲突 | `tray.rs:103–181` | 04，使用现有原始资源 |
| A35 | P2，现场+源码 | 首页入口堆叠；五步引导推荐未经证实自动；统计开关无功能 | `Overview.tsx`；`Onboarding.tsx:102–150`；`General.tsx:437–447` | 04 |
| A36 | P2，源码 | 非核心请求失败阻止全部刷新；隐藏时独立预览/截图定时器可能继续；排除页也抓屏 | `App.tsx:117–149,195–229,268–291,488–540` | 04、05 |
| A37 | P2，源码/证据管理 | 审查时 README 宣称功能完整（本次已修正文档状态）；测试含源码/文案钉死和条件断言；库默认/e2e 多测 Grid | `README.md:9,43–53`；`design.test.ts`；`engine/tests/exit_barrier.rs:65–94`；`win/src/bin/e2e.rs:78–85,175–190` | 06 |
| A38 | P1，源码 | e2e 声称仅移动自建窗口，实际把同桌面其他应用纳入迁移，恢复断言只过滤自建集合 | `win/src/bin/e2e.rs:15–16,78–85,175–190` | [06 测试隔离](06-implementation-and-validation.md#41-先修测试隔离) |

表中简写路径：`engine/layout/journal/settings/detect/domain` 位于 `crates/`；`win/mac` 位于 `crates/platform/`；`commands/monitor/session/tray/update` 与标为 `desktop/lib/platform` 的桌面模块位于 `apps/desktop/src-tauri/src/`；前端页面位于 `apps/desktop/src/pages/`，App 位于 `apps/desktop/src/`。源码行号是审查基线定位，实施后以符号和调用关系为准。

## 保留的正确基础

无需推倒重写：Rust 核心与平台 trait、纯布局模块、持久化 preimage/intent 的方向、Windows PMv2/DWM 可见矩形、稳定显示器 ID 的意图、用户排除不阻断恢复、部分平台实现、Tauri/React 壳与现有中性视觉都可继续使用。修复约束失效、元数据丢失和分散执行路径，价值高于增加页面或新框架。

已有测试与验证记录是历史场景证据。自建窗口移动成功、截图不溢出、日志 CRC 前缀可读，分别只能证明对应路径；不能据此反驳当前真实窗口不工作，也不能证明默认收拢、跨应用恢复、自动连接或发行链已完成。
