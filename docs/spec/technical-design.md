# Unfy 技术设计

版本：0.1，设计基线日期：2026-10-21。状态：可进入 M0 验证与实现拆分，尚未通过平台实测。需求见 [PRD](product-spec.md)，决策见 [ADR](decisions.md)，发布门禁见 [路线图](roadmap.md)。

## 1. 契约、事实级别与范围

本文中 `Unfy` 自定义类型、字段、事件、目录、阈值均为**设计契约**，不是声称已经存在的第三方接口。外部 API 已有官方依据的列出来源；未确认的 API、参数、路径、行为标 `[待验证]`，并关联第 15 节验证任务。文档核验不等于兼容性实测；SDK、依赖和远控工具版本在 M0 锁定并保存证据。

| 项目 | v1（M3）契约 |
|---|---|
| mirror | 在同一交互会话内，将合格窗口集中到用户确认的目标屏；不声称知道远程客户端正在捕获哪个输出 |
| session | 仅救援当前远程会话内越界窗口；不迁移 console 的窗口到 RDP、另一用户或另一 Window Server |
| 权限 | 普通用户运行；权限不足的窗口明确跳过；v1 无特权服务、驱动、显示输出禁用 |
| 恢复 | 首次变更前持久化原始布局；用户修改优先，可见性高于原位复原；安全功能不依赖许可证 |
| 平台 | Windows 10/11 x64/arm64、macOS 13+ Universal、Linux X11 为目标；Wayland 无通用控制，只提供检测状态、通知和手动指引 |
| 故障保证 | 可控退出/更新先恢复；断电、强杀、撤权时不能保证立即操作窗口，保留恢复记录，下一次具备权限时处理；禁止把无能力完成写成成功 |

安全不变量：①没有持久化的 preimage 不移动；②只修改当前用户、当前交互会话中身份验证通过的窗口；③目标矩形经过当前工作区约束；④恢复记录不得在结果未知时清除；⑤一个会话只有一个写入协调器；⑥自动规则不能执行代码或扩展权限；⑦任意失败都不能主动把原本可见窗口放到不可见区域。

“永不永久不可见”落实为上述不变量、启动修复、热插拔救援和人工可恢复路径；它不是对不受控应用、断电或系统权限永久撤销的绝对保证。拒绝控制的窗口要保留失败状态与系统移动窗口指引。

## 2. 架构与模块契约

采用既定 Tauri 2 + Rust + React/TypeScript + Tailwind + shadcn/ui。单一应用主进程持有 Rust 核心和 UI 生命周期；系统 WebView 可产生自身子进程，不将“单进程”误读为进程树只有一个进程。关闭设置窗口后释放非必要 WebView，保留托盘和核心。

```text
Unfy/                         # 规划目录；此次仅交付 docs/spec 六份文档
  apps/desktop/src/              # React 页面、i18n、无 OS 操作权限
  apps/desktop/src-tauri/        # Tauri 生命周期、受限 command、capabilities
  crates/domain/                # 状态、值对象、错误，禁止 OS 依赖
  crates/orchestrator/          # 单写者状态机、事务、退出屏障
  crates/detection/             # 信号聚合、去抖、规则解释器
  crates/window-engine/         # 过滤、布局、迁移、恢复、救援
  crates/platform/{win,mac,x11,wayland,fake}/
  crates/storage/               # journal、配置、迁移、恢复记录
  crates/distribution/          # updater、规则更新、license 验签
  crates/diagnostics/           # 本地事件、脱敏、显式导出
  rules/                        # 已审核规则源、fixture、兼容范围、签名清单
  tests/{contract,scenario,platform}/
  packaging/{windows,macos,linux}/
  docs/spec/
```

| 模块 | 输入 → 输出 | 责任与错误隔离 |
|---|---|---|
| Session/Display Adapter | OS 事件 → 会话身份、拓扑 generation、权限 | 不根据工具品牌推断拓扑；拓扑变化使旧计划失效 |
| Signal Adapter | 有界读取 → 带时效与来源族的 Evidence | 区分 true / false / unknown；权限失败不是断线 |
| Detector | Evidence + 固定规则版本 → Candidate/Connected/Disconnected/Unknown | 不直接移动窗口，不持有特权 |
| Coordinator | 检测/用户/系统事件 → 串行事务 | 每 OS session 单实例；取消令牌与 operation_id；恢复优先于新迁移 |
| Window Backend | 枚举/快照/请求/读回 → typed Result | 一次请求不等于成功；每窗口独立 deadline；不在 UI 线程阻塞 |
| Layout Planner | 快照、约束、模板 → 纯函数 Plan | 不调 OS；确定性输出；支持 Fake/property tests |
| Journal | preimage/intents/results → durable ack | 未 ack 禁止变更；磁盘满立即停止新动作 |
| UI Bridge | 用户意图 → 校验后的 command / 只读状态 | 不接受任意 HWND、PID、文件路径或 shell；命令引用核心签发的短期对象令牌 |
| Update/License | 签名材料 → update intent / entitlement | 不得绕过恢复屏障，不改变免费安全能力 |

Rust 协调器按优先级处理：恢复/锁屏/权限撤销 > 拓扑失效 > 用户取消 > 已确认连接 > 低优先级观测。队列有界 1,024 条；同类位置/拓扑事件合并。溢出、observer 断连或生命周期事件缺口使受影响窗口的 generation 连续性失效：停止其原位写入，完整重枚举仅重建当前视图，旧布局匹配降为候选确认，不能用重枚举掩盖期间的句柄复用。每会话互斥锁阻止两个实例写同一窗口。macOS 的 AppKit/AX 调用线程约束由适配器承担 `[待验证]（V-MAC）`，不可把异步 runtime 当成任意 OS 调用线程。

内部前后端契约草案：`GetStatus`、`PreviewPlan`、`BeginManualSession`、`ChangeTarget`、`RestoreNow`、`RescueOffscreen`、`SetSettings(expected_revision)`、`ExportDiagnostics`、`RequestExit`。这些是本产品命令，不是 Tauri 内置参数。变更命令均有 request_id、配置 revision、预期 state_revision；过期请求返回 `Conflict`。UI 只获统计数量和显示器用户别名，窗口标题默认不进入事件流。

## 3. 核心数据结构草案

以下 Rust 风格仅为类型契约，不是实现代码；`Opaque*` 由平台适配器封装，不直接序列化原始指针。

```rust
enum EngineState { Idle, Armed, Detecting, Migrating, Active, Restoring, Degraded, Error }
enum TopologyKind { Mirror, Session, Unknown }
enum EvidenceValue { True, False, Unknown }
enum RestorePolicy { RespectUserChanges, RestoreOriginal }
enum WindowMode { Normal, Maximized, Minimized, NativeFullscreen, BorderlessFullscreen }
enum RecordPhase { Prepared, Applying, Active, Restoring, Pending, Closed }
enum WindowOutcome { Applied, Skipped, ExactRestored, FallbackVisible, RespectedUserChange, WindowClosed, Unresolved }

struct SessionKey { user_scope: OpaqueUserId, os_session: OpaqueSessionId, boot_id: OpaqueBootId }
struct WindowKey {
    session: SessionKey, process_id: u64, process_birth: OpaqueBirthId,
    native_id: OpaqueWindowId, observed_generation: u64, local_uuid: Uuid
}
struct DisplayKey {
    device_path_digest: Option<String>, edid_digest: Option<String>,
    connector_hint: Option<String>, identity_quality: IdentityQuality
}
struct LogicalRect { x: f64, y: f64, width: f64, height: f64 } // 相对显示器工作区的逻辑单位
struct DisplaySnapshot {
    key: DisplayKey, native_bounds: NativeRect, work_area: NativeRect,
    scale_x: f64, scale_y: f64, primary: bool, topology_generation: u64
}
struct WindowSnapshot {
    key: WindowKey, display: DisplayKey, normal_rect: LogicalRect,
    original_native_rect: NativeRect, native_placement: Option<OpaquePlacement>,
    mode: WindowMode, topmost: bool, z_rank: u32, desktop: OpaqueDesktopId,
    captured_at_utc: Timestamp, captured_at_monotonic: Duration,
    fingerprint: NonContentFingerprint, minimum_size: Option<LogicalSize>
}
struct WindowRecord {
    before: WindowSnapshot, last_applied: Option<ObservedWindow>,
    user_modified: bool, phase: RecordPhase, outcome: Option<WindowOutcome>,
    attempt: u8, last_error: Option<ErrorCode>
}
struct SessionJournal {
    schema_version: u32, transaction_id: Uuid, operation_sequence: u64,
    session: SessionKey, topology_generation: u64, rules_digest: String,
    config_revision: u64, records: Vec<WindowRecord>, checksum: String,
    phase: RecordPhase, pending_reason: Option<ErrorCode>
}
struct Evidence {
    adapter_id: String, correlation_group: String, value: EvidenceValue,
    observed_at: MonotonicTime, expires_at: MonotonicTime,
    direction: ConnectionDirection, local_session: SessionKey, proof_ref: String,
    connection_key: ConnectionKey, connection_epoch: u64, lifecycle_sequence: u64
}
struct ConnectionKey { adapter_id: String, source_id: String, aggregate: bool }
struct CapabilitySet {
    enumerate: bool, read_rect: bool, move: bool, resize: bool,
    observe_changes: bool, same_session: bool, restore_state: bool
}
```

所有矩形必须 finite、宽高正数，转换后限制到平台整数范围；NaN/溢出/负宽拒绝而非截成巨型窗口。配置/journal 版本单调升级；上一个版本必须可读新版本写出的最小恢复 preimage，破坏性迁移禁止在未关闭事务时执行。

**窗口身份**：运行时 `native_id + process_birth + session + observed_generation` 抵御句柄复用，窗口销毁即封存记录。不能把标题、应用名或 PID 作为稳定身份。即使 HWND + PID + 进程创建时间仍可能在同进程内复用 HWND；macOS AX 也无通用跨重启稳定窗口标识。核心崩溃后无法证明连续性的匹配仅是候选，用户确认预览后才能套用旧布局；通用越界救援可独立运行，不认领旧窗口。不会启动已经关闭的应用或重建窗口。

**显示器身份**：优先 device path + EDID 摘要，连接端口作消歧辅助；不按枚举索引恢复。EDID 可能缺失、克隆、重复、经 KVM 改变；macOS display UUID 与 X11 output 的持久性 `[待验证]（V-DISPLAY）`。身份低置信或多个匹配时不猜，使用当前主屏可见区域并提示用户映射。原始 EDID/设备序列不出设备。

已核验的三平台身份方案（API 名与语义见 [verify/windows.md §3](../verify/windows.md)、[verify/macos.md §3](../verify/macos.md)、[verify/linux-detection.md §2](../verify/linux-detection.md)）：

| 平台 | 主键 | 消歧 | 已知缺口 |
|---|---|---|---|
| Windows | `DisplayConfigGetDeviceInfo(DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME).monitorDevicePath`（形如 `\\?\DISPLAY#DELA1B2#5&...&0&UID4352#{...}`） | `edidManufactureId`+`edidProductCodeId`（仅 `edidIdsValid` 时有效）、`connectorInstance`、分辨率/物理尺寸 | `HMONITOR` 不可持久化（`WM_DISPLAYCHANGE` 后可能失效）；同型号双屏 EDID 相同，必须叠加 device path；换端口后 path 变化 → 需按 EDID+分辨率回退匹配 |
| macOS | `CGDisplayCreateUUIDFromDisplayID`（ColorSync，10.13+）与其逆 `CGDisplayGetDisplayIDFromUUID` | `CGDisplaySerialNumber`/`CGDisplayVendorNumber`/`CGDisplayModelNumber`（是**函数**，非字典键） | 厂商未编码时序列号返回 0；`CGDisplayIsMain` 语义是"菜单栏所在屏"，不是"被捕获屏" |
| Linux X11 | RandR output 的 EDID property（`XRRGetOutputProperty`）+ connector 名 | RandR 1.5 `XRRGetMonitors` 的几何 | EDID 常缺失；`_NET_WORKAREA` 是每 desktop 单一工作矩形，多屏不可靠 → 用 RandR monitor geometry 保守求交 |

跨平台共同规则：身份键必须是 (稳定 ID, 分辨率, 相对位置) 三元组，任一不一致即降级为"候选 + 用户确认"，不得自动原位恢复。

## 4. 核心状态机与并发语义

启动先检查 journal，再开放自动检测。状态是会话协调器的唯一权威；托盘映射见 PRD。`Active` 可带 disconnect_deadline 和 paused 标志，不增加一个与 PRD 冲突的顶层状态。

