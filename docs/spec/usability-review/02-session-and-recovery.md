# 02 · 事务、恢复、崩溃与动作一致性规格

状态：待实施。P0：先恢复可信的状态、写入约束和恢复记录，再扩展自动化与布局。窗口已经搬动但无法说明如何恢复，比没有自动搬动更严重。

## 1. 直接证据

- `Engine::preview`（`crates/engine/src/lib.rs:545–569`）把 Active/Degraded/Error 改成 Planning 后无条件改回 Idle。实际 UI 已观察到「就绪、快照 6、恢复失败 3、整理按钮」。`commands.rs:269–293,1203–1236` 又凭枚举状态计算 pending，导致恢复入口消失。
- 准备时跳过读取失败窗口，提交时仍遍历全部原计划：`engine/src/lib.rs:757–810,863–925`。
- intent/result/flush/close 多处错误被忽略；恢复清空前像，keep 先 take 所有权再关闭：`engine/src/lib.rs:873–986,1240–1267,1492–1501`。
- `recover_latest` 在读取最新日志前按 mtime 删除其他日志：`engine/src/lib.rs:489–542`。坏头、未来 schema 等解码失败可能返回 Clean：`journal/src/lib.rs:388–436`。
- 接管只恢复 preimages，`last_applied` 清空，忽略 intents/results：`engine/src/lib.rs:468–478`。
- Windows `process_start=0` 且 `hwnd_from_id` 仅 IsWindow；macOS 用 AX 指针作为 ID：`win/src/displays.rs:132–139,297–304`、`mac/src/ax.rs:218–225`。
- retarget 先 restore/关闭旧事务，再 migrate 建新基准；回退位置会替代原位：`engine/src/lib.rs:1453–1488`。
- fallback/rescue 只凭 apply 返回成功；mode 不参与冲突判定；native placement/topmost 未恢复：`engine/src/lib.rs:1297–1429`。
- 更新只在下载前检查内存快照；退出尚无停止新写入的准入阶段；未能读入的日志可能不计入屏障：`update.rs:141–197`、`tray.rs:554–588`、`engine/src/lib.rs:1217–1223`。

这里不能推导为「preview 一变 Idle 就直接绕过更新/退出」：现有两条路径仍会检查快照或调用 restore。屏障漏洞来自未读日志和检查之后仍可新增操作，须分别修复。

## 2. 不变量

1. **没有 durable 的首次前像和本次 intent，绝不向该窗口写入。** 准备失败项不进入提交集合。
2. **首次前像整个事务内不可变。** 换目标、再次收拢、新窗口追加、拓扑救援都不能覆盖旧基准。
3. **Applied 不等于恢复完成。** 移动成功产生恢复义务；只有明确的逐窗解决记录能结束它。
4. **身份不明、权限失败、超时、日志不可读不等于窗口已关闭或零待恢复。**
5. **Idle 表示无正在执行操作、无恢复义务、无未知日志阻塞。** 暂停是独立自动化策略，不能释放事务或清除 Error。
6. **预览/状态查询无副作用。** 不改 engine state、事务、错误、暂停；不删除日志或写 corrupt sidecar。
7. **只有持久 Closed 成功才能释放内存事务。** 删除已关闭文件失败只属于清理失败，与未关闭恢复记录区分。
8. UI、热键、托盘、自动、退出/更新使用同一准入与执行语义；所有路径返回同样完整的逐窗报告。

## 3. 最小结构调整

保留现有 Engine + journal + 平台 trait。桌面层增加一个统一动作协调入口，替代 monitor/tray/commands 中的重复决策；不要再建设第二个引擎或通用工作流框架。

```text
Action = Gather | GatherNew | Retarget(target) | Restore |
         Retry(window_refs) | Rescue | KeepAfterConfirmation |
         SetAutomationPaused | CancelAutoRestore | EnterMaintenance
Trigger = Window | Tray | Hotkey | VerifiedSession(session_id) | Lifecycle

OperationReport {
  operation_id, transaction_id?, revision,
  kind, status: completed | partial | cancelled | failed,
  target, results: [WindowResult], summary,
  error?: {code, params, partial_effects}
}
```

同一时间只有一个写者；请求重复时返回当前操作/Busy，不并发写窗。窗口动作在 worker 执行，主线程不拿长时间 engine 锁。状态快照用短锁发布，UI 读取不等待整个批次；拓扑/会话取消代次能在写者工作时变化。操作提交前验证代次，不能只在排队前检查一次。

`preview(&self, …)` 返回纯计划与 target/config/topology revision。用户点过预览时，执行同一规划逻辑并复核版本；版本变化需要刷新计划，不能展示旧预览却执行另一套 restore→replan。

`StatusDto` 一次一致快照返回：revision、operation、transaction 摘要、captured_total、gathered_count、restore_pending、recovery_blockers、target_state、automation_state、allowed_actions、latest_report。计数来自逐窗状态，不从 `EngineState::has_pending_work()` 推导；恢复错误也不只存一条字符串。

