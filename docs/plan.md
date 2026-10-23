# Unfy 产品规划与决策备忘

日期：2026-10-21。本文是给项目负责人的决策材料，不是对外文档。
输入：[docs/spec](spec/)（`gpt-6-astra` 生成）+ [docs/verify](verify/)（三个独立 agent 的对抗性核查）。

---

## 1. 执行摘要

规格已经足够详细（约 1900 行，18 个 P0、41 条验收、15 个 ADR、8 条工具种子规则、22 条风险），但**它不是为单人/小团队写的**：路线图假设 6.5 FTE、6 个月、180–320 万人民币。以当前实际条件（1 人 + AI 编码），直接照做会在 M0 就卡住。

同时，独立核查推翻了原始设想里最关键的一条假设。必须先修正认知，再谈实现。

| 原始设想 | 核查结论 | 影响 |
|---|---|---|
| "检测到被任意软件远程" | **做不到通用检测**。`SM_REMOTESESSION` 只有 RDP 会置位；TeamViewer/AnyDesk/RustDesk/Parsec/Sunshine/ToDesk/向日葵 均不置位（RustDesk 源码、Sunshine 全仓库 grep 为证）。macOS 无"谁在录屏"API；Linux 无"正在被控制"信号 | "自动"必须降级为"用户确认的入站证据 + 热键兜底" |
| "检测当前主显示器并搬过去" | 能拿到**主显示器**，但拿不到"远程软件正在捕获哪块屏" | 目标屏必须由用户指定并确认，UI 不能写"已识别远程屏" |
| "断开后自动恢复" | 可行，但比迁移难：Chromium 会覆写外部给的窗口位置；`WINDOWPLACEMENT` 是 workspace 坐标；X11 下 `GetGeometry` 是父窗口坐标 | 必须写后回读 + 有界重试 + 持久化 journal |
| "多端通用" | Windows/macOS 可行（macOS 需 TCC 授权）；Linux 仅 X11；Wayland 无通用窗口控制 | 三平台不是等价工作量，Linux 是残值 |
| "虚拟显示器" | Windows 需 EV 证书 + 微软 attestation 签名（ARM64/Server 实际要 WHQL）；macOS 私有 API 必被 MAS 拒；Linux 需内核模块 | 成本量级不同，且**窗口搬迁根本不需要它** |

**结论**：产品成立，但价值主张要改。真正卖的不是"自动检测"，而是**"一键/自动把工作窗口收拢到远程能看到的那块屏，并在断开后安全复原"**。检测只是锦上添花，恢复正确性才是护城河。

---

## 2. 已确认的 v1 范围（决策已拍板）

规格的 M3 是"三平台 + 商业化 + 规则热更新 + License"。v1 收缩为 **M0.5 可自用产品**，先把闭环跑通，再谈商业化。

**已确认的四项决策**：

| 决策 | 结论 | 直接影响 |
|---|---|---|
| D1 平台 | **Windows + macOS** | 需要 Windows 与 macOS 两套平台适配；Linux 推迟；架构上仍预留 `crates/platform/x11` 但不实现 |
| D2 触发 | **纯手动**（全局热键 + 托盘） | 不实现规则包、日志 tail、进程/TCP 探测、置信度融合；检测子系统整块推迟 |
| D3 恢复 | **冲突感知**（默认尊重用户改动） | 必须实现 journal、写后回读、外部改动检测、可见性夹取；不实现"强制还原全部" |
| D4 商业化 | **暂不做**，免费内测 | 不实现 License 服务、MoR、试用、设备额度；Tauri updater 保留（内测需要） |

| 项 | 规格（M3） | v1 实际范围 | 理由 |
|---|---|---|---|
| 平台 | Win + macOS + Linux | **Windows + macOS** | macOS 需 TCC 授权向导 + Developer ID 签名；Linux X11 推迟 |
| 触发 | 自动检测 + 规则包 | **纯手动** | 核查证明无可靠通用信号；误报代价高 |
| 布局 | 网格/主次/相对顺序/自定义模板 | **网格 + 相对顺序** | 主次分区需要用户调参，先不做 |
| 恢复 | journal + 冲突感知 + 崩溃恢复 | **journal + 冲突感知 + 崩溃恢复** | 全部保留，安全底线 |
| 更新 | Tauri updater + 灰度 + 回滚 | **Tauri updater**（签名），不做灰度 | 灰度需要服务端 |
| 授权 | License + MoR + 试用 | **无** | 免费内测 |
| 虚拟屏 | M4 | **不做** | 与核心价值无关 |
| helper | M4 | **不做**（跳过提权窗口） | 提权窗口是长尾 |
| 设置 UI | 10 个页面 | **3 个页面**：概览 / 目标屏与布局 / 通用 | 先能用 |