| 当前 | 事件与条件 | 下一状态 / 动作 |
|---|---|---|
| Idle | 用户启用，能力满足，无未处理 journal | Armed；订阅低成本信号 |
| Idle | journal 有待恢复事务 | Restoring 或 Degraded（身份/权限不确定）；先处理旧事务 |
| Armed | 新鲜候选、未在冷却、同会话 | Detecting；不触碰窗口 |
| Detecting | mirror 达门槛持续 2s、双独立来源、目标有效、用户已启用自动 | Migrating |
| Detecting | session 已确认且有越界候选 | Migrating；执行 rescue 子模式，不集中所有窗口 |
| Detecting | 候选消失、仅 unknown、dry-run、过期 | Armed；dry-run 记录 would_trigger，不行动 |
| Migrating | journal 写入成功、计划仍有效 | 逐窗执行；至少一项已应用则 Active；全为安全跳过则 Degraded |
| Migrating | 用户取消、拓扑变化、已确认断开 | Restoring 已应用子集；未应用的不动 |
| Migrating | preimage 持久化失败 | Error；零新增窗口变更 |
| Active | 未暂停，明确断开且 active connection set 为空 | 保持 Active，启动 15s 延迟；重连则取消计时，复用原始快照 |
| Active | 未暂停且15s到期，或用户恢复、普通禁用、退出、更新、卸载请求 | Restoring；停止新动作 |
| Active | 显式“保持布局并暂停检测” | 保持 Active，paused=true；取消断线计时、停止新自动整理，保留 journal、手动恢复和紧急可见性救援 |
| Active(paused) | 用户恢复检测 | paused=false，重新取得新鲜连接/权限/拓扑证据；仍活跃则保持 Active，明确全部断开才开始15s计时，unknown则Degraded；暂停时长不作断线证据 |
| Active | 目标切换 | Migrating；只新增操作 intent，不覆盖最早 before |
| Active | 丢失信号/读取权限、连接状态 unknown | Degraded；保留窗口、取消自动断线推断；给出“无法确认连接状态” |
| Active | 显示器移除/分辨率变化 | 取消过期计划；在可操作桌面中先救援不可见窗口，再按策略进入 Active/Degraded |
| Restoring | 已应用窗口均恢复/用户修改保留/已销毁，且可见性验证完成 | journal Closed；Armed 或 Idle；冷却 30s |
| Restoring | 拒绝/身份不明/无显示器/锁屏 | Degraded；保留 Pending；用户/权限事件后重试 |
| Degraded | 仅检测 unknown，重新证实同一连接仍活跃，权限/拓扑有效 | Active；沿用原事务；不把信号恢复当作断线恢复 |
| Degraded | 原因是权限/拓扑暂停，连接仍活跃且没有 restore intent | 重验证当前计划和可见性后 Active；失效对象保留异常，不套旧计划 |
| Degraded | 已有显式 restore intent、启动旧 journal，或最后连接明确断开且15s到期 | Restoring；不直接启动新会话 |
| Degraded | 能力恢复，无 pending，用户启用 | Armed |
| Error | 存储损坏、校验失败、内部不变量破坏 | 禁止新迁移；提供只读诊断与通用救援；修复后显式重试 |
| 任意 | 锁屏、用户切换、休眠 | 冻结写入并刷 journal；解锁/恢复先重取 session、权限和拓扑，不复用过期 token |

`connection set` 键为适配器给出的本地连接标识（若无可靠连接标识则该工具仅能报告聚合连接）。同一工具多客户端、多个工具连接不能简单用最后一个断开事件决定恢复。v1 有冲突时保持首个目标并提示，用户可手动切目标；Pro 的仲裁支持优先级/最近用户选择，仍是**单一共享桌面、单个实际目标**，不提供一个窗口同时处于两处。自动热切目标默认关闭。

事件优先级稳定、操作序号递增。协调器持有 degraded_reason、restore_intent 与 paused，不能仅凭 journal 非空决定恢复。连接的 start 建立新 epoch，end 必须匹配 key/epoch/sequence；缺少可验证连接ID的适配器使用 aggregate key，只能据已验证的“全部断开”状态清空，不能对每条不明end减计数。恢复期间新连接先记入集合，等恢复完成再建立新事务；不会对同一窗口并发执行迁移与恢复。短暂信号丢失与明确断线分开，单条旧日志不得结束新连接。用户手动恢复时，把当前连接 key/epoch 标记为本周期已人工结束；30s冷却到期也不能被同一驻留证据再次触发，只有已验证的新连接边缘或用户明确重新开始才解除。手动触发事务没有可验证连接生命周期时，由用户恢复，不用“没有检测到来源”推断断开。

## 5. 检测、规则 DSL 与工具适配

### 5.1 信号与保守融合

| 信号 | 用途 | 不得推导的结论 |
|---|---|---|
| OS session protocol/状态 | Windows RDP 当前会话强证据；当前会话作用域 | `SM_REMOTESESSION` 为 false 不排除 TeamViewer 等 mirror |
| 签名进程/安装信息 | 缩小适配器范围、绑定日志来源 | 进程存在、服务启动不代表有人已连入 |
| TCP listener / established | 关联进程的新鲜辅助证据 | listener、云中继心跳、软件更新、出站控制别人的连接不能当入站屏幕控制；UDP/QUIC 不可用 TCP 完整覆盖 |
| 已验证日志 start/end | 方向、连接身份与生命期 | 日志路径、语言、轮转格式、字段均按工具版本验证；不得拿历史 start 触发新会话 |
| 窗口 class/title | 局部交叉验证（用户可关） | 标题可能本地化/伪造并含隐私，不能单独触发；不上传原文 |
| 虚拟输入设备 | 特定适配的弱辅助 | 设备存在不是正在远控；不装键盘钩子、不记录按键 |
| 锁屏/切会话/拓扑 | 作用域失效、暂停、救援触发 | 解锁不等于远程登录，热插拔不等于远程开始 |

评分为**启发式分数而非统计概率**：只统计未过期、已验证的 Evidence；同一 correlation_group 取最大权重，再求和并 clamp 到 [0,1]。mirror 自动触发要求 score ≥0.90，至少两个经审核独立来源族，至少一个能证明“入站且处于交互连接中”的信号；纯 process + TCP 心跳组合永远不合格。两种读取同一日志的规则不得算双信号。unknown 不计分也不是 false。

Windows session rescue 有明确例外：OS 的当前会话 RDP 协议/活动状态构成一个 authoritative provider，加上本会话越界窗口的空间谓词可直接触发救援；不把 WTS 和 SM 两次读数伪装成独立证据，也不得据此执行 mirror 集中。规则必须声明该 guard 类型。

默认 2s 连续确认、断开 15s、恢复后 30s 冷却为产品设计值，不是远控软件参数；M0/M2 调优后同版本跨文档统一。手动操作可跳过检测去抖，不能跳过权限、范围、journal。按进程白名单“该进程连接时总是触发”必须绑定已验证的**入站连接事件**；仅进程存在时界面显示“无法验证连接，仅可手动”，不把用户白名单当作协议事实。

事件优先，空闲 5s 低频兜底核对；候选态最多 500ms 检测一次；断开使用单调时钟。日志首次附加从 EOF 起，记录文件身份与 offset，轮转重新定位；截断不重放旧 start；恢复后的旧日志需当前连接证据才能再启用。单次 tail ≤64KiB，总缓存 ≤2MiB；单行 ≤8KiB；正则禁止回溯型执行和任意脚本。

### 5.2 规则包格式与 JSON Schema 草案

传输 envelope（自定义格式）：`key_id`、`payload_base64`、`signature_base64`。对解码后的**原始 UTF-8 payload 字节**验 Ed25519 签名，再解析下列 schema，不做会改变签名输入的 JSON 重新序列化；生产密钥不与 updater/license 共用。总包 ≤512KiB、规则 ≤128、每条信号 ≤8。验签算法库/版本在 M0 安全依赖审查锁定 `[待验证]（V-RULE）`。

