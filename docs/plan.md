# Conflux 产品规划与决策备忘

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

## 2. 建议的 v1 范围（相对规格的收缩）

规格的 M3 是"三平台 + 商业化 + 规则热更新 + License"。建议把第一版定义为 **M0.5 可自用产品**，先把闭环跑通，再谈商业化。

| 项 | 规格（M3） | 建议 v1 | 理由 |
|---|---|---|---|
| 平台 | Windows + macOS + Linux | **Windows 单平台** | 多显示器 + 单屏远程的主场景在企业 Windows 上最集中；Windows 窗口 API 最可控；先证明价值 |
| 触发 | 自动检测 + 规则包 | **全局热键 + 托盘一键**，加 1 个已验证工具的可选自动触发（RustDesk 或 TeamViewer） | 检测是最大的不确定项，不该成为第一版的阻塞 |
| 布局 | 网格/主次/相对顺序/自定义模板 | **网格 + 相对顺序** | 主次分区需要用户调参，先不做 |
| 恢复 | journal + 冲突感知 + 崩溃恢复 | **journal + 冲突感知**（崩溃恢复必须有） | 崩溃恢复是安全底线，不能省 |
| 更新 | Tauri updater + 灰度 + 回滚 | **Tauri updater**（签名），不做灰度 | 灰度需要服务端，单人维护成本高 |
| 授权 | License + MoR + 试用 | **无**（免费自用/内测） | 没有用户前不做收费 |
| 虚拟屏 | M4 | **不做** | 与核心价值无关 |
| helper | M4 | **不做**（跳过提权窗口） | 提权窗口是长尾 |
| 设置 UI | 10 个页面 | **3 个页面**：概览/目标屏与布局/通用 | 先能用 |

预估：这个范围的工作量是规格 M3 的 15–20%，但仍包含全部安全关键路径（journal、回读、恢复）。

---

## 3. 必须现在拍板的决策

### D1. 平台顺序

| 选项 | 代价 | 收益 |
|---|---|---|
| A. Windows 先行 | 放弃 macOS/Linux 早期用户 | 最快看到可用产品；API 最可控；企业市场最大 |
| B. Windows + macOS | 双倍适配成本；macOS 需 TCC 授权流程 + 签名证书 | 覆盖"设计师/开发者"人群 |
| C. 三平台并行 | 至少 3 倍适配；Linux Wayland 用户无法使用 | 只有团队 ≥3 人时合理 |

**建议 A**。macOS 的 AX 实现可以在 Windows 版稳定后增量加入，架构上预留 `crates/platform/{win,mac,x11}` 即可。

### D2. 触发方式的产品定位

| 选项 | 说明 | 风险 |
|---|---|---|
| A. 纯手动（热键/托盘） | 用户按一下就搬，零误报 | 不算"无感"，卖点弱 |
| B. 手动为主 + 单工具自动 | 对 RustDesk/TeamViewer 做规则，其余手动 | 需要为每个工具维护规则；工具升级会失效 |
| C. 追求通用自动检测 | 进程 + 端口 + 日志启发式 | 误报代价极高（用户在本地工作时窗口被搬走），核查已证明没有可靠信号 |

**建议 B**，且自动触发默认**关闭**，由用户在向导里显式开启，并提供"干跑"模式先观察。这与规格的保守策略一致，但把它从"可选"提升为"默认"。

### D3. 目标屏选择

核查证明无法知道远程端在看哪块屏。三种产品处理：

| 选项 | 用户体验 | 实现 |
|---|---|---|
| A. 默认主屏 | 零配置，但远程端可能看的是副屏 | 最简单 |
| B. 用户在向导里指定 + 远程端确认标记 | 一次性配置，之后稳定 | 需要在目标屏画一个临时标记让用户远程确认 |
| C. 热键循环切换 | 随时纠正 | 需要托盘反馈 |

**建议 B + C 都做，A 作为默认值**。B 是"确认契约"，C 是"纠错通道"。这是规格里已经写对的少数设计之一。

### D4. 商业化时点

**建议：v1 完全免费，不做 License 服务。** 在积累 100+ 真实用户、拿到误报率和恢复成功率数据之前，任何定价都是猜测。规格里的 US$29/年 可以保留为假设，但不进入第一版实现。

---

## 4. 技术基线（已核实，可直接开工）

```
Conflux/
  apps/desktop/            # Tauri 2 + React + TS + Tailwind + shadcn/ui
    src-tauri/             # Tauri 生命周期、command 白名单
  crates/
    domain/                # 状态、值对象、错误（无 OS 依赖，可单测）
    orchestrator/          # 单写者状态机、事务、退出屏障
    window-engine/         # 枚举、过滤、布局、迁移、恢复
    platform/
      win/                 # Win32 FFI（windows-rs）
      mac/                 # AX + CG（后续）
      x11/                 # xcb/x11rb（后续）
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

不需要写完产品就能验证风险：

1. **窗口搬迁可行性探针**：一个 Rust 二进制，枚举窗口、打印 `DWMWA_EXTENDED_FRAME_BOUNDS` 与 DPI、把一个记事本窗口从副屏移到主屏并回读。验证 §4 的第 2、3、4 条。
2. **检测信号采样**：用 RustDesk/TeamViewer 真实连一次，记录 `SM_REMOTESESSION`、`WTSClientProtocolType`、进程列表、TCP 表、目标工具日志尾部。验证"到底能拿到什么信号"。
3. **journal 断电测试**：写入后立刻 `TerminateProcess`，重启读回。验证原子写假设。

这三件事的结论会直接决定 D2 的选择。

---

## 6. 与规格文档的关系

- [docs/spec](spec/) 保留为**完整设计参考**：数据模型、状态机、验收用例、风险登记都可以直接复用。
- 但其中 M3 的范围、6.5 FTE 假设、商业化章节不适用于当前阶段。
- 我已经把核查结论回写进 `spec/technical-design.md`（DPI 算法、显示器身份、UIPI、自我纠正、三平台 API 表、工具端口表、Wayland 边界、禁屏 flags）。规格里标 `[待验证]` 的项与 `verify/` 里的 `不确定` 是一一对应的。
- 规格中没有、但必须补的：`docs/verify/README.md` 的三条结论（无通用检测、虚拟屏不可行、恢复比迁移难）。

## 7. 事实说明

- 原始要求用 `gpt-6-astra-max`；该模型在当前 Codex 账号下返回 `model is not supported when using Codex with a ChatGPT account`，实际使用配置默认的 `gpt-6-astra`（reasoning effort: ultra）生成规格。
- 本机当前**没有 Rust 工具链**（`rustc`/`cargo`/`rustup` 均不存在），有 Node 24.12、npm 11.6、MSVC 14.44 + Windows SDK 10.0.26100、WebView2 运行时 155。开工前需安装 Rust。