因为不做自动检测，规格里的这些模块在 v1 **不需要实现**：`crates/detection`、`rules/`、规则包 JSON Schema、8 条工具种子规则、签名规则热更新、kill switch、`V-DETECT`/`V-RULE` 验证项。它们保留在规格里作为 v2 的设计资产。

工作量对比：v1 ≈ 规格 M3 的 12–18%，但**安全关键路径一条不少**（journal、回读、恢复、退出屏障）。macOS 的 AX 实现与 TCC 向导是本范围里最大的单块新增成本。

---

## 3. 决策记录

### D1. 平台顺序 → **Windows + macOS**

后果：

- 需要两套平台适配器：`crates/platform/win`（windows-rs）、`crates/platform/mac`（objc2 + AX/CG FFI）。
- macOS 必须先解决三件事，否则功能不可用：Accessibility 授权向导、Developer ID 签名（ad-hoc 签名会让授权每次更新后失效，见 [verify/macos.md §9](verify/macos.md)）、原生全屏窗口跳过。
- Linux 不实现，但目录与 trait 边界预留；Wayland 无论何时都不承诺。
- 发布形态：Windows NSIS（每用户）+ macOS DMG（Universal，需 Intel 与 Apple Silicon 各实测）。

### D2. 触发方式 → **纯手动**

后果：

- 交互只有三个入口：全局热键、托盘菜单、设置窗口按钮。
- 不实现：进程扫描、TCP 表、日志 tail、规则 DSL、置信度融合、干跑模式、规则包签名与热更新、kill switch。
- 热键默认不预占系统组合键；注册失败必须显式提示并保留托盘入口（规格 P0-09-B 的验收仍适用）。
- 产品文案不得出现"自动检测远程连接"。可以说"一键把窗口收拢到你指定的屏幕"。

### D3. 恢复策略 → **冲突感知**

后果（全部来自规格 ADR-011，保留不变）：

- 第一次移动窗口前必须把 preimage 持久化并拿到 durable ack，失败则零变更。
- 每个窗口操作后回读；偏差 >2px 有界重试（最多 2–3 次），不无限争抢。
- 迁移后与 `last_applied` 偏差超过 4 逻辑单位且持续 500ms → 标记 `external modification`，恢复时保留用户位置。
- 原显示器缺失时夹到当前可见工作区，标注"原屏不可用"，不报告精确恢复。
- 最大化窗口：restore → move → maximize，或改 `rcNormalPosition` 保持 `SW_SHOWMAXIMIZED`。
- 崩溃后：不认领身份不明的窗口，只提供候选预览 + 通用越界救援。

### D4. 商业化 → **暂不做**

后果：

- 不实现 License 服务、MoR 接入、试用、设备额度、离线凭证、退款流程。
- 设置界面无"账户与 Pro"页面；所有功能免费。
- Tauri updater 保留（内测需要自动更新），但只有 stable 单通道，无灰度、无 kill switch。
- 规格 [roadmap.md](spec/roadmap.md) 的商业化章节保留为未来参考，不进入当前排期。

### 仍待决定：目标屏选择（D5）

核查证明无法知道远程端在看哪块屏，所以必须用产品手段确定：

| 选项 | 用户体验 | 实现成本 |
|---|---|---|
| A. 默认主屏 | 零配置，但远程端可能看的是副屏 | 最低 |
| B. 向导中指定 + 远程端确认标记 | 一次性配置，之后稳定 | 需要在目标屏画临时标记（文字/色块，非截图） |
| C. 热键循环切换目标屏 | 随时纠正 | 需要托盘显示当前目标与序号 |

**建议 A 作为默认值 + B 作为首次向导 + C 作为纠错通道**。三者互补而非互斥，B 是"确认契约"，C 是"远程中切屏后的唯一补救手段"。

---

## 4. 技术基线（已核实，可直接开工）

