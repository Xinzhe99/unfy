# Conflux

把多显示器主机上的窗口，一键收拢到你指定的那一块屏；结束后按快照复原。

用于这种场景：主机有三块屏，你用只有一块屏的笔记本通过远程软件连回来，只能看到其中一块，其他屏上的窗口全在视野之外。

## 状态

**v1 功能完整，Windows 已实测，macOS 已实现但未在真机验证。**

产品决策见 [docs/plan.md](docs/plan.md)，实现计划见 [docs/impl-plan.md](docs/impl-plan.md)，发布与签名见 [docs/release.md](docs/release.md)。

## 已验证的行为

以下都是在真实 Windows 桌面上跑出来的结果，不是文档推断：

| 项 | 结果 |
|---|---|
| 跨进程移动窗口 | 0 px 偏差（移动另一个进程的窗口跨两块屏再移回） |
| 端到端迁移 + 恢复 | `e2e` 二进制：6 个自建窗口迁移到目标屏全部可达，恢复全部回到原始矩形（容差 4 px） |
| 崩溃恢复 | 迁移中途强杀进程 → 重启后 UI 显示"上次整理未完成" → 点击恢复 → 窗口回到原始位置，journal 被清理 |
| journal 耐久性 | 写入后强杀子进程 4 次（5/15/40/120 ms），恢复出的记录始终是完整前缀 |
| NSIS 安装包 | release 3.40 MiB / debug 4.56 MiB；静默安装 → 运行 → 静默卸载，无残留进程 |
| 显示身份 | `monitorDevicePath` 解析成功并持久化到设置 |
| 跨 DPI 换算 | 100%↔150% 往返误差 ≤1 px（单元测试） |
| 100 窗口性能 | 迁移 181–200 ms、恢复 94–125 ms（目标 300 ms / 1 s） |
| 用户改动保护 | 远程期间用户拖动的窗口，恢复时保留新位置 |
| 全局热键 | `Ctrl+Alt+G` 移动 3 个窗口；`Ctrl+Alt+Shift+G` 让 11/11 个窗口精确回到原位 |

测试总数 **82**，`cargo clippy --workspace --all-targets -- -D warnings` 干净。

完整记录（含未验证项与复现命令）见 [docs/verification.md](docs/verification.md)。

## 未验证的部分

- **macOS**：AX 后端、TCC 授权流程、`CGDisplayCreateUUIDFromDisplayID` 全部只做了类型检查（交叉编译到 `aarch64-apple-darwin`）与纯逻辑单测，**没有在真机上运行过**。首次在 Mac 上运行需要真机验收。
- **签名、公证、自动更新的实际安装**：本机无证书、更新端点为未配置的空数组。签名流程写在 [docs/release.md](docs/release.md)，但未执行。
- **其他应用的回弹率**：实测桌面窗口迁移时约有 1/3 拒绝移动（Chrome/Electron 系）。拒绝会如实报告并显示进程号，但未做大规模统计。
- **托盘菜单点击**：托盘图标无法被 UI 自动化识别，菜单项走的是与热键相同的引擎路径（已测），但点击本身只做了代码审查。

## 结构

```
crates/domain/          # 几何、身份、状态机；无 OS 依赖
crates/layout/          # 纯函数布局：网格 / 主次 / 保持顺序 / 容量不足级联
crates/journal/         # 追加式事务日志，CRC + 崩溃重放
crates/engine/          # 单写者状态机：预览、迁移、回读重试、恢复、救援
crates/settings/        # 原子写入的设置，拒绝未知字段
crates/platform/        # trait + Fake 实现
crates/platform/win/    # windows-rs 后端 + probe/e2e 二进制
crates/platform/mac/    # AX 后端（未真机验证）
apps/desktop/           # Tauri 2 + React 19 + Tailwind 4
```

## 运行

```bash
cargo test --workspace
cargo run -p conflux-platform-win --bin e2e      # 真实桌面端到端，会创建并移动自己的窗口
cargo run -p conflux-platform-win --bin probe    # 只读：打印显示器与候选窗口

cd apps/desktop
npm install
npm run tauri dev
```

打包：

```bash
cd apps/desktop && npm run tauri build
```

需要 Rust stable + MSVC 工具链、Node 22+。Tauri 首次打包会从 GitHub 下载 NSIS/WiX；无法直连时用 `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR` 指向镜像。

## 明确不做

自动远程检测（没有可靠信号）、虚拟显示器（需驱动签名，且与核心价值无关）、Linux/Wayland、特权 helper、付费授权。

## 已知限制

- macOS 无法移动原生全屏窗口（独立 Space），也无法操作其他 Space 上的窗口。
- Windows 无法操作以管理员权限运行的窗口。
- 无法得知远程端在看哪块屏，目标屏需要用户指定。
- 无自动远程检测：手动热键/托盘触发。