以下为 payload 的完整结构草案，字段是 Unfy 自定义；`format` 必须开启严格验证。JSON Schema 外仍需本节的语义校验。

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "UnfyRulePayloadV1",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema_version", "pack_id", "sequence", "issued_at", "expires_at", "min_client", "rules"],
  "properties": {
    "schema_version": {"const": 1},
    "pack_id": {"type": "string", "maxLength": 80},
    "sequence": {"type": "integer", "minimum": 1},
    "issued_at": {"type": "string", "format": "date-time"},
    "expires_at": {"type": "string", "format": "date-time"},
    "min_client": {"type": "string", "maxLength": 32},
    "rules": {"type": "array", "minItems": 1, "maxItems": 128, "items": {"$ref": "#/$defs/rule"}}
  },
  "$defs": {
    "signal": {
      "type": "object", "additionalProperties": false,
      "required": ["adapter", "kind", "group", "weight", "ttl_ms", "verification", "locator", "predicate", "evidence_ref"],
      "properties": {
        "adapter": {"type": "string", "maxLength": 80},
        "kind": {"enum": ["os_session", "process", "tcp", "log", "window", "input_device"]},
        "group": {"type": "string", "maxLength": 40},
        "weight": {"type": "number", "minimum": 0, "maximum": 1},
        "ttl_ms": {"type": "integer", "minimum": 100, "maximum": 30000},
        "verification": {"enum": ["pending", "validated"]},
        "locator": {"type": ["string", "null"], "maxLength": 256},
        "predicate": {
          "type": "object", "additionalProperties": false,
          "required": ["field", "op", "value"],
          "properties": {
            "field": {"enum": ["present", "inbound_active", "current_session_rdp", "current_session_active", "disconnected"]},
            "op": {"const": "eq"},
            "value": {"type": "boolean"}
          }
        },
        "evidence_ref": {"type": "string", "maxLength": 300}
      },
      "allOf": [{
        "if": {"properties": {"verification": {"const": "validated"}}},
        "then": {"properties": {"locator": {"type": "string", "minLength": 1}}}
      }]
    },
    "rule": {
      "type": "object", "additionalProperties": false,
      "required": ["id", "tool", "platform", "tool_versions", "mode", "topology", "guard", "threshold", "min_groups", "on_ms", "off_ms", "cooldown_ms", "target", "signals"],
      "properties": {
        "id": {"type": "string", "pattern": "^[a-z0-9._-]{1,80}$"},
        "tool": {"type": "string", "maxLength": 80},
        "platform": {"enum": ["windows", "macos", "linux-x11", "linux-wayland"]},
        "tool_versions": {"type": ["string", "null"], "maxLength": 160},
        "mode": {"enum": ["disabled", "dry_run", "auto"]},
        "topology": {"enum": ["mirror", "session", "unknown"]},
        "guard": {"enum": ["two_independent_inbound", "authoritative_session_rescue"]},
        "threshold": {"type": "number", "minimum": 0.9, "maximum": 1},
        "min_groups": {"type": "integer", "minimum": 1, "maximum": 8},
        "on_ms": {"type": "integer", "minimum": 2000, "maximum": 30000},
        "off_ms": {"type": "integer", "minimum": 15000, "maximum": 120000},
        "cooldown_ms": {"type": "integer", "minimum": 30000, "maximum": 300000},
        "target": {"enum": ["primary", "user_selected", "virtual_user_selected", "current_session_visible"]},
        "signals": {"type": "array", "minItems": 1, "maxItems": 8, "items": {"$ref": "#/$defs/signal"}}
      }
    }
  }
}
```

语义校验：id 唯一；版本范围可解析且覆盖当前版本；adapter、locator 必须来自客户端内置允许目录；pending 信号不能参与评分；auto 的每个必要信号均须 validated 且 evidence_ref 对应签名审核记录，topology=unknown 禁止 auto 搬窗。kind/group 必须与受信 adapter 的已审核来源族映射一致，规则不能通过改名伪造独立性；`disconnected` 仅作匹配生命周期的结束事件，不贡献正向分数。`two_independent_inbound` 必须 min_groups≥2；`authoritative_session_rescue` 只准 topology=session、目标=current_session_visible、受信 OS adapter，guard 自身强制当前会话活动、未锁屏且窗口确实越界，不能只靠 `current_session_rdp=true`。Wayland 无 move capability 时任何 auto 只能产生通知。包不得降低最低安全阈值、开启 helper、指定任意日志路径或绕过本地用户关闭选项。

日志定位和正则是**随客户端审查的 adapter 配置**：DSL 的 locator 只是其逻辑键；需要改变文件访问范围或原始日志语法时先审查/升级 adapter，避免签名规则包成为任意文件读取器。M1 内置一组 adapter，后续可增加受 schema 约束的声明式解析版本，不能下载执行代码。

包更新：校验签名 → schema/语义/有效期 → 本地 fixture 回放 → 灰度 dry-run → 等 Idle/Armed 原子切换；Active 固定旧摘要。保留两版 last-known-good；sequence 防降级，回滚须独立签名授权和新 sequence；过期包停自动，保留手动和恢复。首次安装时钟异常/过期不猜测有效期，降级并提示校时。kill switch 只能停新自动动作或停特定规则；不能立即撤销恢复能力。

### 5.3 八个真实工具的规则实例（均为待验证种子）

下列数组每一项匹配 schema 的 `$defs/rule`；它不是签名生产包。`adapter`/`locator` 是拟定的 Unfy 适配键，**不是远控厂商 API、日志路径或已知参数**。`null` 表示未确认，`[待验证]` 明确记录在 evidence_ref；所有示例 `dry_run`，禁止不经样本验证直接改为 auto。示例平台仅指第一批验证宿主，不暗示其他平台不支持该工具。

```json
[
  {"id":"teamviewer.win.seed","tool":"TeamViewer","platform":"windows","tool_versions":null,"mode":"dry_run","topology":"mirror","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"primary","signals":[
    {"adapter":"vendor.teamviewer","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：版本、日志路径、开始结束字段、方向与日志轮转"},
    {"adapter":"vendor.teamviewer","kind":"tcp","group":"transport","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：进程归属及入站交互关联，排除云心跳"}]},
  {"id":"anydesk.win.seed","tool":"AnyDesk","platform":"windows","tool_versions":null,"mode":"dry_run","topology":"mirror","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"primary","signals":[
    {"adapter":"vendor.anydesk","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：安装版/便携版路径及入站会话语法"},
    {"adapter":"vendor.anydesk","kind":"window","group":"session_ui","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：类名/标题/无人值守模式与本地化，若不独立则弃用"}]},
  {"id":"rustdesk.win.seed","tool":"RustDesk","platform":"windows","tool_versions":null,"mode":"dry_run","topology":"mirror","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"primary","signals":[
    {"adapter":"vendor.rustdesk","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：固定源码tag对照实际发行日志/安装服务/入站方向"},
    {"adapter":"vendor.rustdesk","kind":"tcp","group":"transport","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：直连/中继与无连接心跳对照"}]},
  {"id":"parsec.win.seed","tool":"Parsec","platform":"windows","tool_versions":null,"mode":"dry_run","topology":"mirror","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"primary","signals":[
    {"adapter":"vendor.parsec","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：host/client角色、日志位置和连接字段"},
    {"adapter":"vendor.parsec","kind":"input_device","group":"input_session","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：是否有与活跃入站绑定的独立信号，持久设备不足以成立"}]},
  {"id":"sunshine.x11.seed","tool":"Sunshine","platform":"linux-x11","tool_versions":null,"mode":"dry_run","topology":"mirror","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"user_selected","signals":[
    {"adapter":"vendor.sunshine","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：源码版本、日志路径、stream start/end及多个Moonlight客户端"},
    {"adapter":"vendor.sunshine","kind":"input_device","group":"input_session","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：运行时独立关联证据；不得只判断设备存在"}]},
  {"id":"nomachine.x11.seed","tool":"NoMachine","platform":"linux-x11","tool_versions":null,"mode":"dry_run","topology":"unknown","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"primary","signals":[
    {"adapter":"vendor.nomachine","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：产品版本、物理/虚拟桌面模式、日志路径与会话身份"},
    {"adapter":"vendor.nomachine","kind":"tcp","group":"transport","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：可观察协议与连接方向，不能套用固定端口"}]},
  {"id":"rdp.windows.seed","tool":"Windows RDP","platform":"windows","tool_versions":null,"mode":"dry_run","topology":"session","guard":"authoritative_session_rescue","threshold":0.9,"min_groups":1,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"current_session_visible","signals":[
    {"adapter":"os.windows.wts","kind":"os_session","group":"os_session_authority","weight":1,"ttl_ms":3000,"verification":"pending","locator":"current-session-wts","predicate":{"field":"current_session_rdp","op":"eq","value":true},"evidence_ref":"WTS API 名称已核验；[待验证] V-WIN：本地fixture、当前会话活动性/重连与锁屏"}]},
  {"id":"screen-sharing.mac.seed","tool":"macOS Screen Sharing","platform":"macos","tool_versions":null,"mode":"dry_run","topology":"unknown","guard":"two_independent_inbound","threshold":0.9,"min_groups":2,"on_ms":2000,"off_ms":15000,"cooldown_ms":30000,"target":"primary","signals":[
    {"adapter":"vendor.apple.screen-sharing","kind":"log","group":"inbound_lifecycle","weight":0.7,"ttl_ms":5000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：系统版本/共享或独立登录模式/日志可读性/事件字段"},
    {"adapter":"vendor.apple.screen-sharing","kind":"tcp","group":"transport","weight":0.3,"ttl_ms":3000,"verification":"pending","locator":null,"predicate":{"field":"inbound_active","op":"eq","value":true},"evidence_ref":"[待验证] V-DETECT：不能把服务监听视为有人控制，CGSession字典不作mirror证明"}]}
]
```

**可靠性是验证前的技术判断，不能充作已测成功率**。升为高/自动必须保存工具版本、平台版本、匿名 fixture、正负样本统计、reviewer 和复核日期。

| 工具/模式 | 当前评级 | 理由、验证后可能提升条件 |
|---|---|---|
| Windows RDP 当前 session | 高（协议判定）；整体产品仍待实测 | 有公开 WTS 协议/会话状态；不表示能观察 console mirror |
| TeamViewer | 低 | 常驻/中继不证明连入，独立日志证据未核实；确认双来源和版本范围后可评中 |
| AnyDesk | 低 | 便携/服务/无人值守路径及语言变化；验证 lifecycle + 独立连接/UI 证据后可评中 |
| RustDesk | 中（可验证性），当前自动关闭 | 可基于具体开源版本审计与回放，但发行配置/中继差异仍要测 |
| Parsec | 低 | 客户端/host 与持久设备容易混淆，TCP不足以覆盖传输；无独立证据维持手动 |
| Sunshine/Moonlight | 中（可验证性），当前自动关闭 | 可核对版本源码与服务生命周期，仍需验证远端切屏和多客户端语义 |
| NoMachine | 低 | 同品牌可有物理共享或独立桌面，必须先确定模式与会话归属 |
| macOS Screen Sharing | 低 | 当前 OS session 信息不是共享连接状态 API；日志权限和模式须分版本验证 |
| ToDesk / 向日葵 | 低 | 未确认公开入站会话接口/日志格式 `[待验证]（V-DETECT）`；v1 默认手动 |
| VNC | 低（泛称）；具体发行版另建规则 | 不能通用判定 mirror 或新 session；服务监听不足 |
| xrdp | 中（可验证性），当前自动关闭 | Linux 登录会话与图形会话映射 `[待验证]（V-LINUX）`；只做当前会话救援 |

捕获屏优先级：本次用户显式选择 > 用户保存的工具+主机规则 > 已验证捕获模式启发 > 主屏。缺失指定显示器不偷偷切屏迁移，先提示并允许手动选择主屏。用户热键循环当前活动屏，托盘显示名称/分辨率/目标序号（序号仅显示用）；屏幕边框预览可关闭。规则“默认目标”不是实时捕获来源；远端切屏对 Unfy 不可见时由热键纠正。虚拟屏存在也必须先在远控软件选择该屏。

**已核验的端口/路径事实**（见 [verify/linux-detection.md §8](../verify/linux-detection.md)，仅列有官方依据者；其余一律不得写入默认规则）：

| 工具 | 默认端口（官方依据） | 日志/会话证据 | 可检测性 |
|---|---|---|---|
| TeamViewer | **无入站端口需求**；出站 TCP/UDP 5938、443、80 回退 | `Connections_incoming.txt` 列成功连接；主日志 `CreatePassiveSession` / `SessionTerminate` 可判起止 | B+（但日志面向排障、1MB 滚动，需版本锁定） |
| AnyDesk | 直连 TCP 7070；防火墙 TCP 80/443/6568、UDP 50001–50003 | `ad.trace` / `ad_svc.trace`；`connection_trace.txt` 格式未公开 | B（格式稳定性不确定） |
| RustDesk | 21118 = `direct-access-port` 默认；服务端 TCP 21114–21119、UDP 21116 | 官方未给客户端日志路径 | C（仅能判运行中） |
| Parsec | **无固定端口**（`network_server_start_port` 默认 0 = 随机）；STUN UDP 3478 | `~/.parsec/log.txt` 存在但无公开会话格式 | C |
| Sunshine | `port` 默认 47989；Web UI 47990 | 官方未列日志文件 | C |
| NoMachine | nxd TCP 4000；nxsshd 4022（Win）；nxhtd 4443；display 7001 | 未公开 | C |
| Windows RDP | TCP/UDP 3389 | 事件日志 `TerminalServices-LocalSessionManager/Operational` | A-（`SM_REMOTESESSION`/`SM_REMOTECONTROL` 可直接判） |
| macOS Screen Sharing | TCP 5900；高性能模式 UDP 5900–5902 | 无公开日志契约 | B（端口 + ESTABLISHED） |
| VNC | TCP 5900+N；HTTP 5800；反向 5500 | 实现各异 | B |

**结论**：除 RDP 外没有任何工具提供“会话开始/结束”的稳定公共 API；B 级证据在工具升级后会静默失效，因此规则必须绑定版本范围并有 fixture 回放，未知版本一律降级为干跑/手动。

## 6. 窗口枚举、布局与迁移算法

### 6.1 合格窗口与状态策略

枚举结果先做硬性安全过滤，再做可配置应用排除。所有 skip 都有 reason code，计入覆盖率，不能伪装成成功迁移。窗口与 owner 的关系构成小型有向图；循环/无法确定归属的关系整组跳过，不用标题推断 owner。

| 对象 | 默认动作 | 例外和理由 |
|---|---|---|
| 非当前 session/user/desktop、Unfy 自身 | 跳过 | 用户不能用规则突破 scope |
| 不可见、零尺寸、cloaked、已销毁 | 跳过 | 最小化窗口单独按正常位置处理，不把其 sentinel 坐标当真实矩形 |
| 工具窗口、IME 候选、通知、桌面/任务栏/dock | 跳过 | Windows `WS_EX_TOOLWINDOW`、Shell 类名等做多因素排除；`Shell_TrayWnd` 是候选识别线索，版本/其他外壳 `[待验证]（V-WIN）` |
| owner 弹窗/模态对话框 | 不当独立布局槽 | 已验证 owner 链则相对移动并约束可见；无法读/写子窗则整组跳过，避免对话框与 owner 分离 |
| UWP/宿主框架窗口 | 部分支持，识别真实可见顶层 | 不承诺 EnumWindows 枚举全部 UWP；不硬编码某个框架类为全部有效窗口 `[待验证]（V-WIN）` |
| DRM、独占全屏游戏、原生 macOS fullscreen Space | 跳过 | 不能通用可靠检测所有 DRM/独占状态；疑似全屏保守跳过，应用白名单单独实测 |
| 无边框全屏 | 默认跳过 | 窗口矩形恰好等屏只是疑似；显式规则且经过应用实测才按普通窗处理 |
| 最大化 | 保存 normal rect 和状态，临时还原再布局 | 多窗网格中保持 normal，恢复时重建最大化；单窗口可在目标屏最大化 |
| 最小化 | 默认保留最小化、不抢显示 | 可安全设置其 normal placement 时更新将来展开位置，否则只记 skip；用户“展开最小化窗口”默认关 |
| 置顶 | 保留 topmost 分类和组内相对顺序 | 不把普通窗口提升为全局置顶；无法保留顺序时报告部分支持 |
| 其他虚拟桌面/Space | 默认跳过 | 不切换用户桌面、不拉走别的 Space 的窗口 |

焦点策略：记录原 foreground，所有能不激活的移动均不激活；结束只在用户 foreground 未改变、原窗口仍有效且 OS 允许时维持原焦点。不得模拟输入、抢焦点或全局锁输入来制造“无闪烁”。z-order 保存为 topmost/normal 两组内的局部偏序，owner 必须在正确层；不承诺重建与新窗口混合后的完整全局栈。

### 6.2 坐标、DPI 与布局

存储采用**显示器局部逻辑坐标**，不对整个多屏全局 x/y 直接除以一个 DPI。设源工作区原点为 `(sx,sy)`，物理矩形 `(x,y,w,h)`，源缩放 `(ax,ay)`，则 local rect 为 `((x-sx)/ax,(y-sy)/ay,w/ax,h/ay)`。到目标屏时以目标工作区原点和目标缩放还原；尺寸优先保留逻辑可用性，再满足工作区和最小尺寸约束。负坐标、竖屏、主屏在中间均必须支持。

Windows 核心采用 Per-Monitor DPI Awareness V2；它从 Windows 10 1703 可用，早于此的 Windows 10 不列入完整兼容承诺，具体最小受测 build 在 M0 冻结。读取 `GetDpiForWindow` 时需注意其返回值受**目标窗口自己的 awareness** 影响，DPI-unaware 窗口可能返回 96，不能拿它当显示器缩放。平台后端还必须获取目标 monitor 的几何/有效缩放，跨 aware/unaware 的正确读取路径 `[待验证]（V-DPI）`，失败时保留物理尺寸并 clamp。官方：[DPI context](https://learn.microsoft.com/en-us/windows/win32/hidpi/dpi-awareness-context)、[GetDpiForWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow)。

**已核验的 Windows 跨 DPI 迁移算法**（API 语义经官方文档核对，见 [verify/windows.md §2](../verify/windows.md)；应用级行为仍在 M0 实测）。在 PMv2 线程上，`GetWindowRect`、`DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`、`GetMonitorInfo` 的 `rcMonitor`/`rcWork` 同为**物理像素**；`DWMWA_EXTENDED_FRAME_BOUNDS` 恒为物理像素且不含不可见 resize 边框。流程：

```text
SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)  // 读他人窗口前显式设置
R_vis = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS)  // 可见矩形
R_win = GetWindowRect(hwnd)                                        // 含不可见边框
d     = R_win.origin - R_vis.origin                                // 每边不可见边框差
dpiA  = GetDpiForMonitor(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI)
dpiB  = GetDpiForMonitor(target_hmon, MDT_EFFECTIVE_DPI)
w2 = round(w1 * dpiB / dpiA);  h2 = round(h1 * dpiB / dpiA)         // 保持视觉大小
fx = (R_vis.left - rcWorkA.left) / width(rcWorkA);  fy = 同理      // 保持工作区内相对位置
x2 = rcWorkB.left + round(fx * width(rcWorkB));     y2 = 同理
SetWindowPos(hwnd, NULL, x2 - d.x, y2 - d.y, w2 + d.w, h2 + d.h, SWP_NOZORDER | SWP_NOACTIVATE)
回读校验；偏差 >2px 且有界重试（见 §6.3、§8.1 自我纠正）
```

`MDT_EFFECTIVE_DPI` 才是含用户缩放的值（`MDT_RAW_DPI` 不含）；`GetSystemMetrics` 不是 DPI-aware，PMv2 线程下应改用 `GetSystemMetricsForDpi`。`AdjustWindowRectExForDpi` 只在需要保持“客户区 DIP 尺寸”时才用，不参与外框视觉尺寸换算。

Windows 同时保留 native `WINDOWPLACEMENT`，其 `rcNormalPosition` 是 **workspace 坐标**（不是屏幕坐标），禁止直接当 screen 坐标传 `SetWindowPos`，否则窗口会逐次偏移。native placement 只交回对应 placement API；独立几何经后端正规转换。只有原显示拓扑/工作区仍一致时才可按已验证路径原样重放 native placement；否则必须按新工作区重算 placement 的几何字段，仅保留允许恢复的状态，不能在安全重映射后又用旧 placement 覆盖坐标。最大化窗口应改 `rcNormalPosition` 并保持 `SW_SHOWMAXIMIZED`，或 restore→move→maximize，不要对最大化窗口直接 `SetWindowPos`（Chromium 会覆写）。官方：[WINDOWPLACEMENT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement)。

macOS 使用 points，截图像素/Retina backing pixels 不参与 AX 窗口定位；AppKit 和 AX/CG 的坐标原点及 y 轴方向由适配器显式转换，拿上下排列屏做 golden test。X11 用根窗口坐标、RandR monitor geometry 与 EWMH 工作区/保留区转换；`_NET_WORKAREA` 是每个 desktop 的工作矩形，不是每monitor专属工作区。v1 对各monitor取保守交集，非对称panel造成过度留白时再评估 dock strut 解析 `[待验证]（V-LINUX）`，绝不把保留区当可放窗口区域。X11 缺少统一 per-window DIP 契约，逻辑缩放默认 1；不能据此声称 X11 从不缩放，RandR transform/桌面缩放存在时读取并测试，未知组合仅位置救援 `[待验证]（V-DPI）`。

| 布局 | 规划算法 | 约束与降级 |
|---|---|---|
| 网格（免费） | 枚举列数 1..min(n,8)，行数=ceil(n/列数)；评分为最小尺寸违反数量优先、空槽数其次、与原长宽比偏离再次 | 工作区边距 12 逻辑单位、槽间距 8；面积相等时保持原显示器位置顺序、原 z-rank |
| 主次分区（免费） | foreground/最近用户指定窗口占 60% 宽度；其余在侧栏网格 | 低于侧栏最小宽度则切均匀网格；比例是设计默认 |
| 保留相对顺序（免费） | 按源显示器空间序与窗中心排序，映射目标槽；不要求所有全局坐标同比压缩 | 避免无序重叠；既有目标屏窗口也参与规划，若用户固定则预留其占用区 |
| 自定义模板（Pro） | 归一化工作区矩形 [0,1] + 优先级 + 应用绑定；导入先静态检验 | 重叠模板必须显式允许；越界、零面积、环形绑定拒绝；失去授权保持当前布局至安全结束 |
| 容量不足 | 按最小可交互尺寸计算容量，选焦点/最近使用者入主槽；其余级联且标题/拖动区可触达 | 不把 100 个窗口压成不可用小格；不会最小化或关闭其他应用；“更多窗口”列表支持定位与下一组 |

最小尺寸来自可用平台提示/最近实测拒绝结果；Windows `WM_GETMINMAXINFO` 是应用约束消息，不承诺能安全同步查询任意卡死进程的所有限制。未知时采用 320×200 逻辑单位规划下限（可在应用配置覆盖），写回后按应用实际约束调整一次。屏幕小于应用最小尺寸时不强制缩到非法尺寸，只保证拖动/系统移动入口可达并报告“尺寸受应用限制”。

### 6.3 迁移伪代码

```text
MIGRATE(trigger, selected_target):
  acquire per-session writer lease; reject stale trigger/session
  refresh capabilities, display topology, lock state
  if locked / cannot enumerate / target absent: degrade without mutation
  capture current foreground and immutable configuration revision
  enumerate -> classify -> group owners -> filter -> capability checks
  if trigger is session rescue: keep only offscreen candidates
  build deterministic plan against current work areas and constraints
  for each newly touched live window: append original preimage (never replace an earlier one)
  durably commit Prepared + plan generation + operation intents
  if durable commit fails: stop, move zero new windows
  for each bounded batch:
    check cancellation, identity generation, permissions, topology generation
    if invalidated: stop batch; restore already changed subset
    record intent before OS write
    normalize state if safe, request rect without activation, preserve topmost grouping
    enqueue asynchronous rect/state readback; do not await settlement before submitting next batch
  settle whole plan within deadline, with per-window outcomes:
    if accepted within tolerance: persist actual last_applied, record Applied
    if rejected or application corrects itself: one bounded adaptive retry
    if still divergent: mark Pending/Skipped; do not fight user/application
  persist Active with per-window outcomes, verify visibility
  publish counts, target and exceptions; never report 100% for skipped/unknown items