```
Unfy/
  apps/desktop/            # Tauri 2 + React + TS + Tailwind + shadcn/ui
    src-tauri/             # Tauri 生命周期、command 白名单
  crates/
    domain/                # 状态、值对象、错误（无 OS 依赖，可单测）
    orchestrator/          # 单写者状态机、事务、退出屏障
    window-engine/         # 枚举、过滤、布局、迁移、恢复
    platform/
      win/                 # Win32 FFI（windows-rs）
      mac/                 # objc2 + AX/CG FFI
      fake/                # 测试替身
    storage/               # journal、配置
  tests/
```

关键实现约束（每条都有核查依据）：

1. **先写 journal 再动窗口**。写入失败 = 零窗口变更。
2. **每个窗口操作后回读校验**，偏差 >2px 有界重试（最多 2–3 次）。
3. **最大化窗口**：restore → move → maximize，不要直接 `SetWindowPos`。
4. **DPI**：PMv2 线程上 `GetWindowRect` 与 `rcWork` 同为物理像素；用 `GetDpiForMonitor(MDT_EFFECTIVE_DPI)` 而非 `GetDpiForWindow` 判断屏幕缩放；用 `DWMWA_EXTENDED_FRAME_BOUNDS` 做视觉尺寸换算。
5. **显示器身份**：`monitorDevicePath`（Windows）/ `CGDisplayCreateUUIDFromDisplayID`（macOS）/ RandR EDID（Linux），绝不用索引。
6. **跳过**：提权窗口（UIPI）、原生全屏、DRM/独占全屏、其他虚拟桌面、系统外壳、IME 候选、本应用自身。
7. **退出/更新/卸载**必须先通过恢复屏障。

---

## 5. 立刻可做的三件事（M0 验证，1–2 天）

不需要写完产品就能验证风险。因为 D2 已定为纯手动，原"检测信号采样"不再是阻塞项，替换为 macOS 权限探针。

1. **Windows 窗口搬迁探针**（Rust）：枚举顶层窗口，打印 `DWMWA_EXTENDED_FRAME_BOUNDS`、`GetWindowRect`、`GetDpiForMonitor`，把一个窗口从 150% 屏移到 100% 屏再移回，回读偏差。验证 §4 的第 2、3、4、6 条。
2. **macOS AX 探针**：一个最小 .app，申请 Accessibility 授权，枚举 `kAXWindowsAttribute`，读写 `kAXPositionAttribute`/`kAXSizeAttribute`，记录哪些应用返回 `kAXErrorAttributeUnsupported`，并验证 size→position→size 的两次设置是否必要。
3. **journal 断电测试**：写入后立刻 `TerminateProcess`，重启读回。验证原子写与 durable ack 假设。

产出：一份三平台能力矩阵（能/不能/需确认），直接决定 `crates/platform/{win,mac}` 的 trait 边界。Windows 侧已在 [verify/windows.md](verify/windows.md) 有文档级结论，探针用于确认应用级行为。

---

## 6. 与规格文档的关系

- [docs/spec](spec/) 保留为**完整设计参考**：数据模型、状态机、验收用例、风险登记都可以直接复用。
- 与已确认范围冲突的部分：M3 的三平台范围、6.5 FTE 假设、商业化章节、检测与规则子系统、Linux/Wayland 适配 —— 不进入 v1 实现，保留为 v2 设计资产。
- 已把核查结论回写进 `spec/technical-design.md`（DPI 算法、显示器身份、UIPI、应用自我纠正、三平台 API 表、工具端口表、Wayland 边界、禁屏 flags、IDD 签名路径）。规格里标 `[待验证]` 的项与 `verify/` 里的 `不确定` 一一对应。
- 规格中没有、但必须补的：`docs/verify/README.md` 的三条结论（无通用检测、虚拟屏不可行、恢复比迁移难）。

## 7. 事实说明

- 原始要求用 `gpt-6-astra-max`；该模型在当前 Codex 账号下返回 `model is not supported when using Codex with a ChatGPT account`，实际使用配置默认的 `gpt-6-astra`（reasoning effort: ultra）生成规格。
- 本机初始没有 Rust 工具链；已安装 stable MSVC toolchain。可用环境：Node 24.12、npm 11.6、MSVC 14.44 + Windows SDK 10.0.26100、WebView2 运行时 155。
