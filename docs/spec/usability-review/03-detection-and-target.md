# 03 · 远程检测、自动化与目标屏规格

状态：待实施。默认可靠路径是用户直接整理/恢复；自动化只能由已验证的连接生命周期授权。软件正在运行、虚拟屏存在、TCP 已建立，都不能等价于「有人正在控制这台主机」。

## 1. 审查结论与定位

| 证据 | 当前行为 | 影响 |
|---|---|---|
| `crates/detect/src/lib.rs:198–234,295–332` | Aggressive 接受进程存在；Balanced 接受虚拟屏存在；一次无候选就结束 | 本地常驻软件也能触发，暂时观测失败可被当成断开 |
| `crates/detect/src/platform.rs:77–79,112–195` | 查询失败返回空集合；端口与任意已知工具 PID 组合，没有入站/控制方向 | 无法区分云端心跳、控制别人、被控连接和查询失败 |
| `apps/desktop/src-tauri/src/monitor.rs:37–65,442–488` | 观测线程在断连恢复倒计时中同步等待 | 等待时无法轮询自然重连 |
| `monitor.rs:186–265,374–420` | 重连取消挂在 gather 内，受 cooldown/自动整理开关阻断；自动结果只返回 applied | 重连仍可能恢复；自动整理失败被计为零待处理 |
| `commands.rs:1971–2017`、`tray.rs:424–445` | 窗口与托盘分别处理暂停，历史 deferred_restore 不与会话关联 | 同一动作不同结果，重连后恢复暂停可能把窗口移回去 |
| `commands.rs:1944–1955`、`App.tsx:308–352` | 手动标记取反整体 active；后端 Message 对象被前端当字符串 | 「结束标记」与真实 active 不符，证据可能显示 `[object Object]` |
| `engine/src/lib.rs:297–308` | 失联目标自动回退主屏 | 主屏未必是远程能看到的屏幕 |

当前已知能力：能读本机显示器拓扑、截图、编号和部分运行信号；**不能读取任意远程软件当前捕获哪块显示器**。本次真实窗口中用户选择的是 Unfy 编号 2 的主屏；这来自配置，不能称作自动识别远程屏。

## 2. 检测数据契约

保留 `crates/detect` 作为纯观测/状态归约模块，修订现有模型，不建设规则 DSL、云规则包、置信度平台。

```text
Observation {
  source_id, platform, tool_version?, sampled_at,
  availability: ok | unavailable,
  evidence: application_running | virtual_display_present |
            inbound_session_started | inbound_session_alive |
            inbound_session_ended,
  connection_id?, session_kind: mirror | session | unknown
}
RemoteState = unknown | candidate | connected(connection_id, kind) |
              disconnected(connection_id)
```

规范：

1. 查询失败是 `unavailable`，不得合成为空观测；进程消失只结束其进程证据，不能代替未验证工具的会话断开。
2. `candidate` 只用于帮助排查，不能搬窗。显示「发现 UU 远程在运行；尚不能确认有客户端连接」。不写「已识别 UU 远程会话」。
3. `connected/disconnected` 只能由通过工具+平台+版本正负例的入站会话证据产生；多个客户端不能因其中一个断开就结束其他客户端的会话。
4. 切换设置、关闭检测、权限失败、睡眠、锁屏不制造 disconnected；保留恢复记录，将检测标为未知/暂停。
5. 去抖处理可靠信号的短暂抖动，不把弱信号累计成可靠证据。无断开证据的来源不提供「断开自动恢复」。
6. 不使用窗口标题、端口猜测或进程名作为授权信号；进程名匹配至少是完整规范化名称，不使用泛 substring。

## 3. 支持策略与自动化上线条件

| 场景 | 当前可提供 | 本轮实现要求 |
|---|---|---|
| UU/TeamViewer/AnyDesk/ToDesk/向日葵等镜像工具 | 通用手动收拢/恢复；部分进程信息 | 进程信息不能自动移动；首次明确选择目标；工具适配必须单独验收 |
| Windows RDP | 当前会话协议类型 | 加入当前 session id、连接状态/变更证据；按会话拓扑处理越界窗口，不能把 RDP 类型直接当作控制台多屏收拢指令 |
| Parsec/Sunshine 等及长期存在的虚拟屏 | 本机拓扑/虚拟设备信息 | 虚拟屏存在不授权自动整理；需独立客户端连接生命周期 |
| macOS | 目前 detector 返回空观测 | 手动能力独立开放；不展示「自动（推荐）」；权限不是连接证据 |
| Linux | 当前窗口后端未实现 | 不承诺自动窗口操作；平台后端验收前禁用相关动作 |

工具适配的最小交付物：锁定工具与 OS 版本、信号接口或可核验的状态来源、入站/出站区分、连接 id、明确结束事件、查询失败语义、权限需求；保存脱敏后的正负例轨迹和结果。首先评估用户使用的 UU 远程。**仓库目前没有足够信息指定可靠的 UU 入站 API/日志字段；后续先验证实际接口，不能猜路径/正则或把 GameViewer 进程常驻包装成实现完成。** 如果没有可靠接口，该工具手动路径必须完整，自动功能保持不可用并解释原因。