```

每批最多 16 窗；deadline 不靠阻塞 UI 来实现。最大一次重试，延迟 150ms；settle 观察默认 1s。`SetWindowPos` 的 `SWP_ASYNCWINDOWPOS` 只是异步投递条件，不是对方已移动的保证。默认不使用 `SWP_NOREDRAW`，因为其要求自行补齐重绘，否则会残影；`SWP_NOACTIVATE`/批处理适用性由后端验证。官方：[SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)。

新增窗口默认保持应用选择；只对不可见新窗口救援，不在 Active 中不断全量重排。用户可点“重新整理”；新纳入的窗口保存其首次纳入时 preimage。应用自己在 1s 后又改位置，与用户修改难以可靠区分：都作为外部修改保护，不持续拉回。

## 7. 持久化、恢复与故障闭环

### 7.1 Journal 存储与原子性

存储逻辑根称 `AppDataRoot`，由平台目录解析器返回，最终 OS 路径 `[待验证]（V-STORAGE）`，不得在实施前猜注册表或安装目录。以下**相对路径为本产品拟定契约**：`state/journal/`、`settings.json`、`rules/last-good/`、`logs/`，均位于当前用户私有数据根。不存在任何第三方工具日志固定路径承诺。

事务采用 append intents + checkpoint：每条记录含序号、长度、schema、payload checksum；先写 preimage/intents 并获得 durable ack，再 OS 变更；读回确认后追加结果。checkpoint 同目录临时文件 → flush 文件 → 原子替换 → 刷新必要目录元数据，保留上一完整版本；Windows/NTFS、macOS/APFS、Linux 本地文件系统的具体 durable replace 调用组合 `[待验证]（V-STORAGE）`，M0 断电/强杀测试是发布阻断项。不能把单次 rename 等同于断电安全。

重放只取校验通过的完整记录，截断尾部保留诊断，损坏 preimage 不执行原位恢复。磁盘满/ACL 错误：停止新动作，保持上一次完整 preimage；如果窗口已被移动但 result 未写入，将其记为“可能已应用”，读回后解决，不能假设未应用。单事务上限 1,000 窗/10MiB；超限拒绝扩大范围，不能通过淘汰尚未恢复记录腾空间。

journal 文件仅当前用户可读写；不保存标题、文档路径、远程 IP、剪贴板或屏幕内容。活动/待恢复记录保留到完成或用户显式放弃原位恢复；正常关闭的 journal 7 天后清理，仅用于诊断。用户放弃需先做可见性检查并说明无法操作的异常；记录解决原因，不假报 exact_restore。卸载流程读取这些状态。

### 7.2 恢复冲突、显示器变化和伪代码

默认 `RespectUserChanges`：写后 settle 完成后，实际 rect/state 与 last_applied 相差超过 4 逻辑单位且持续 500ms，或发现明确外部位置事件，标记 external modification。该标记不宣称区分了真人拖动与应用自调。自身移动通过 operation generation 与期望读回过滤；不能凭一个时间窗口吞掉用户事件。`RestoreOriginal` 是显式选项，仍须遵守当前可见性与身份约束。

```text
RESTORE(reason):
  close admission of new mutations; acquire writer lease
  load last valid journal; durably enter Restoring
  refresh session, permissions and display topology
  for each record that might have been applied:
    if observed destroyed: close record as window_closed; do not recreate
    if identity cannot be proved: keep Pending, offer candidate preview; no old-layout write
    if same user/session is not interactive: defer until its next accessible login
    read current rect/state; map original display by identity, not index
    if policy respects external modifications AND record.user_modified AND current placement is reachable:
       keep current placement; close as respected_user_change
    else:
       if original display uniquely exists: project original logical rect onto its current work area
       else: choose current primary/nearest reachable work area, clamp into visible bounds
       persist restore intent; restore normal rect then supported prior state/topmost ordering
       verify actual placement and reachability; retry once within deadline
       if exact impossible but safe placement accepted: close as fallback_visible
       else: retain Pending with reason
  if all records resolved: write Closed checkpoint and durable ack
  else: Degraded, expose retry/permissions/candidate review; do not erase preimage