## 4. 事务与逐窗状态

现有七个 EngineState 可保留为外观映射，不再同时承担事务是否存在和自动化是否暂停。

| 事务事实 | 外观状态 | 能执行的关键动作 |
|---|---|---|
| 无记录、无阻塞 | Idle | Gather/Preview/Rescue |
| 有未恢复窗口，操作稳定 | Active | Restore/GatherNew/Retarget/Keep/Preview |
| 有未完成结果或恢复困难 | Degraded | Restore/Retry/Keep/Preview；新动作受未结 OS 请求限制 |
| 日志/身份扫描存在阻断 | Error/RecoveryRequired | 查看原因、重试读取、可证明安全的恢复；禁止新整理 |
| 正在写入 | Migrating/Restoring | 只读状态、提出取消后续批次；不开始第二写者 |

每个窗口记录含：immutable preimage、identity proof/quality、最近 intent/observed、last_owned_write（含 mode）、外部变更状态、恢复责任和解决原因。已经解决的窗口不在下一次 restore 再处理。

**恢复终态：** VerifiedRestored、ConfirmedGone、KeptExternalChange、ExplicitKeep。原屏缺失但窗口被救到可见区域是 `SafeFallbackAwaitingOriginal`：安全可用但仍保留原位义务，允许之后接回原屏再恢复；不得自动当精确恢复并删除基准。若用户接受回退位置，明确 Keep 后才终结。该策略优先保留原始目标，代价是少量持久待处理记录。

身份不明没有可以静默终结的默认值。不能恢复时允许用户明确放弃对应记录；这个动作不能把未经读取的损坏日志全局标为解决。

## 5. Journal 修订

在现有 append + CRC 结构上升级 schema；不引入数据库。最小记录协议：

```text
Begin(schema, tx_id, trigger, original_target, coordinate_space)
Captured(tx_id, window_ref, immutable_preimage, identity_evidence)
Intent(tx_id, op_id, write_seq, window_ref, kind, requested)
Observed(tx_id, op_id, write_seq, window_ref, actual, outcome)
Resolved(tx_id, window_ref, resolution, confirmed_actual?)
Closed(tx_id, last_seq)
```

- write_seq 单调递增；结果精确对应 intent，不以“这个窗口曾经有一个 result”解决后续写入。
- 重复 Captured 仅允许完全相同的首次前像且有明确幂等语义；不同前像为协议错误。不能通过重复写覆盖原位。
- 批次顺序：Captured durable → Intent durable → OS submit → 同一最终观测的 Observed → durable。OS 已变但结果未写成时，记录为可能已移动；下一轮从 durable intent 恢复，不假设未发生。
- append/fsync 失败停止后续平台写入，保留已知责任与 Error；不能只 break 然后报告 Active/成功。
- 关闭：解决所有窗口 → Resolved durable → Closed durable → 释放内存 → 尝试删除。失败可重试且不能先 take 后丢失所有权。
- `recover()` 分别返回 Clean/Unfinished/Corrupt/UnsupportedSchema/Unreadable；仅有效 Begin、匹配事务、合法顺序、末尾有效 Closed 才是已关闭。
- 只有有效日志前缀之后的短尾记录可按中断写入处理；坏头、混入另一个事务、Closed 后追加、未来 schema 都要保留原文件及明确阻塞，不能等价于干净。
- 扫描所有日志，按事务 ID 索引；禁止按 mtime 删除旧事务。多事务有同窗冲突时不自动覆盖或选“最新”，逐项进入恢复审查。已验证关闭的文件可单独清理。
- 读操作不写 sidecar；损坏副本仅由明确的恢复/诊断操作创建，不覆盖原件。

兼容：旧 schema 1 仍可读取；已有 preimage 保留，缺少 write_seq、identity continuity 或 last_applied 时降为待确认，不能补造成功或强身份。格式升级先写新文件并验证，再切换，原文件保留到迁移提交；老版本无法理解新 schema 必须显式拒绝，不当作空日志。

## 6. 窗口身份与原生恢复

Windows 每次读取/写入/native restore 检查当前会话、HWND、PID、真实进程创建时间。运行期可用窗口创建/销毁事件维护本次生命周期代次；同进程 HWND 重用也必须识别。跨 Unfy 重启时，HWND+PID+创建时间仍不是永不复用的窗口身份证明；没有可靠连续性证据时进入待确认，不把旧位置套到新窗口。若后续采用窗口令牌机制，必须先证明它跨进程可写、生命周期可靠且不需要提权；不得先把尚未验证的 API 方案当完成条件。

macOS AX 指针只作持有引用的进程内句柄；运行期建立稳定逻辑 ID，进程重启后的地址不能与旧记录直接匹配。不可唯一确认时保留记录；属性读失败有 Permission/Timeout/Unsupported，不能统一成 Gone。平台能力和对象模型见 05。