RDP 验收至少包括连接、断开但会话存活、重连、console 切换、多用户会话、单屏与多屏客户端；只操作本用户当前会话。默认在 RDP 会话中救援越界窗口，不主动重排当前仍可见的多屏窗口。远程工具协议/服务不由 Unfy 修改。

## 4. 自动化调度

使用现有 `schedule.rs` 的代次取消概念，统一由 02 的动作协调入口持有；不允许 monitor、commands、tray 分别直接执行恢复策略。

```text
on connected(session):
  invalidate pending auto-restore first
  update current remote state
  if no verified mirror lifecycle / target unconfirmed / paused / maintenance: stop
  if transaction belongs to this session: keep original baseline; no re-gather
  if user already ended this session's transaction: do not re-gather until new session
  else begin gather (automatic policy explicitly enabled)

on reliable disconnected(session):
  if tx is not owned by session: stop
  if auto-restore disabled or automation paused: retain tx; no hidden debt
  else schedule(tx_id, session_id, generation, due_at)

at deadline:
  acquire operation coordination
  recheck tx_id, session_id, generation, remote state, pause,
          auto-restore setting, topology/target, maintenance
  if any condition changed: cancelled, retain tx
  else restore through the shared engine path
```

- 观测循环持续运行，等待在独立可取消任务中；不得持锁 sleep。
- 重连取消必须先于 cooldown 判断；冷却仅限制新的自动整理次数，不能限制取消或手动恢复。
- 延迟默认 15 秒、允许 0–120；只用于可靠断连后的自动恢复。手动恢复、退出前恢复立即执行。
- 手动整理生成手动事务，检测器不能随意恢复它；除非用户明确把本次事务绑定到已确认的当前连接。
- 暂停只阻止自动动作并取消待执行自动恢复，保留事务；手动整理/恢复可用。继续自动化只基于当前状态决策，不消费陈旧 `deferred_restore=true`。
- 取消本次恢复只取消 deadline；不调用 keep/discard。取消后同一连接的重复结束采样不得重新启动倒计时，直到新的可靠会话或用户操作。
- 锁屏/睡眠期间不下发新窗口操作。唤醒后先复核身份和拓扑，观测未知不触发恢复。
- 新窗口发现适用于所有活动事务；可见新窗只由「收拢新窗口」纳入，越界新窗按 01 的既有事务救援规则处理。暂停、锁屏、维护或目标未确认时只提示，不自动写入；候选/未知远程状态不能授权启动新事务。

## 5. 目标屏契约

持久化目标至少包含 native_id、identity_quality、last_known_label；编号仅用于显示，不参与身份匹配。

| 目标状态 | 手动行为 | 自动行为 |
|---|---|---|
| 未选择 | 推荐主屏并请用户确认；不直接移动 | 禁止 |
| 强/中身份且在线、已确认 | 正常整理 | 其他条件满足才允许 |
| 保存目标断开 | 明确选择另一块屏或取消；已有事务仍可恢复/救援 | 暂停新整理，显示原因 |
| 弱身份或相同设备有歧义 | 本次确认，只对本次拓扑有效 | 不跨重启自动绑定 |
| 仅编号或坐标改变 | 依稳定身份找到原目标，更新显示编号 | 不改变目标身份 |

普通目标选择与托盘目标选择使用同一命令。无事务时只选择；有事务时走显式 retarget，保留首次快照，并返回全部窗口结果。不能先保存新目标再吞掉移动错误。不要增加“猜测捕获屏”的后台机制。

## 6. 通知与设置

检测状态与操作结果分别返回：`MessageDto` 用 `renderMessage` 渲染；事件含 session_id、operation_id、revision、reason。`acted=false` 不能推断「自动整理已关闭」。自动整理必须返回与手动完全相同的逐窗 report 和未完成数量。

设置面向用户仅提供「手动」「已验证连接自动触发」及必要的自动恢复偏好、延迟。仅存在候选证据时，自动选项禁用并说明该工具暂不支持可靠检测。关闭自动化不移动窗口，也不自动清除本次整理；保留醒目的手动恢复入口。

## 7. 验收

D-01 远控软件常驻、只控制别人、云端心跳、持久虚拟屏：至少持续 10 分钟，自动移动次数为零。

D-02 一次或连续查询错误、睡眠、降低灵敏度/关闭自动化：没有伪造断开，也不恢复用户窗口。

D-03 15 秒倒计时第 5 秒重连（处于 30 秒冷却内）：旧恢复取消，无重复整理；轮询仍进行。

D-04 截止前取消/暂停/更换目标/退出维护、或 token 校验后等待 engine 锁期间发生重连：旧任务不得执行。

D-05 手动事务遇到检测断开保持原样；自动事务只受自己的 session 结束影响；手动结束后同一连接不重新收拢。

D-06 UI、托盘、热键的暂停/继续与重启后状态一致；没有陈旧恢复欠账。

D-07 目标屏拔出不向未知主屏整理；编号交换仍选原设备；弱身份重启需确认。

D-08 RDP 当前会话、镜像远控、两个客户端重叠连接分别验证；没有外部证据的工具不计为自动支持。

D-09 自动结果含所有失败/限制，不显示对象字符串，不先宣称自动关闭再宣称成功。