```

可达性：普通非最小化窗口优先整个矩形位于某个工作区；最小尺寸不允许时，至少有 64×32 逻辑单位的标题/可拖动区位于工作区且能调用该平台系统移动入口。无标题应用需配置可交互锚点，未知则以整个窗口可见为成功条件；不能以“有 1 像素在屏内”通过。最小化窗口验证 normal rect 的将来可达性。触发 session rescue 的谓词为正常放置区域不满足可达性，不移动仅跨两屏但可操作的窗口。

| 路径 | 必须执行 | 不可能时的结果 |
|---|---|---|
| 正常断开/手动结束 | 15s 断线去抖后恢复，手动结束立即恢复 | Pending + Degraded |
| 普通退出/设置禁用/卸载 | 先拒绝新动作，恢复并刷盘，屏障成功后结束 | 默认阻止完成，提供权限修复/候选确认；用户强制退出仅留 journal，不能称成功 |
| 更新重启 | 下载可后台；安装前恢复屏障 | 未恢复不得静默重启，延期更新 |
| panic | 能执行清理时只做有界 best effort，不能依赖析构保证 | 下次启动恢复先于 Armed；崩溃循环进入恢复专用模式 |
| kill/断电 | 不能执行清理 | 依赖先写盘；重启后身份不明需确认，系统重启后不能认领重建窗口 |
| 注销/系统关机 | 收到事件时尽力在系统允许窗口内恢复并刷盘 | OS 不保证足够时间；下次同用户启动处理，不阻塞关机无限等待 |
| 热拔掉原屏/目标屏 | generation 失效；按当前剩余工作区救援 | 无活动屏时冻结直到屏恢复，不写虚构坐标 |
| 休眠/快速切用户/锁屏 | 刷盘、冻结，恢复后重新验证 scope | 不操作安全桌面或别人的桌面 |
| journal 损坏 | 保留损坏副本，进入 Error；允许独立通用救援 | 不从未校验数据构造窗口写入 |

## 8. 平台 API 清单与可行边界

### 8.1 Windows

API 存在与特定应用兼容分开记录；下表官方来源确认的是接口语义。未逐项核对的组合/标志明确落入验证任务。

| 目的 | API/常量 | 使用边界与依据 |
|---|---|---|
| 顶层枚举 | `EnumWindows` | Windows 8+ 文档范围为 desktop apps 顶层，不保证全部 UWP；[官方](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumwindows) |
| 属性/身份/状态 | `GetWindowThreadProcessId`、`GetWindowLongPtrW`、`GetWindow`、`IsWindowVisible`、`GetWindowRect`、`GetWindowPlacement`、`SetWindowPlacement`、`ShowWindowAsync` | 在锁定 SDK 逐项核对 flags/位宽/线程使用 `[待验证]（V-WIN）`；入口见[窗口函数目录](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-functions) |
| 移动/批量 | `SetWindowPos`、`BeginDeferWindowPos`、`DeferWindowPos`、`EndDeferWindowPos` | 批量支持不意味着事务原子；无激活与读回确认；[SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos) |
| DWM frame/cloaking | `DwmGetWindowAttribute` | `DWMWA_EXTENDED_FRAME_BOUNDS`、`DWMWA_CLOAKED` 对目标 build 的返回与边框换算 `[待验证]（V-WIN）`；[官方](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute) |
| DPI | `GetDpiForWindow`、`SetThreadDpiAwarenessContext`、`DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` | UI 创建前定 manifest；后端不改变别人的 awareness；[进程 DPI 声明](https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process) |
| 显示器身份/拓扑 | `GetDisplayConfigBufferSizes`、`QueryDisplayConfig`、`DisplayConfigGetDeviceInfo` | buffer 可能在热插拔后不足需重试；远程 session 可能拒绝 CCD，改用本会话可见 monitor 枚举；[官方](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig) |
| 工作区/位置事件 | `EnumDisplayMonitors`、`GetMonitorInfoW`、`SetWinEventHook` | 完整 event 常量组合、异进程销毁/位置观察 `[待验证]（V-WIN）`，不注入 DLL；事件丢失时重枚举 |
| OS session | `GetSystemMetrics(SM_REMOTESESSION)`、`GetSystemMetrics(SM_REMOTECONTROL)`、`WTSQuerySessionInformationW`、`WTSRegisterSessionNotification` | `SM_REMOTESESSION`(0x1000) 只表示“本进程关联 Terminal Services 客户端会话”，`SM_REMOTECONTROL`(0x2001) 表示会话正被 shadow；`WTSClientProtocolType` 0=console、2=RDP。**已核验：无任何证据表明 TeamViewer/AnyDesk/RustDesk/ToDesk/向日葵/Parsec/Sunshine 会置位这些标志**（RustDesk 源码仅用它挑选会话，Sunshine 全仓库无 `SM_REMOTESESSION`，其余闭源工具无公开证据），因此该行只覆盖 RDP/shadow 场景，不得作为 mirror 场景的检测依据；[WTS 信息类](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ne-wtsapi32-wts_info_class)、[查询接口](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsquerysessioninformationw)、[检测远程会话的官方警告](https://learn.microsoft.com/en-us/windows/win32/termserv/detecting-the-terminal-services-environment)（RemoteFX vGPU 会误判，需 `GlassSessionId` 兜底）；通知注册参数组合 `[待验证]（V-WIN）` |
| TCP 归属 | `GetExtendedTcpTable` | IPv4/IPv6 读取与 PID 关联；不窥探 payload；[官方](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable) |
| 虚拟桌面 | `IVirtualDesktopManager` | 仅用当前桌面/ID 查询辅助过滤；不承诺创建、枚举/切换所有桌面或可靠搬运其他应用窗口；[接口](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager) |
| M4 单屏/IDD | `SetDisplayConfig`、IddCx | **已核验**：`SetDisplayConfig`/`QueryDisplayConfig` 的官方返回码含 `ERROR_ACCESS_DENIED`（“running on a remote session”）→ 强制单屏只能由 console 会话内进程执行，且 `SDC_APPLY` 不写库、崩溃后拓扑停在少一块屏的状态；正确 flags 为 `SDC_APPLY \| SDC_USE_SUPPLIED_DISPLAY_CONFIG`（`QDC_VIRTUAL_MODE_AWARE` 是 Query 侧 flags，`SDC_VIRTUAL_MODE_AWARE` 才在 Set 侧且仅修饰 `SDC_USE_SUPPLIED_DISPLAY_CONFIG`/`SDC_TOPOLOGY_SUPPLIED`）。驱动侧：IddCx 是 UMDF2 用户态驱动，普通用户机器需 EV 证书 + Partner Center attestation 签名；attestation 不能发布到 Windows Update 零售渠道，Windows Server 2016+ 不接受 attestation 提交，ARM64 社区证据（VDD issue #465：证书链完整仍被 Code Integrity 以 `0x800b0109` 拒绝）显示实际需 WHQL。驱动接口版本与签名路线 `[待验证]（V-VIRTUAL）`，v1 不调用 |

UIPI/完整性级别差异可能阻止观察/操作：**已核验**，低完整性进程对高完整性窗口调用 `SetWindowPos`/`MoveWindow` 返回 `ERROR_ACCESS_DENIED`，且 `ChangeWindowMessageFilterEx` 只放行消息、不改变窗口移动的权限检查，因此“从外部让别人的窗口接受移动”不成立；UIAccess 需 Authenticode 签名 + 安装到安全目录 + manifest `uiAccess="true"`，属于 M4 helper 范围。无 helper 时提高权限的窗口部分支持，失败返回 PermissionDenied/Unsupported；不要求整个 Tauri UI 管理员运行。Session 0 服务不能直接操作用户交互桌面（官方：“hwnd and window manager operations are only effective inside a session”）；RDP 和 console 的同名应用也不能串会话。Windows 11 Snap Layouts 的组/历史并非普通 rect 可以重建，`IsWindowArranged`（无 header/lib，需 `LoadLibrary`+`GetProcAddress`）只能判断是否 snapped，`GetWindowPlacement` 不反映 snap 状态；v1 只恢复几何、可支持状态和局部顺序，Snap 组精确复原不承诺。

**应用自我纠正（已核验）**：Chromium `ui/views/win/hwnd_message_handler.cc` 的 `HWNDMessageHandler::OnWindowPosChanging` 会在 maximized/fullscreen/work-area 变化时直接覆写 `window_pos->x/y/cx/cy` 并置 `ignore_window_pos_changes_`；Electron issue #10862 记录多 DPI 下 `setPosition`/`setSize` 偏差恰为缩放因子。因此迁移必须“写后回读 + 有界重试”，对 `IsZoomed` 窗口走 restore→move→maximize，对全屏窗口跳过。

### 8.2 macOS

| 目的 | API/属性 | 使用边界与依据 |
|---|---|---|
| TCC Accessibility | `AXIsProcessTrustedWithOptions` | 用户在系统设置授权；不改 TCC 数据库；授权提示选项/本地化跳转参数 `[待验证]（V-MAC）` |
| 应用与窗口读取 | `AXUIElementCreateApplication`、`AXUIElementCopyAttributeValue`、`AXUIElementIsAttributeSettable` | `kAXWindowsAttribute`、position/size/minimized 等具体属性支持矩阵 `[待验证]（V-MAC）`；不是所有应用都有可写属性 |
| 写入与超时 | `AXUIElementSetAttributeValue`、`AXUIElementSetMessagingTimeout` | 可拒绝、失效或超时；写后读回，不在全局无限重试；[AX API 目录](https://developer.apple.com/documentation/applicationservices/axuielement_h)、[写属性](https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue) |
| 窗口变化 | `AXObserverCreate`、`AXObserverAddNotification` | 通知支持与 runloop/线程安排 `[待验证]（V-MAC）`，回退有界重枚举 |
| 窗口标识 | `CGWindowListCopyWindowInfo`（`kCGWindowNumber`/`kCGWindowOwnerPID`/`kCGWindowLayer`/`kCGWindowBounds`） | 只用公开键；**`kAXWindowNumberAttribute` 在公开 SDK 中不存在**，需窗口号时用 `_AXUIElementGetWindow`（私有）或 pid+bounds 匹配；CGWindowID 仅用户会话内唯一且会复用；`kCGWindowName` 需 Screen Recording 权限；[必需窗口键](https://developer.apple.com/documentation/coregraphics/required-window-list-keys) |
| 屏幕工作区/缩放 | `NSScreen` 的 `frame`、`visibleFrame`、`backingScaleFactor` | API 与 AX 坐标转换的 SDK 签名/边界实测 `[待验证]（V-MAC/V-DPI）`；[官方入口](https://developer.apple.com/documentation/appkit/nsscreen) |
| 显示器身份 | `CGDisplayCreateUUIDFromDisplayID`、`CGDisplayGetDisplayIDFromUUID`、`CGDisplaySerialNumber`/`CGDisplayVendorNumber`/`CGDisplayModelNumber`（函数，非字典键） | UUID 作为稳定主键（ColorSync 10.13+）；序列号可能为 0；[官方](https://developer.apple.com/documentation/colorsync/cgdisplaycreateuuidfromdisplayid(_:)) |
| 主屏/拓扑事件 | `CGMainDisplayID`、`CGGetOnlineDisplayList`、`CGGetActiveDisplayList`、`CGDisplayRegisterReconfigurationCallback` | 主屏用CG定义；`NSScreen.mainScreen`与焦点有关，不等价于主显示器；重配置回调**不在主线程**且需事件循环运行，Rust 侧须 `run_on_main_thread` 并去重 begin/end 两次回调；[Quartz Display Services](https://developer.apple.com/documentation/CoreGraphics/quartz-display-services)、[显示重配置通知](https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/QuartzDisplayServicesConceptual/Articles/Notification.html) |
| 当前登录环境 | `CGSessionCopyCurrentDictionary`（公开键仅 `kCGSessionUserIDKey`/`kCGSessionUserNameKey`/`kCGSessionConsoleSetKey`/`kCGSessionOnConsoleKey`/`kCGSessionLoginDoneKey`） | 描述调用方 Window Server session，不是“有人在 Screen Sharing”的通用 API；`CGSSessionScreenIsLocked` **非公开键**，不得依赖；[官方](https://developer.apple.com/documentation/coregraphics/cgsessioncopycurrentdictionary()) |
| 窗口状态 | `kAXMinimizedAttribute`、硬编码 `"AXFullScreen"` | macOS **无 zoom（最大化）AX 属性**；`AXFullScreen` 未公开常量，按易变处理；尺寸设置需 size→position→size 两次并回读（yabai/AeroSpace 均如此） |
| 自启/未来 helper | `SMAppService` | macOS 13+ 的 LoginItem/LaunchAgent/LaunchDaemon 注册，仍需用户批准和 TCC；`SMJobBless` 自 13.0 起废弃；[官方](https://developer.apple.com/documentation/servicemanagement/smappservice) |
| 虚拟屏/禁屏 | `CGVirtualDisplay` `[待验证]（V-VIRTUAL）`（私有、无稳定公开契约，App Review 2.5.1 必拒）；`CGDisplaySetDisplayMode` | 模式切换不等于禁用显示输出，不能承诺任意 macOS 强制单屏；私有对象构造/参数不在此杜撰 |

首次授权前不移动窗口。重装、签名/安装位置改变、系统升级可能使授权失效；启动与权限失败时重新检查，保留 Pending 并展示准确修复路径。**已核验**：ad-hoc 签名的 designated requirement 绑定单次 cdhash（Apple TN3127），任何重建/更新都会让辅助功能授权静默失效 → 第一天起就用固定 Developer ID + 固定 bundle id 签名，并把“授权丢失”做成可自检 + 一键引导修复。原生 fullscreen 在独立 Space（Apple 归档文档："Cocoa dynamically creates a space"），AX 只能看到当前可见 Space 的窗口，v1 不尝试退出它；`Displays have separate Spaces` 开/关分别测试（`NSScreen.screensHaveSeparateSpaces` 可读），不修改该系统设置。AX 没有公开通用的 Spaces 搬运方案，跨 Space 不支持；提升为 root 也不构成绕过 TCC 的方案。MAS 路线需沙箱（App Review 2.4.5(i)），沙箱进程能否持有 Accessibility 授权 `[待验证]（V-MAC）`，默认按“不可行”规划，走 Developer ID 直发。

### 8.3 Linux X11 与 Wayland

| 目的 | 协议/API/工具 | 使用边界 |
|---|---|---|
| X11 窗口/栈 | EWMH `_NET_CLIENT_LIST`、`_NET_CLIENT_LIST_STACKING`、`_NET_WM_WINDOW_TYPE` | 先读 `_NET_SUPPORTED`；两者是 SHOULD 而非 MUST，缺失时回退 `QueryTree` 递归；不把 override-redirect 系统小窗当应用 |
| 窗口请求/状态 | `_NET_MOVERESIZE_WINDOW`、`_NET_WM_STATE`、`_NET_WM_DESKTOP`、`_NET_WORKAREA` | **已核验**：`XMoveResizeWindow`/`XConfigureWindow` 在窗口被 WM 重定向时只产生 `ConfigureRequest`，WM 可拒绝执行；移动他人窗口必须发 `_NET_MOVERESIZE_WINDOW` client message（带 `StaticGravity` 才能精确含装饰定位），改状态必须发 `_NET_WM_STATE` client message 到 root，不能直接写属性；最大化/全屏窗口需先清 `_NET_WM_STATE_MAXIMIZED_*`/`FULLSCREEN` 再移动，否则 WM 会把坐标钳回原显示器 |
| 几何/约束 | `XGetWindowProperty`、`XGetWindowAttributes`、`XTranslateCoordinates`、`_NET_FRAME_EXTENTS`、`XGetWMNormalHints` `[待验证]（V-LINUX）` | **已核验**：reparenting WM 下 `GetGeometry` 返回父窗口坐标，不可直接当根坐标；必须用 `XTranslateCoordinates` 或 `_NET_FRAME_EXTENTS`；真实 `ConfigureNotify` 用父坐标，合成事件用根坐标；ICCCM 规定客户端不得在被移动后自行回弹（对我们有利） |
| 多屏 | RandR 1.5 `XRRGetMonitors`、`XRRGetScreenResourcesCurrent`、`XRRGetOutputInfo`、`XRRGetOutputProperty`；`XineramaQueryScreens` | **已核验**：`_NET_WORKAREA` 每 desktop 只有单一工作矩形，多屏不可靠 → 用 RandR monitor geometry 求保守交集；EDID 常缺失，connector/EDID 缺失时不宣称稳定身份 |
| 拓扑更改 | `xrandr` / `XRRSetCrtcConfig` `[待验证]（V-LINUX）` | v1 只读，M4 Linux 禁屏仍非承诺；不写未经核验的命令参数 |
| 自启 | XDG Autostart desktop entry | 文件位置/键与沙箱发行方式 `[待验证]（V-PACKAGE）`；用户自启，不要求 system root |
| 登录/连接 | systemd-logind 或桌面会话适配 `[待验证]（V-LINUX）` | 不编造通用 Linux remote-session API、D-Bus 方法/属性；非 systemd 系统只保留手动 |
| Wayland | compositor 适配能力协商 | **已核验**：`xdg_toplevel.move`/`resize` 是 user-driven 且只作用于自身 surface；`wlr-foreign-toplevel-management` 的 request 全集**无 set_position**（`set_rectangle` 只是应用自报位置提示）；Electron 官方文档承认 Wayland 下 `getBounds()` 返回 `{0,0}` 且无法程序化移动。Xwayland 也不能据此控制原生 Wayland 窗口 |

EWMH 原语与 WM 行为参考[官方规范](https://specifications.freedesktop.org/wm/latest-single/)；Wayland 的 clients 不能获知其他 clients 的全局 surfaces/位置，参见[协议模型](https://wayland.freedesktop.org/docs/book/Protocol.html)。KWin scripting、GNOME Shell extension 只能作为 M4、特定桌面/版本的独立适配器 `[待验证]（V-WAYLAND）`；未安装/被系统升级禁用时回退通知。即便识别了 RDP/session，Wayland 也不能仅凭“会话型”绕开窗口控制权限，offscreen rescue 同样不承诺。

### 8.4 M3 目标支持矩阵

“支持”指设计承诺且必须通过平台门禁后才能发布，不表示本仓库已经实现。

| 能力 | Windows 10/11 | macOS 13+ | Linux X11 | Wayland 通用 |
|---|---|---|---|---|
| 普通顶层窗口枚举/迁移 | 支持；UWP/权限例外 | 部分；AX 授权/应用属性限制 | 部分；EWMH WM 白名单 | 不支持 |
| 网格/主次/目标热键 | 支持 | 部分；不能操作其他 Space | 部分；热键冲突可改 | 窗口操作不支持；托盘/通知部分 |
| 原位恢复/热插拔救援 | 支持；失屏则安全回退 | 部分；权限/身份不足需确认 | 部分；WM/显示身份限制 | 不支持通用救援 |
| session offscreen rescue | 支持当前 RDP session | 部分；需确认当前会话与 AX | 部分；仅可操作的本会话 | 不支持通用救援 |
| mirror 自动检测 | 部分；按工具+版本认证 | 部分；证据不足即手动 | 部分；证据不足即手动 | 部分；只通知 |
| 管理员窗口/安全桌面 | 部分 / 不支持安全桌面 | 不支持绕过 TCC | 不提升 root 操作他人会话 | 不支持 |
| 原生 fullscreen/独占游戏 | 不支持默认自动迁移 | 不支持 fullscreen Space | 不支持默认自动迁移 | 不支持 |
| 其他虚拟桌面/Space | 不支持跨桌面迁移 | 不支持跨 Space 迁移 | 不支持跨 desktop 迁移 | 不支持 |
| 内置虚拟屏、强制单屏 | v1 不支持 | v1 不支持 | v1 不支持 | 不支持 |
| 自启/安全更新/诊断 | 支持；包类型有区别 | 支持；需签名/授权 | 支持；包管理器有区别 | 部分；桌面环境托盘差异 |

## 9. 特权 helper、IPC 与威胁模型

### 9.1 何时引入及安装形态

v1 没有常驻特权 helper。普通权限不可控窗口计入能力缺口；不静默提权 UI。M4 仅在企业/提权窗口需求有样本且外部安全审查通过后引入；安装、启用、撤销均由用户/管理员明确操作。

| 平台 | 可选形态 | 安装、作用域、卸载 |
|---|---|---|
| Windows | 最小服务负责授权与生命周期，实际窗口动作由**该用户交互 session 的受限代理**执行 | 独立签名安装组件，经 UAC/企业部署批准；Session 0 不操作桌面；退出卸载先恢复，再停代理、服务和凭据。代理 token/启动方式、SCM 参数 `[待验证]（V-HELPER）`，不指定编造的服务名或注册表路径 |
| macOS | `SMAppService` 管理独立 daemon/agent；必要动作限其授权范围 | 签名 bundle 与系统批准；daemon 不能凭 root 绕过 Accessibility；在有 TCC 授权的用户代理中操作 AX。实际 entitlement、plist 键与 IPC audit API `[待验证]（V-HELPER）` |
| Linux | 用户 agent 负责 X11；仅驱动/设备安装由独立系统组件承担 | 包管理安装；需要授权时评估 polkit，精确 action ID/规则路径 `[待验证]（V-HELPER）`；root daemon 不读取任意用户 Xauthority，不提供通用命令执行 |

官方 Session 0 边界见 [Interactive Services](https://learn.microsoft.com/en-us/windows/win32/services/interactive-services)。无管理员权限、企业禁止服务或用户拒绝授权时继续免费普通窗口功能，不能启动安装循环。

### 9.2 IPC 契约草案

Windows Named Pipe；macOS 使用系统 IPC（候选 XPC）；Linux 用户 Unix domain socket。具体 pipe/socket 名为运行时随机、当前 session 派生的本产品对象，最终命名/ACL API `[待验证]（V-HELPER）`。UI 到核心与核心到 helper 是两个不同信任边界。

每条帧（自定义字段）：`protocol_version, request_id, nonce, connection_epoch, sequence, session_scope, operation_id, ttl_ms, verb, object_token, expected_generation, payload`；长度前缀、上限 64KiB、每调用方≤20请求/s。允许动作为 `QueryCapabilities`、`MoveKnownWindow`、`RestoreKnownWindow`、`CancelOperation`；不允许 shell、进程启动、任意文件/注册表写、任意目标进程注入。`object_token` 由同会话代理通过自己的枚举签发，不能信任 UI 给的数字窗口句柄。

鉴权成功后 helper 签发随机 128-bit `connection_epoch` 和 challenge nonce，仅在该连接有效；`sequence` 为从1开始严格递增的 u64，重复、缺口、旧 epoch 均拒绝，溢出必须重建连接。`ttl_ms` 为接收完整帧后按 helper 单调时钟计算的排队及执行启动期限，范围1–5,000ms；不得比较两个进程的单调时钟原点，也不解释为墙钟时间。超时、连接断开或 epoch 失效后不再启动新动作；已提交 OS 的动作按 operation_id 读回并恢复，不能声称取消能撤销已执行操作。

建立连接时验证 OS peer 身份、UID/SID、session、完整性级别、受信发行者签名/安装位置，并绑定活跃进程实例；请求里的 caller PID/签名字符串不算证明。签名校验不能替代 peer/session ACL；验证和执行间重新检查 PID/窗口 generation，防 TOCTOU。challenge nonce、单调序号、短有效期防重放；helper 独立校验目标工作区和状态，不信任客户端规划矩形。macOS/Linux peer credential 与签名验证具体 API `[待验证]（V-HELPER）`。

### 9.3 安全边界与缓解

| 威胁/边界 | 失败后果 | 必须控制 / 验收 |
|---|---|---|
| WebView XSS → Rust command | 非授权窗口操作/读文件 | UI 仅本地打包资源，CSP、无远程脚本，Tauri capability 最小授权；commands 验 schema/session/意图；外部链接在系统浏览器；不将通用 fs/shell 权限开放给设置页。依据 [Tauri Capabilities](https://v2.tauri.app/security/capabilities/) |
| 同用户恶意进程伪造 IPC | 假用户命令/未来提权 | peer 校验、受保护端点、每会话对象令牌、操作白名单；同用户已完全被控不是产品可解决边界，但不能扩成管理员权限 |
| journal/路径被替换 | 恢复错误窗口、任意文件写 | 私有目录 ACL、拒绝符号链接/不可信重解析点、文件身份校验；不跟随第三方日志中的路径；原始句柄不从磁盘直接执行 |
| 规则投毒/重放 | 大量误搬/越权读日志 | 独立签名、sequence/期限、受限 DSL、审查 fixture、双信号硬门禁、灰度/停用、客户端固定文件访问范围 |
| 更新 CDN/manifest 投毒 | 任意代码执行 | HTTPS + artifact 签名 + 平台签名；自定义发布策略元数据另签名并包含摘要/版本/通道/架构；不信任仅下载 URL |
| 发布密钥泄漏 | 合法签名恶意更新 | CI protected environment、短时取密、硬件/托管密钥、双人批准；离线 root 批准新公钥/吊销，应用和规则密钥分离；客户端拒绝未经受信 root 授权的密钥替换 |
| 高复杂度 regex/海量日志 | CPU/RSS 超额、错触发 | 固定复杂度解析器、大小/时间预算，异常 adapter 自动停止而不是阻塞核心 |
| 恶意窗口最小尺寸/不断自移 | 无限重试/不可见 | 有界一次重试、readback、生成号、排除并解释；不持续对抗应用 |
| 诊断/遥测外泄 | 标题、IP、文档位置泄漏 | 白名单序列化、原文不落盘、预览导出、匿名遥测 opt-in，见第12节 |
| 许可证服务不可用 | 恢复被锁/误降级 | entitlement 缓存，license 不在安全恢复调用链；失效只禁止新付费策略 |

签名密钥轮换不是一条“远程给新 key”的配置。发行根预置、至少一个离线恢复根、轮换跨两个客户端版本完成；失陷版本已信任的恶意代码无法靠应用内规则完全挽救，应同时撤回分发、公告并发布平台签名修复包。安全/密钥事故责任与演练见路线图。

## 10. 虚拟显示器与强制单屏的阶段设计

| 路线 | 阶段与能力 | 准入/代价 |
|---|---|---|
| 用户已有虚拟屏 | M1/M2 按普通可用显示器允许用户选择 | 不负责安装驱动；必须验证远控软件可捕获且用户已选中；拔掉即触发可见性救援 |
| Windows IddCx/IDD | M4 第一期；创建专用远程输出、分辨率档案、明确生命周期 | 驱动签名资格、WHCP/HLK/目标 OS、GPU、Secure Boot/HVCI、卸载回退均验证；具体 IddCx 版本/回调/构建参数 `[待验证]（V-VIRTUAL）` |
| Linux evdi | M4 单独实验，不成为 X11 主产品依赖 | 内核模块、DKMS、发行版内核更新和 Secure Boot；许可审查 module/library 分开，费用见 roadmap；不能解决 Wayland 全局窗口权限 |
| macOS `CGVirtualDisplay` | 仅隔离实验室、测试数据生成候选 | 私有 API，无稳定公开参数契约；生产与 MAS 不承诺，公证不等于私有 API 得到长期支持 |
| Windows 强制单屏 | M4 高级实验，明确 opt-in、默认关闭 | 先保存原拓扑，验证目标输出；独立 watchdog + 本地撤销热键 + 恢复超时，任一缺失不开放 |
| macOS / X11 强制单屏 | 研究项 | CGDisplay 没有据此确认的通用安全“禁用副屏”方案；X11 RandR 可有输出配置能力但跨驱动恢复 `[待验证]（V-VIRTUAL）`；不在 v1 承诺 |

虚拟屏只增加输出，不复制应用进程、隔离输入或克隆同一窗口；把窗口移到虚拟屏仍然改变本机同一会话窗口位置。若要求物理屏工作区完全不变，需独立 OS 会话/应用实例等另外的产品范围，不能靠 IDD 达成。远控端是否缓存输出列表、支持热插拔、需要重连，必须逐工具验证。

强制单屏实验流程：独立恢复代理已运行 → 持久化 original topology → 明确解释本机副屏会熄灭并确认 → 校验剩余输出 → 应用临时拓扑 → 15s 内用户确认，否则 watchdog 恢复 → 会话结束先恢复显示拓扑再恢复窗口。watchdog 为 M4 新组件，不能作为 v1 已有保障；主进程死、断流、拔掉唯一输出、重启和驱动加载失败均要注入。绝不禁用最后一个可用输出；恢复失败保持当前可见屏并提供系统显示设置指引，不反复重设拓扑。独立恢复代理同样不能保证断电瞬间操作。

微软当前 attestation 文档将该路线描述为测试用途并限制零售 Windows Update 分发；商业驱动不以“EV + attestation”作为充分条件。[官方说明](https://learn.microsoft.com/windows-hardware/drivers/dashboard/code-signing-attestation)。签名路线、第三方驱动再分发、商标、私有 API、evdi 各层许可的法律评估与预算由 [roadmap](roadmap.md) 管理，发布前按锁定版本获得结论。

## 11. 更新、自启、企业配置与授权接口

### 11.1 更新事务

Tauri updater 执行签名 artifact 的下载/安装；签名密钥、`pubkey`/`endpoints`、Windows `installMode` 等配置按锁定插件版本验证后使用 `[待验证]（V-UPDATE）`。官方文档已确认签名是更新机制一部分，但**没有替 Unfy 实现恢复屏障、灰度决策、全平台 A/B 回滚**。[Tauri updater](https://v2.tauri.app/plugin/updater/)。

```text
CHECK -> download into staging -> verify artifact + release policy
      -> Ready (Active/Restoring 时仅等待，不安装)
      -> request maintenance; stop new actions; RESTORE(update)
      -> verify journal Closed + all accessible windows resolved
      -> save compatible settings/journal checkpoint and last-known-good package
      -> invoke the package-specific installer within its privileges
      -> launch new version in recovery-first mode; record health acknowledgement
      -> commit release / hold rollout and enter safe recovery on failure