Windows 恢复须恢复正常还原位置、mode、必要的 topmost 事实。相同显示器/拓扑且 native placement 仍适用时使用平台恢复能力并读回；拓扑改变时不能盲目回灌过时 `WINDOWPLACEMENT`，先重算正常位置。workspace 坐标只交给对应 native API，不能直接当屏幕坐标用于 SetWindowPos。

## 7. 恢复、冲突与换目标

恢复决策顺序：

1. 只遍历仍有义务的窗口；重新核验身份。确认已关闭可结束；未知保持 pending。
2. 比较当前 rect、mode 与最近一次本工具确认写入。仅 mode 改变（用户最大化/最小化）也要保护。
3. 外部差异持续至少 500ms，且不来自本工具的移动/救援/系统拓扑变化时保守保留。不能断言所有应用回弹都是用户意图；若无法区分则标「外部位置发生变化，已保留」，不强制抢回。
4. 原显示器仍存在且身份可确认：相同几何恢复原位；几何/DPI 变更使用统一转换及约束。必须确认 rect/mode/可操作性。
5. 原屏不存在：仅在当前不可操作时做安全回退；保存首次原位与 last_owned_write，标 AwaitingOriginal。已可见的窗口无需再搬到某个主屏。
6. 每个解决结论 durable；仍 pending 的条目可逐项重试。部分成功后崩溃，已解决项不能重新打开。

Retarget/GatherNew 在同一个事务内追加操作，不先关闭旧事务。已收拢窗口保留第一次原位；用户明确重定位的窗口默认不在自动 retarget 中抢回。新增窗口首次被纳入时才添加前像。存在未结平台请求时先解决请求归属，不能重用一个窗口开始新代次。

目标选择提交与操作结果一起协调：零平台变更的失败保留旧目标；成功或部分成功后提交新目标选择，并展示未完成项及各窗实际去向。不能用旧目标标签掩盖已经发生的部分移动，也不能因失败重新捕获原位。目标设置写盘失败时分别报告本次请求目标与仍已保存的目标，保留恢复入口。

例：P（原屏）→ Q（目标）→ 拔原屏→可见救援 R→换目标 S→接回原屏→恢复，原基准仍为 P。UI 需要显示“已安全可见，原屏恢复待处理”，不能用精确恢复计数隐藏回退。

## 8. 退出、更新、卸载屏障

统一维护流程：停止接受新整理/retarget/自动任务 → 使旧定时任务失效 → 等待当前写者结算 → 扫描所有恢复义务（包括不可读日志）→ 请求/执行明确的恢复 → 验证 durable Closed → 安装/退出。

失败返回正常可交互界面并保留待处理；未知文件状态不能视作零。显式 keep 可以解决已知记录，不能抹除未知错误。强杀/断电/系统强制注销不可能保证即时恢复；职责是保留已知日志并在下次可靠处理。

更新下载可以在使用期间进行，但安装前必须重新取得维护准入。当前 updater 的 download_and_install 合并路径需要按插件实际能力分开或在整个不可分割阶段持有维护准入；Windows 安装器可能终止进程，屏障必须发生在这之前。卸载是否遵守屏障要从真实包验证，不能用应用退出代码作证明。

## 9. 验收

R-01 6 条快照、3 已解决、3 待恢复：成功/失败预览、轮询、切页都不改变事务和 pending=3，恢复入口持续可用。

R-02 A 准备成功、B 读回失败：B 所有平台写入数为零；A 可完成；全失败时无伪成功。

R-03 在 Captured/Intent/Observed/Resolved/Closed 的 append 与 flush 边界分别故障注入；只有已耐久准备的项可写，责任不丢，终态一致。

R-04 旧未完成 + 新空/新坏/新关闭日志，各组合保留旧义务；坏头、未来 schema、事务混写、Closed 后追加不返回 Clean。

R-05 A 已保留外部改动、B pending 后崩溃：重启仅处理 B；第二 intent 未完成不被第一 result 掩盖。

R-06 HWND 跨进程和同进程复用、AX 重枚举/进程重启：不能写新对象；未知记录不消失。

R-07 P→Q→拔屏→救援→换目标→接屏→恢复，原基准仍 P；新窗只追加自己的前像。

R-08 平台返回 Ok 但未移动、模式不符、部分尺寸改变、救援回弹：不报精确成功；每次本工具变更更新 last_owned_write。

R-09 用户只最大化/最小化或在目标屏移动窗口：保留其外部变更并准确报告；随后重启不再次恢复已解决项。

R-10 恢复拓扑读取失败、关闭失败、不含内存事务但磁盘日志不可读：有可解释的可重试状态，不永远 busy，也不错误 Idle。

R-11 下载期间整理、退出时热键排队、倒计时等待锁期间重连：不越过维护/代次屏障。

R-12 暂停→保留→再次手动整理、暂停重启、托盘/窗口继续、取消倒计时：一致且不丢恢复记录。
