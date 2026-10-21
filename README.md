# Conflux

把多显示器主机上的窗口，一键收拢到你指定的那一块屏；结束后按快照复原。

用于这种场景：主机有三块屏，你用只有一块屏的笔记本通过远程软件连回来，只能看到其中一块，其他屏上的窗口全在视野之外。

## 状态

**M0 技术验证已完成。** 目前只有平台层与几何层，没有 UI、没有 journal、没有恢复引擎。

已验证（在真实 Windows 桌面上跑出来的，不是文档推断）：

| 项 | 结果 |
|---|---|
| 跨进程移动窗口 | 0 px 偏差（`probe --move`，移动一个**另一个进程**的窗口跨两块屏再移回） |
| 可见矩形 vs 外接矩形 | 差 7/0/7/7 px 的不可见边框；用错方向会导致 28 px 漂移，已修正 |
| 显示器稳定 ID | `monitorDevicePath` 解析成功，形如 `\\?\DISPLAY#DEL41FF#4&3abf5356&0&UID4147#{...}` |
| 显示器枚举 | 2 屏、主屏标识、工作区、有效 DPI 均正确 |
| 窗口枚举 | 592 个顶层窗口中筛出 4 个合格候选（过滤 tool/owned/cloaked/minimized） |
| DPI 换算 | 100%↔150% 往返误差 ≤1 px（单元测试） |

## 结构

```
crates/domain/          # 几何、身份、状态机；无 OS 依赖
crates/platform/        # trait + Fake 实现（4 个测试覆盖拒绝/自纠正/最小尺寸/消失）
crates/platform/win/    # windows-rs 实现 + probe 二进制
docs/plan.md            # 产品决策（平台、触发、恢复、商业化）
docs/impl-plan.md       # 实现计划与任务序列
docs/spec/              # 完整 PRD / 技术设计 / ADR / 路线图（更大范围的参考）
docs/verify/            # 平台 API 的对抗性事实核查
```

## 运行

```bash
cargo test                                  # 19 个测试
cargo run -p conflux-platform-win --bin probe           # 只读：打印显示器与候选窗口
cargo run -p conflux-platform-win --bin probe -- --move # 移动一个子进程窗口跨屏并复原
```

需要 Rust stable + MSVC 工具链。`--move` 会创建一个临时窗口并移动它，不影响你的其他窗口。

## 下一步

见 [docs/impl-plan.md](docs/impl-plan.md) §6：journal → 引擎 → macOS 适配 → UI → 打包。