```

无法恢复时显示“更新已下载，窗口恢复后安装”，不强制结束；用户可延期，安全紧急更新也不能秘密跳过 journal。安装前最后一刻再次检查 generation，避免进入维护后新会话已开始。

| 项目 | 设计契约 |
|---|---|
| 通道/灰度 | stable/beta；稳定安装随机生成的本地 cohort 值做 1%→5%→25%→100%，各档至少观察 24h；不以账号/窗口内容分桶 |
| 发布策略 | 自定义签名元数据含 release_id、sequence、artifact_digest、channel、os/arch/package、min_supported_version、rollout、expires_at；服务端 URL/API 路径 `[待验证]（V-UPDATE）`，不在此杜撰 |
| 更新所有者 | 官网 Windows/macOS 可用应用 updater；Linux AppImage 需可写且实测；deb/rpm、企业管理和商店托管包由各自包管理负责，禁止两个 updater 争写 |
| 静默限制 | 仅已具备安装权限时静默；系统级安装/UAC/MDM 要求不能绕过。精确静默参数由打包产物验证，见 roadmap |
| 安装失败 | 用安装器已验证的事务回退能力；保留签名旧包、配置和 journal。无法自动替换被占用二进制时提供签名修复安装器/人工回退；不承诺插件有原生自动 A/B |
| 新版无法启动 | 外部启动/安装健康记录与上次成功版本关联；若核心还能启动则恢复专用模式；完全无法运行则由修复安装器/包管理回到兼容安全版。该路径必须在升级验收中实测 |
| 防降级与回退 | 常规反回滚 sequence；已知漏洞旧包不可自动再装。优先将 last-known-good 代码作为更高版本修复发行；紧急本地恢复仅接受离线签名 rollback authorization 及目标摘要 |
| 崩溃循环 | 10min 内 3 次未完成健康启动 → 关闭自动动作/自动安装，保留恢复/救援/诊断；启动健康需核心运行 60s 且 journal 可读 |
| kill switch | 签名、有效期、scope；仅暂停新规则/新版本，不远程删数据或撤销恢复。离线节点可能收不到，依靠本地健康保护与包过期策略 |
| 密钥/数据迁移 | 平台代码签名、公证、updater 签名相互不能替代；禁止更新先做不可逆 schema 迁移再指望旧版恢复 |

### 11.2 自启、配置与卸载

Windows 自启入口、macOS `SMAppService`、Linux XDG 自启由用户显式开关，启动时静默检查授权与恢复状态；精确注册表路径、任务名、desktop entry 路径不在本次编造，见 V-PACKAGE。设置“开机启动”实际指该用户登录后启动，不承诺登录前搬窗口。

配置按 schema version、revision 原子保存；默认值 < 用户设置 < 当前会话临时选择；M4 才增加管理员策略对可锁定字段的最高优先级。隐私/安全底线不允许管理员放宽（例如上传屏幕内容、跨 session 操作）。导入预览变更，未知字段拒绝，敏感应用标识仅留本地。多实例配置通过 revision 乐观锁拒绝覆盖。企业策略签名、MDM payload/GPO 注册表地址和静默安装参数统一在 roadmap 标待验证。

卸载必须先与正在运行的各用户实例握手恢复；v1 普通用户安装只负责当前用户。管理员强制卸载或其他用户 session 不可访问时不能宣称全部恢复：保留其最小 journal 与独立说明，提示在对应会话完成恢复后清除。用户数据清理与卸载程序分离确认，不默认删除未解决恢复记录；不读取或删其他远控软件的文件。

### 11.3 License 技术边界

价格、权益与合同是 roadmap 的唯一来源。自定义签名 entitlement 字段草案：`schema_version, license_id, plan, device_public_id, seat_limit, issued_at, paid_until, expires_at, trial_until, features, entitlement_revision, key_id, signature`；此为 Unfy 服务协议设计，不是 Paddle/Lemon Squeezy 参数。`entitlement_revision` 由服务端按同一 license 单调递增；客户端保留最高已接受修订号并拒绝较旧凭证，续订/退款以服务端当前权益为准。该机制不能让未联网且未见新修订的客户端立即知道撤销。支付 webhook 的真实事件名、验签 header、API 路径 `[待验证]（V-LICENSE）`。

客户端只持公钥；服务端校验 MoR webhook 后以 event ID 幂等处理，乱序事件需核对当前订阅版本，不能让旧支付成功覆盖后来的退款。激活/释放按设备额度原子更新；设备标识为安装时随机密钥，不收集硬件序列；重装/丢机可在账号中释放，不要求先联系原设备。客户端不处理银行卡数据，不内嵌支付 secret。

年订阅签名凭证最长 90 天，`expires_at ≤ paid_until + 14天`；trial 不超过 `trial_until`。每天后台刷新，断网退避；到期不终止正在运行的迁移事务，安全结束后禁止发起新付费策略，基础检测/布局/恢复/救援/安全更新保留。离线不能即时撤销已发凭证；本地时钟回拨靠上次服务器时间与单调时钟辅助识别，不能声称完全抗破解，异常先降新付费动作并提供联网修复，不封锁恢复。

## 12. 本地日志、诊断与可选遥测 schema

### 12.1 本地结构化事件

自定义 `LocalEventV1`（JSON Lines 概念，非第三方日志路径）：

| 字段 | 类型/范围 | 内容限制 |
|---|---|---|
| schema_version / ts / monotonic_ms | 1 / UTC时间 / 单调毫秒 | 时钟回拨不影响去抖 |
| level / component / event | 固定枚举 | 禁止把原始错误字符串当 event |
| operation_id / session_local_id | 本地随机 ID | 默认不上传；不含用户 SID/用户名 |
| platform / app_version / rule_pack / rule_id | 版本与已知标识 | 不含工具进程完整路径 |
| from_state / to_state / trigger | 状态枚举 / manual、mirror、session、recovery、update | 不含窗口标题 |
| eligible_count / applied_count / skipped_count / pending_count | 非负整数 | 计数必须可对账 |
| duration_ms / attempt / result / reason | 数值 / 枚举 | result=exact、fallback_visible、respected_user_change、closed、rejected、unknown；reason 来自固定错误表 |
| topology_generation / permission_state | 整数 / 枚举 | 不记录 EDID/原始显示器序列 |

事件表：`detector.candidate/decision/unknown`、`plan.preview`、`journal.commit/failure`、`migration.batch_done`、`restore.done/pending`、`capability.changed`、`update.ready/deferred/failed`、`license.state_changed`、`diagnostic.exported`。规则调试可记录分数/来源族/fixture ID，不记录匹配原文。

本地日志位于 `AppDataRoot` 下的产品拟定 `logs/`，最终绝对位置 `[待验证]（V-STORAGE）`。轮转 5×5MiB 或 7 天（先到者），错误去重限流；当前 journal 不受此淘汰规则影响。`PermissionDenied, Unsupported, WindowGone, IdentityAmbiguous, TargetMissing, TopologyChanged, RejectedByApp, Timeout, StorageFull, JournalCorrupt, RuleInvalid, SignatureInvalid` 为本产品错误码，不是系统返回常量；原生错误数值可本地附加，需平台与调用类型共同解释。

### 12.2 诊断导出

用户显式操作 → 展示清单/预览 → 导出压缩包，默认含 app/OS 版本、能力矩阵、规则摘要、错误计数、去标识日志、匿名化拓扑形状、测试结果；不包含原始 journal 的句柄/PID/窗口指纹，不打包许可证/token、第三方完整日志、机器名、IP、用户名、文档路径或屏幕内容。若用户为排障明确附加第三方日志，逐文件预览与脱敏，标明可能含敏感信息，不能自动上传。

诊断包最大 10MiB，默认导出到用户选择位置；没有硬编码上传 URL。客服上传、保存区域/删除 API `[待验证]（V-PRIVACY）`；建议保留 30 天后删除，并提供提前删除入口。用户不提供日志也可申请退款。

### 12.3 匿名遥测（默认关闭）

`TelemetryV1` 白名单：`schema_version, event, day_bucket, app_major_minor, platform_family, arch, channel, result, reason_class, count_bucket, duration_bucket, rule_public_id, consent_version`。`rule_public_id` 仅允许内置/官方已发布规则的固定 ID，其余统一为枚举 `custom`，不得上传用户自定义 ID；仅 detection_feedback 可带该字段。该事件的 result 为 `true/false/unknown` 三值标签，不能塞入自由文本。不带永久安装 ID、账号、session ID、窗口/进程明文、精确坐标、EDID、IP 字段；网络服务可能看到传输 IP，入口日志必须禁用持久 IP 或短期安全处理并明确告知，不能承诺技术上“从未接触 IP”。按日聚合、粗粒度桶，避免稀有组合再识别。

| event | 允许指标 | 目的 |
|---|---|---|
| migration_summary | 数量桶/延迟桶/结果 | 检查性能及覆盖率 |
| restore_summary | exact/fallback/respected/pending 计数桶 | 恢复质量，不混合成精确原位率 |
| detection_feedback | 用户标注 true/false/unknown 与 rule_public_id | 只作反馈趋势；发布门禁需第13节独立标注研究，不以无投诉代替真负样本 |
| crash_summary | app版本、异常类别 | 不自动上传内存 dump/堆栈中的文件路径 |
| update_summary | 成功/恢复阻塞/签名拒绝/安装失败 | 发布门禁 |

事件≤20条/日，离线聚合队列≤1MiB/7天；撤销同意立即停止并清队列。原始匿名事件建议 30 天、聚合统计 180 天，最终地域/保留期须 V-PRIVACY 验证。付费订单与匿名遥测分库，不通过 license ID 关联；业务必需的订单/反欺诈保留依法律与 MoR 合同另列。该匿名 schema 没有主机/运行会话标识，且计数分桶，不能据此计算精确发布门禁、活跃主机数、主机小时或无崩溃会话率；这些证据来自第13节受控测试与另行同意的公测研究。

## 13. 性能、测试与质量门禁

### 13.1 负载、预算与统计口径

所有数值是**目标而非测量结果**。基准：本地 SSD、8 个逻辑核、16GB RAM，3 屏（含 100%/150%/200% 混合缩放）、100 个可控顶层窗口，设置 UI 关闭，release 构建，无调试跟踪；各目标平台/架构分别报告。正式 reference hardware、OS build、WM/远控版本在 M0 冻结。另测 10/20/500 窗口、慢磁盘/低配/挂起应用，不将它们混入正常样本隐藏瓶颈。

| 项目 | 目标 / 测量边界 |
|---|---|
| 迁移首轮延迟 | p95 <300ms：已确认触发 → 枚举、过滤、规划、preimage 持久化、整个计划100窗的首轮写入请求全部提交；16窗分批仅为调度，不能以第一批完成作为终点。不含2s检测去抖，但必须含journal flush |
| 可见稳定 | 从同一已确认触发起，整个计划100窗均完成读回且矩形/状态稳定的 p95 ≤1s；慢/挂起/拒绝计为超时/失败，不能因未完成就剔除 |
| 首轮分项预算 | 枚举/过滤≤40ms，布局≤20ms，journal≤40ms，提交≤170ms，调度余量≤30ms；总目标<300ms，分项不是声称三平台已达到 |
| 布局复杂度 | 排序 O(n log n)，槽位候选上限8；不得以无界组合搜索分配100窗口 |
| 空闲资源 | 10min暖机后连续30min：平均 CPU<1%（统一换算为单个逻辑核100%），稳定 RSS<120MB；包括核心、WebView/代理等本产品进程树，报告共享页口径，不能只算Rust主进程 |
| UI 打开 | 独立报告峰值与静置后回落；不将 UI 打开峰值冒充后台空闲值 |
| 手动命令响应 | 100ms 内显示已接收/正在处理，不等待 OS 完成才反馈 |
| 检测调度 | 空闲5s兜底、候选500ms；事件驱动优先；单 adapter超预算连续3次则停用并降级 |
| 更新/规则 | 大文件下载不占窗口控制线程；验签/解压有体积上限与取消 |

性能失败处理：先减少重复枚举/跨进程读取、缓存未失效能力、延迟创建 WebView；不得取消持久化来达标。AX 的跨进程序列延迟可能使 100 窗指标无法达到，M0 必须测；达不到则优化、缩小明确标注的支持负载或延期，不发布“已达300ms”虚假声明。

| 质量指标 | M2→M3 门禁、分母 |
|---|---|
| 迁移成功率 | ≥99%，分母为计划提交时已纳入且声明可控的全部窗口；成功必须读回在容差与可达范围。运行时拒绝、超时及失败导致后续未提交的窗口仍计入分母；取消、窗口关闭、拓扑失效分别列原因，不能追溯剔除来达标 |
| 支持覆盖率 | 合格数/用户候选总数独立报告，列出每种 skip；避免只筛到容易窗口后宣称全兼容 |
| 恢复安全终态率 | ≥99.9%，分母为需恢复的已应用/可能已应用窗口；原位、尊重外部改动、失屏安全回退分别计数；Pending不算成功；关闭窗口单列已终止，不冒充原位成功 |
| 精确原位率 | 在原屏存在且无外部修改且身份可证的子集中单列，位置/尺寸容差≤4逻辑单位；不拿fallback覆盖此指标 |
| 误报 | 错误自动动作/全部自动动作≤0.1%；FP/ground truth为无远程的已标注检测决策≤0.1%；同时≥10,000无远程主机小时≤1次误搬。三个分母分别报告，手动动作不算自动误报 |
| 样本 | M2≥100活跃主机；≥10,000合格迁移、≥10,000恢复；≥5,000人工标注/受控回放检测决策；每个启用规则有正负/重连/本地空闲样本 |
| 稳定性 | 有同意上报或实验观测的运行会话无崩溃率≥99.9%；未观测会话不能假定无崩溃 |
| 发布观察 | 连续14天满足门禁，M3再稳定14天；未有足够样本的规则继续dry-run，分平台/工具版本报告 |

误报标签来自受控场景、用户明确反馈或有权限的测试观察，不从网络统计反推真相。报告原始计数、置信区间与平台分层，小样本零错误不等于零风险。

**检测统计单位**：正例按独立、已标注的入站连接 episode 计一次，重连合并规则与第4节一致；负例按预先排定且全程确认无入站远程的30分钟观察窗计一次。每个主机/session/工具/模式在一个观察窗内的重复采样、同一工具多条规则与重复候选合并，任一次错误自动动作即将该负例记为 FP；其余有明确负判决的记 TN。候选每500ms重算不得增加分母；unknown、缺失观测与标签冲突单列，不能计 TN。另列正例漏检率、unknown率及覆盖率，防止通过不作判决获得漂亮误报率。固定时长负例中的无候选状态也要验证观察完整性；分平台/工具/规则版本分别报告，整套测试的标注计划与排除理由在采集前冻结。

**发布证据来源**：受控实验或另行自愿加入的公测研究保存假名 host_id、run_id、精确计数、观察时长、ground-truth标签和退出/崩溃结果；这不是默认匿名遥测，不得沿用匿名遥测同意。研究只采上述质量字段，不采屏幕、标题或原始第三方日志；用途、访问角色、撤回/删除流程与最长90天原始数据保留在招募前经 V-PRIVACY 核准。活跃主机定义为研究窗口内完成≥1小时有效观察且≥1次迁移/恢复闭环的唯一主机；host-hours只累计可证明无远程且观察完整的时段；run为核心启动到正常退出/确认崩溃，未确认终止的run单列unknown，不能当无崩溃。公开报告须给出这些观察缺口，不能从订单量或下载量推断分母。M2/M3的精确门禁由此数据及实验记录计算，TelemetryV1只支持趋势预警。

### 13.2 可自动化测试设计

| 层次 | 必测场景 | 判据 |
|---|---|---|
| Domain + Fake | 8状态所有转移、重复/乱序/过期事件、重连合并、未知不当断线、多连接引用 | 任意序列不跨session、不覆盖最早preimage、不并发写同一窗口；属性测试可重放seed |
| Layout | 负坐标、竖屏、非矩形多屏、重复EDID、极小屏、100/500窗、最小尺寸、DPI变换 | rect finite、可达、确定性、约束满足或有明确降级 |
| 恢复 property tests | 每个journal写入边界/OS写入前后故障、重复恢复、丢结果、窗口销毁/句柄重用 | 重放幂等；不认领新窗口；未解决记录不消失；正常原拓扑round trip误差≤4逻辑单位 |
| 规则 fixture | 仅进程/云心跳/出站连接、双信号相关、旧日志、轮转/截断、重复end、权限拒绝、语言升级 | 不产生错误自动动作；pending/unknown不能提供分数；版本不匹配关闭 |
| Windows 宿主 | 造normal/max/min/owned/topmost/拒绝移动/高完整性/aware与unaware窗口 | 普通与管理员分开；实测WM最小尺寸、Snap边界、UWP、锁屏/快速切换/RDP作用域 |
| macOS | 真实测试应用+AX；外接多屏；`CGVirtualDisplay`私有测试夹具 `[待验证]（V-VIRTUAL）` | 私有夹具只在隔离实验机，不能作唯一凭证；必须有物理屏/真实TCC授权/Space与Retina人工验证 |
| X11 | Xvfb + EWMH WM + Xinerama候选夹具 `[待验证]（V-LINUX）` | 多个X screen不自动等于同根多monitor；确认扩展/坐标语义，不满足则用nested Xorg/RandR与物理屏补测 |
| Wayland | GNOME/KWin原生会话无适配器 | 检测/提示可用，所有move明确Unsupported，无静默假成功 |
| 更新/安全 | 篡改包、重放、过期、错架构、磁盘满、恢复被拒、旧→新→回退、journal schema | 验签拒绝零安装；恢复未完成零静默重启；每平台≥50次升级/故障回退 |
| License | 重复/乱序webhook、离线/过期/时钟回拨、退款/撤销、额度竞争 | 不越额度，不因授权故障禁用安全功能，在线订单与权益可对账 |

### 13.3 CI 与人工 smoke

GitHub Actions 中纯 Rust/Fake、schema、格式、前端类型检查、依赖安全扫描为每 PR 阻断项；依赖和 actions 固定版本/提交摘要。构建矩阵 Windows x64/arm64、macOS x64/arm64→Universal、Linux x64（Linux arm64 先作为后续适配，不隐含已支持）。交叉编译不代替原生运行；交互 GUI、TCC、UAC、真实 GPU/多屏在隔离的自托管测试机/发布实验室运行，PR 不得接触签名密钥。签名/公证/发布另在受保护环境双人审批，详见 roadmap。

发布人工 smoke checklist（每一受支持 OS/架构/WM 的候选版均记录结果）：

1. 新安装无权限→说明→授权→试迁移→撤权；自启关闭/开启，重装/签名更新后授权状态正确。
2. 三屏→单屏远程；远端切屏后用热键纠正；连接结束15s内重连，不产生第二份原始快照。
3. 100%↔150%↔200%、竖屏、任务栏/Dock不同位置、显示器排列负坐标，最大化/最小化/置顶/owner读回。
4. 用户在Active拖动窗口，结束后保留；切“恢复原布局”则恢复；跨Space、游戏、管理员窗口有准确skip。
5. 目标屏拔出、原屏缺失、休眠/唤醒、锁屏、快速切用户，只有当前会话可被修改。
6. 每个阶段强杀核心、模拟磁盘满/损坏journal；重启先恢复；同进程句柄复用不能错误认领。
7. Active时下载更新，恢复被拒则延期；签名错误零安装；回退后journal兼容；崩溃循环不再自动搬窗。
8. 断网/许可过期/退款期间恢复可用；全部包型安装升级卸载，包含无管理员与企业策略锁定情况。
9. zh-CN/en、深浅色、键盘导航、读屏、200%文本、热键冲突、无托盘桌面回退；诊断包逐字段无敏感内容。

## 14. 实现拆分与依赖顺序

| 工作包 | 输入依赖 | 可交付证据 |
|---|---|---|
| T1 平台探针 | V-WIN/V-MAC/V-LINUX/V-DPI | 最小操作矩阵、失败码、100窗与内存基线，不先搭业务后补能力 |
| T2 Domain/Fake | 类型契约/8状态/不变量 | 全转移/属性测试/事件重放 |
| T3 Journal/恢复 | T1身份能力、V-STORAGE、T2 | 中断矩阵、幂等恢复、候选确认、损坏降级 |
| T4 Window Engine | T1/T3、布局纯函数 | 三平台手动mirror与session救援闭环 |
| T5 Detection | V-DETECT、签名schema、T2/T4 | 指定版本规则、正负fixture、干跑→自动证据 |
| T6 UI/权限/诊断 | PRD、T2/T4状态流 | 全P0旅程与中文错误、无障碍、隐私导出 |
| T7 Distribution | T3恢复屏障、V-PACKAGE/V-UPDATE | 包型签名与升级/回退演练、灰度暂停 |
| T8 License/商业 | T6/T7、V-LICENSE/V-PRIVACY | 沙盒购买到退款闭环、离线与幂等证据 |

T1/T2 可并行；没有 T3 的 durable ack 不开放 T4 自动动作；没有 T5 的实测不得把 seed 规则启用。正式实现可按本表拆 issue，但本次不创建实现代码。

## 15. 待验证登记与验证方法

所有 `[待验证]` 都必须在发布时变为“证据链接+目标版本+通过/降级/移出范围”，不是永久免责标记。文档中的第三方参数/日志路径未知项不通过猜测填充。

| ID | 未确认对象 | 验证方法/证据 | Owner / 最晚门禁 |
|---|---|---|---|
| V-WIN | Windows10最低build/arm64、过滤类名、UWP、hook flags、最小尺寸、完整性级别、WTS边界 | 锁定SDK核对头文件/官方文档；在Win10/11 x64/arm64宿主逐状态造窗；RDP/console/快速切用户对照；保存调用与读回结果 | Windows负责人 / M0原型、M2发布矩阵 |
| V-MAC | AX属性、线程/runloop、TCC撤权、NSScreen坐标、Space差异、自启授权 | macOS13与当前受测版本、Intel/Apple Silicon最小AX探针；设置Space开关并恢复；记录API返回值与权限前后行为 | macOS负责人 / M0、M2 |
| V-DPI | monitor有效缩放获取、非aware矩形、X11 transform | 三屏100/150/200%上下左右布局，normal/max/min往返误差测试；未知缩放组合保留物理尺寸降级 | 各平台负责人 / M0 |
| V-DISPLAY | EDID/device path/UUID稳定性、重复显示器、KVM、RDP虚拟输出 | 拔插、换接口、重启、重复EDID、dock/KVM切换；比较身份冲突，证据不足使用用户映射 | 平台负责人 / M1 |
| V-DETECT | 每工具版本/可执行身份、注册表（若确需）、日志实际路径与正则、端口/方向、捕获模式 | 官方文档+固定源码tag（若开源）+已安装软件只读检查；至少20次连/断与本地常驻、出站、心跳、直连/中继、无人值守、语言变化对照；用户授权fixture脱敏；找不到独立证据维持手动 | 检测负责人 / M0候选、M2每条auto规则 |
| V-LINUX | libX11/RandR函数签名、login会话属性、WM、Xvfb/Xinerama启动参数 | 锁定发行版头文件/man page与最小编译探针；同根多monitor验证；至少KWin X11与另一EWMH WM实机；不把多X screen当多monitor | Linux负责人 / M0、M2 |
| V-WAYLAND | KWin/GNOME扩展接口、支持版本/安装权限/更新行为 | 仅M4隔离适配器POC，版本升级、禁用扩展与权限撤销测试；失败回退通知 | Linux负责人 / M4立项 |
| V-STORAGE | AppDataRoot确切路径、ACL、原子替换/flush API与断电持久性 | 目标OS目录API/文件系统实测；逐写入点强杀/断电、磁盘满、符号链接、损坏尾部；没有durable证据不开放自动迁移 | 核心负责人 / M0设计、M1完整 |
| V-RULE | 验签库/版本、fixture格式、parser资源上限 | 官方库文档+已知签名测试向量、fuzz、schema校验、重放/过期/关联信号负样本；公钥轮换演练 | 安全/检测负责人 / M1 |
| V-UPDATE | updater版本/config字段、各包型静默与回退、发布策略URL/字段实现 | 固定Tauri插件版本核对官方文档/源码；安装器沙盒上逐包测试签名错误、恢复阻塞、旧新回退及崩溃循环 | 发布负责人 / M2 |
| V-PACKAGE | 自启入口/注册表路径、安装属性/CLI、MDM profile/GPO key | 生成实际签名产物后用干净VM及企业管理工具验证；每个参数有出处与日志；未验证不写给客户执行 | 发布/企业负责人 / M3或M4对应范围 |
| V-HELPER | peer验证/签名/ACL/token/session代理API、服务安装/卸载 | 三平台威胁建模、伪造peer/重放/TOCTOU/跨会话攻击、独立安全审查；普通权限降级回归 | 安全+平台负责人 / M4 |
| V-VIRTUAL | IddCx回调/版本/签名资格、CGVirtualDisplay私有字段、evdi/DKMS、禁屏flags、watchdog | 驱动官方文档/锁定SDK、法务与分发资格确认；Secure Boot/HVCI/GPU实机、断流/断电/恢复矩阵；mac夹具不进入生产依赖 | 驱动/法务/平台负责人 / M4 |
| V-LICENSE | MoR事件/API/header、税/退款/地区、License服务接口 | 官方合同/文档与沙盒支付/乱序/撤销/退款演练，独立审查服务端验签、幂等与额度 | 商业后端/法务 / M3 |
| V-PRIVACY | 诊断服务地址/区域、遥测基础设施、法定保留期 | 数据流审查、脱敏测试、删除与撤回同意演练、处理商合同；不采屏幕内容列入隐私与回归 | 安全/法务/支持 / M3 |



