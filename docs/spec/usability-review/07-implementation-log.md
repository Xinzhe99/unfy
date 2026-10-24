# 可用性修订 · 实施与验证记录

分支：`feat/usability-review-implementation`。基线：`docs/spec/usability-review/` 的 01–06。本文只记录本轮**实际执行**的检查与结果；`docs/verification.md` 保留此前历史证据，两者范围不同。

## 1. 已实施的契约

| 规格 | 实现结果 |
|---|---|
| 01 默认移入 | 默认模式只把异屏/越界窗口移入目标屏，按两屏 DPI 比例保留视觉尺寸；目标屏已有可操作窗口零写入；网格成为唯一显式替代，主次布局删除。 |
| 01 提交与确认 | 只提交已取得 durable 前像的窗口；结果同时校验矩形与模式；失败项零写入并逐窗报告；进度按已提交窗口计数。 |
| 02 只读查询 | `preview` 不再改引擎状态，活动/降级/错误状态与待恢复数量在预览、切页、轮询后保持不变。 |
| 02 事务与恢复 | 首次前像在事务内不可变；`retarget` 追加到同一事务；每窗 `Resolved` 记录在关闭前落盘；日志写入失败中止批次而不是忽略。 |
| 02 崩溃恢复 | 结构校验拒绝混合事务、`Closed` 之后追加、重复 `Begin`、无前像的 `Intent`、冲突的重复前像与未来 schema；未解决日志不再因“有更新文件”被删除；多份未完成日志全部保留。 |
| 02 身份 | Windows 读取真实进程创建时间并在每次句柄解析时校验 PID 与创建时间；`process_start == 0` 的旧记录不再写入任何窗口。 |
| 03 检测 | 只有 RDP 会话生命周期可授权自动动作；探测失败返回 `Unknown` 而不是断开；进程/端口/虚拟显示器只作为候选信息；手动标记与陈旧 deferred 状态删除。 |
| 03 调度 | 倒计时在独立可取消任务中等待；重连取消先于冷却判断；暂停是独立策略，不释放事务。 |
| 04 UI | 恢复入口由未解决窗口数派生；预览不再顶掉恢复；失败卡片移除「全部排除」；保留布局需二次确认并说明后果；设置移除遥测、主次、四级灵敏度；托盘使用原始图标，不随状态或主题变色。 |
| 05 设置 | 单一 `engine_config_from_settings` 供启动、保存、重置共用；候选设置先验证后发布；schema 2 迁移旧值；旧字段在反序列化前剥离。 |
| 05 数据迁移 | Conflux→Unfy 迁移递归复制整棵树（含 `journal/`），先写临时目录再原子提交，失败保留旧目录以便重试。 |
| 05 平台 | 不支持平台返回 `Unsupported`，不再用两块虚构屏幕；macOS 几何按 point、`scale = 1`，AX 布尔按 `CFBoolean` 读取，读失败不再冒充 `Gone`，元素按 +1/−1 引用计数管理，缓存与工作区会过期刷新。 |
| 05 自启 | 写入后回读校验，失败返回给界面，不再静默。 |
| 05 更新 | 先下载，再取维护准入、恢复、从磁盘重算待处理数，最后安装；失败重新开放准入。 |
| 05 诊断 | 打开目录改为固定枚举（配置/诊断），移除任意路径入口。 |
| 06 测试隔离 | e2e 通过作用域后端只暴露自建窗口，不再移动用户桌面上的其他应用。 |

## 2. 实际执行的验证

### 2.1 自动化检查

| 命令 | 结果 |
|---|---|
| `cargo test --workspace` | 通过；新增 `crates/engine/tests/contracts.rs` 9 项契约回归 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 干净 |
| `cargo fmt --all -- --check` | 干净 |
| `cargo check -p unfy-platform-mac --tests --target aarch64-apple-darwin` | 通过（类型检查，非真机） |
| `cd apps/desktop && npm run build` | `tsc` + 137 前端测试 + `vite build` 通过 |

新增契约测试覆盖：预览不改状态、失败预览不掩盖 Error、不可快照窗口零写入、目标屏已有窗口零写入、默认整理保留尺寸、`retarget` 保留首次基线、仅最大化/最小化的用户改动被保留、原屏缺失仍保留义务、无法证明身份的旧记录不写入。

### 2.2 真实 Windows 桌面

只读探针（`cargo run -p unfy-platform-win --bin probe`）：

```
displays: 2  (1920x1080, work 1920x1032, both 100%)
windows: 520 total | visible=44 tool=105 cloaked=24 owned=216 minimized=10
candidates after filters: 4
```

受作用域限制的端到端（`cargo run -p unfy-platform-win --bin e2e`，只操作自建窗口）：

```
migration: applied=6 rejected=0 skipped=0
migration PASS: every window is inside the target work area
idempotence PASS: 6 windows already on target were left alone
restore wall time: 8 ms
restore: complete=true pending=0
restore PASS: every window returned to its original rectangle (tolerance 4 px)
journal PASS: resolved transaction removed its journal
E2E PASS (6 windows, real desktop)
```

真实应用界面（打包协议构建，无障碍树读取）：

- 概览在存在旧恢复记录时显示「部分待恢复 / 待恢复窗口 6 个」并持续提供「恢复本次布局」；点击预览后该入口与计数保持不变。
- 点击恢复：旧记录全部为 `process_start = 0`，结果为「已恢复 0 个窗口，仍有 6 个待处理」，窗口未被写入，记录保留。
- 真实整理：报告「已整理 1 个窗口，4 个已在目标屏，跳过 7 个」；随后恢复报告「已恢复 1 个窗口」，状态回到「就绪」。
- 显示器页仅提供「移到目标屏 / 网格」，默认模式不显示边距与间距；「切换到下一块屏」已移除。
- 设置页检测说明与后端一致（只有 RDP 会话授权自动动作）。
- 托盘截图为原始单色图标，未随状态变色。

## 3. 仍未验证

- **macOS 真机**：仅完成交叉类型检查。多屏混合 Retina、TCC 授权/撤权、AX 超时与跨 Space 行为均未在 Mac 上运行。
- **正式发行**：本机无证书与更新源。tag 构建现在会因 updater 未配置而**故意失败**，这是设计行为；真实签名、公证、旧版→新版升级与自启登录未演练。
- **真实远控会话**：未用 RDP/第三方远控建立实际连接验证自动触发；检测决策由单元测试覆盖。
- **真实应用兼容矩阵**：e2e 使用自建窗口；规格要求的 Explorer/Edge/VS Code/Terminal/原生编辑器/模态框/固定尺寸/提权/UWP 逐项记录尚未执行。
- **100 窗口性能**：本轮未跑 `--stress`，不沿用历史数字作为当前结论。
