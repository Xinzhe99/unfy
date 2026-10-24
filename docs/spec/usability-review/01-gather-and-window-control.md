# 01 · 默认收拢与窗口控制规格

状态：待实施。优先级：提交安全 P0；默认语义及 Windows 兼容性 P1，均为可用版本门禁。用户报告「默认效果不对、很多窗口无法整理」按现存问题处理；下面区分已定位的原因与仍需实机确定的应用差异。

## 1. 已定位的原因

| 源码 | 事实 | 后果 |
|---|---|---|
| `crates/engine/src/lib.rs:607–631`、`crates/layout/src/lib.rs:239–292` | 对所有合格窗口按整个虚拟桌面 bounds 分别缩放 x/y | 目标屏已有窗口也变小；空屏和屏间空白改变尺寸；长宽比变形 |
| `apps/desktop/src-tauri/src/commands.rs:1733–1759` | 重置保存 KeepLayout/15s，live engine 却设 Grid/0s | 界面、实际执行、重启后行为不一致 |
| `crates/platform/win/src/windows_impl.rs:190–206` | `visible==outer && width>=1280 && height>=720` 判全屏 | 普通大号无边框窗可能被排除；小屏全屏反而漏判 |
| `windows_impl.rs:103–170,210–245` | owner 在转成 snapshot 时丢失；工具窗提前过滤；能力全部 true | 模态框可能与主窗分开；固定尺寸窗仍收到 resize |
| `crates/platform/win/src/lib.rs:71–102`、`crates/platform/src/lib.rs:99–111` | WinPlatform 未转发 apply_async，使用同步默认实现 | 生产路径不具备注释声称的异步提交行为 |
| `engine/src/lib.rs:751–818,867–900` | 准备失败跳过快照，提交仍遍历原计划 | 某窗口可能被移动却没有可恢复前像 |
| `engine/src/lib.rs:1043–1057,1111–1147` | 默认重试没有实际等待；只比较矩形；另一次读回不修正 outcome | 过早判拒绝，或成功与最终观测矛盾 |
| `engine/src/lib.rs:879–894` | 所有布局统一要求 Normal | 默认模式也取消最大化 |

这些不是所有失败窗口的完整归因；权限、真实应用回弹、UWP/虚拟桌面与混合 DPI 仍需兼容矩阵。不能把所有失败都归咎于 Electron，或用自动加入排除名单提高成功率。

## 2. 默认行为

默认模式的产品含义固定为：**把其他屏和越界的可控应用窗口移入已确认的目标屏，保留可用尺寸、窗口状态及原恢复位置；目标屏已有且可操作的窗口不动。** 允许窗口叠放，通过系统任务栏/窗口切换访问。默认不是把整个桌面缩成缩略图，也不是保证所有窗口同时平铺可读。

显式网格保留为可选高级排列：用户选择时才允许重排目标屏上已有窗口及将最大化变成普通窗口。删除主次布局及其参数，避免在焦点、固定尺寸和恢复尚未可靠时维护第三套语义；旧配置迁移见 05。不得给主任务添加模板、自动布局评分、兼容性白名单或新的布局开关。

### 2.1 范围和分类

枚举先保留足够事实，再由统一策略分类。对发现的应用窗口，每项必须有一个可解释结果；结构性的系统窗口可按类别汇总，不把数百个隐藏控件倾倒给用户。

| 窗口类型 | 默认收拢 | 恢复责任 |
|---|---|---|
| 目标屏已有且操作区可达 | `Unchanged(AlreadyOnTarget)`，零平台写入 | 没有因本次操作产生的新责任 |
| 其他屏普通窗 | 移到目标，保留逻辑尺寸 | 首次前像持久化后才允许移动 |
| 最小化 | 保持最小化，不展开；报告跳过 | 已有事务内后被最小化视为外部状态变化 |
| 最大化 | 在目标保持最大化，正确更新正常还原位置 | 恢复原显示器、正常位置和模式 |
| 可移动、不可缩放 | MoveOnly；不发送尺寸请求 | 同上 |
| owner/owned/modal | 先组成关联组，不单独占布局槽；可控组同向迁移 | 组成员分别记录前像，保持 owner 层级 |
| 模态组关系/能力不明 | 整组不移动并说明 | 不拆开主窗与不可见对话框 |
| 原生/确认全屏 | 保持原状，说明退出全屏后可重试 | 不自动退出全屏 |
| 疑似全屏 | 保守跳过，原因明确 | 不称为确认全屏或 DRM 检测 |
| 其他虚拟桌面/未知归属 | 不移动；不切换桌面 | 保留已有记录，权限/归属恢复后再处理 |
| 本工具、系统外壳、IME、瞬时菜单 | 不作为工作窗口 | 不创建伪恢复记录 |
| 提权窗口/权限不足 | 能力按真实结果说明，其他窗口继续 | 读取失败不冒充关闭 |
| 主动排除 | 后续收拢跳过 | 已收拢的窗口仍需恢复 |

跨屏窗口归属：以可见矩形和显示器有效区域交叠面积最大者为源屏；并列时用已记录源屏，否则按稳定显示器 ID 排序，不依赖枚举顺序。没有交叠的窗口按到各工作区的最短几何距离选择位置参照，标记越界。目标已有窗口必须同时满足目标归属、可操作区域在目标内；仅有一角落在目标上不算已收拢。

可操作区域：标准标题栏至少 64×32 逻辑单位可见；有可靠自定义拖动区域时使用该区域。未知无边框窗口优先完整移入工作区；超大且不能证明控制区域可达时报告受限，不承诺“可以操作”。`Rect::reachable_in` 的任意区域相交不能直接当成标题栏可达。

### 2.2 逐窗几何算法

输入：原可见矩形 R、源屏工作区 S、目标工作区 T、源/目标逻辑缩放、可缩放能力、可靠的最小尺寸、原窗口模式。先验证工作区非空、缩放有限且大于零。

1. 目标屏已可达窗口直接 Keep；不得创建重新布局请求。
2. Windows 逻辑尺寸转换：`w = round(R.width * target_scale/source_scale)`，高度同理。例如源 150% 下的 1200×900 物理像素，在 100% 目标请求 800×600。不能按显示器总宽度除以屏幕数量。
3. 复用 `domain::rescale_between` 的源屏局部位置约定：`fx=(R.left-S.left)/S.width`、`fy=(R.top-S.top)/S.height`，目标原点为 `T.origin + (fx*T.width, fy*T.height)`，随后夹取。修改该公共函数前检查全部恢复调用方；尺寸约束处理可拆成同模块纯函数，不引入第二套坐标约定。
4. 已知最小尺寸先按其单位、可见/外框基准和测量 DPI 规范化；未知最小尺寸不能把 180×120 的现有小窗放大到 320×200。
5. 仅当可缩放且确实大于目标时考虑缩小。用一个统一比例 k 同时缩宽高，不做 x/y 独立压缩；必须满足可靠最小尺寸。无法同时满足时保留合法尺寸、移入可操作区域，返回 `Adjusted(LimitedByWorkArea)`。固定尺寸窗永远 MoveOnly。
6. 位置夹取基于实际工作区（避开任务栏/Dock），不能用整个显示器 bounds；负坐标、竖屏、上下排列使用同一逻辑。超大窗不要先 max(min) 再 min(target) 产生违反最小尺寸的计划。
7. 默认模式 margin/gap 不参与计算；UI 隐藏。网格可继续使用二者，单位明确为逻辑单位，进入平台计划前转换。
8. 增加/移除一块没有候选窗口的无关屏，不得改变同一源屏窗口到同一目标的结果。

macOS 的 AX/Quartz 桌面点与 backing pixel 不混算：详见 05；不得对点坐标重复应用 Retina 比例。此处 150% 算例只适用于 Windows 的物理像素模型。

### 2.3 状态、焦点与关联窗

- 默认最大化不能只把 `plan.mode` 改成 Maximized：必须调整正常还原位置及目标屏，再通过不抢焦点的经过验证的路径保持最大化。
- 普通窗口采用 `SWP_NOACTIVATE | SWP_NOZORDER`；`ShowWindow(SW_RESTORE/SW_MAXIMIZE)` 会有激活语义，后续 NOACTIVATE 不能抵消。适配层需采用并实测无激活的状态/placement 路径，不用强抢回焦点掩盖问题。
- foreground 作为本次枚举上下文，不通过全局静态变量共享。保留 topmost 分组和 owner 相对层级，不重新排列用户其他窗口的全局 Z 序。
- modal 组先核验全体、全体前像 durable 后提交；提交期间成员消失/拒绝时停止组的后续步骤，已移动成员仍有明确恢复责任。需要补偿时也走相同有记录的写入路径，不能裸调用回滚。

## 3. 原生窗口兼容性修订

### 3.1 Windows

修改 `windows_impl.rs`、`displays.rs`、`win/src/lib.rs`：

- 全屏判定结合实际 monitor bounds、mode/style、几何来源；DWM 读取失败是 Unknown，不能构造「无边框」证据。1600×900 的普通无边框窗位于 1920×1080 屏时不能仅因尺寸被排除。
- 使用公开的 `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` 或等价可验证事实，区分 Current/Other/Unknown；cloaked 是独立事实。对 pinned windows、查询失败、切桌面在执行前重新校验。
- 能力区分 Yes/No/Unknown 及错误原因；不再对所有有效 HWND 声称可移动和可缩放。不要向外部窗口传未经跨进程安全保证的指针型消息数据。
- 身份读写校验覆盖 HWND、PID、真实进程创建时间、当前用户会话及窗口生命周期。见 02；`IsWindow` 不是身份证明。
- `WinPlatform` 显式转发非阻塞提交；不再默默落到同步默认。一个 hung app 不能阻塞其他窗口提交或 UI 状态读取。
- 尺寸约束注明 outer/visible、测量 DPI、可信度。跨 DPI 移动后重新观测 frame/约束；PMv2 与 DWM 可见矩形保留，不把查询失败默认 96 DPI 当成已确认 100%。
- CCD 设备映射通过合法 source mode 索引并核对范围、infoType、adapterId、sourceId，再访问 union；无法唯一匹配降级身份，不误标 Strong。
- probe 与产品使用同一个资格分类输出；当前 probe 单独排除 owner，不能作为引擎实际可移动窗口数。

### 3.2 提交与确认

沿现有 Engine 批次结构改，不引入并行争抢同一个窗口：

```text
inventory = enumerate_once(topology_revision)
plan = classify_and_plan(inventory, target, config_revision)
prepared = validate_identity + fresh_observation + immutable_preimage
persist(prepared); durable_ack()
for each bounded batch from prepared only:
  recheck target/topology/session and each identity
  append intents; durable_ack()
  submit nonblocking (one outstanding operation per window)
  observe results, then optionally retry eligible unresolved items
  append the exact observation that determined each outcome
```

未成功准备的窗口必须有 `NotAttempted(SnapshotUnavailable/IdentityUncertain/StateChanged)`，平台写调用数为零。更换目标、停止、拓扑变化使尚未提交的批次失效；已提交项保持恢复责任。禁止对所有原始 placements 无条件提交。

内部初始预算（待兼容实测校准，不是已经证明的性能）：每窗总写入最多两次（首次＋一次重试）；提交后分阶段读回，观察窗口约 1 秒；确认成功至少两个相隔约 50ms 的一致有效观测。只对可重试的未完成项重发，已挂起请求不得叠加。不把观察超时断言为应用拒绝，记 `TimedOut/Unknown` 并阻止与该窗口后续写入发生交错。迟到完成必须关联 operation_id；没有可靠平台取消时不能假装请求已撤销。

成功验证同时包含矩形约束、模式及实际可操作性。同一份观测生成结果、last_applied 和 journal；不得先判成功，再用另一次相反观测落盘。接受应用尺寸约束后的可用位置是 `Adjusted`，不是精确 `Applied`。用户在过程中干预后停止争抢。

## 4. 数据与结果合同

沿现有类型扩展，不创建平行引擎：

| 类型 | 必须承载 |
|---|---|
| WindowSnapshot/枚举记录 | owner/group、window role、desktop membership、geometry provenance、identity quality |
| Capabilities | move/resize/read 的 Yes/No/Unknown；约束的单位/DPI/来源；typed error |
| Candidate | 源屏工作区/scale、原 mode、可缩放策略、关联组 |
| WindowPlan | Keep / MoveOnly / Place；目标 mode 或 PreserveMode；topology/config revision |
| WindowResult | window_ref、应用名、outcome、reason、requested、confirmed_observed、attempts、恢复责任 |

本次结果只能落入一个主类别：`Unchanged / Applied / Adjusted / Skipped / Rejected / Gone / Unknown / NotAttempted`。跳过和限制原因至少包括 AlreadyOnTarget、MinimizedPreserved、Fullscreen、SuspectedFullscreen、OtherDesktop、DesktopUnknown、OwnerGroupUnsupported、PermissionDenied、Unsupported、IdentityChanged/Uncertain、SnapshotUnavailable、StateChanged、TargetMissing、SizeConstraint、TimedOut、ApplicationCorrected。不存在事实的原因不能伪造。

统计：`本次已发现应用窗口 = 未变 + 成功 + 调整后可用 + 跳过 + 拒绝 + 已关闭 + 未知 + 未尝试`。系统结构性排除另计；平台本身不能枚举到的窗口不伪造分母。成功率不能靠把失败应用永久排除来提高。空计划仍须保留 unplaced/skip 原因。

## 5. 整理期间新窗口

保持既有“新建可见窗口默认不自动全量重排”的范围。事务期间用低成本窗口事件或有界增量枚举发现新窗口：

- 新窗口在任一现有工作区正常可见：不持续追赶；界面提供「收拢新窗口」，重复整理只纳入新窗口及用户明确重试项。
- 新窗口停在不存在的屏幕坐标：已有活动事务、目标已确认且未暂停自动化时，按相同排除/权限策略救援到目标可见区；否则只提示并提供手动救援。不需要先拔插屏幕才能发现。
- 用户主动把已收拢窗口拖回其他屏：视为外部改动，不能每次扫描又强行拉回。
- 每个新增窗口首次移动前追加它自己的 preimage；旧窗口首次基准不覆盖。自动会话也遵循此策略，不新增一个持续抢窗模式。

## 6. 验收

W-01 两屏/三屏/错位/上下/负坐标/竖屏：目标屏已有可达窗零写入；增一块空屏不改变其他窗口尺寸。

W-02 150%→100% 的 1200×900 普通窗、空间足够时成为 800×600；返向符合逻辑尺寸；分别覆盖 DPI-unaware/system-aware/PMv2 应用。

W-03 180×120 小窗保持尺寸；fixed-size 只有移动；minimum 大于目标时不发非法缩小请求、可操作或明确受限。

W-04 最大化保持状态和正常还原位置；最小化不展开；无关窗口焦点/topmost 不改变。

W-05 普通 1600×900 无边框窗不误判全屏；真实 1024×768 全屏保守跳过；DWM 读取失败明确 Unknown。

W-06 模态、禁用 owner、owned tool、关系断裂：不分离窗口组；跳过理由可见。

W-07 一个窗口准备失败、另一个准备成功：失败项零平台写入，成功项可收拢并恢复。

W-08 慢消息泵、挂起、迟到完成、首次命中后回弹、仅 mode 错误：结果准确，单窗至多两次写入，没有同步阻塞整批。

W-09 第一批提交后拔掉目标/切换虚拟桌面：后续不写旧坐标；已提交项有记录。

W-10 新可见窗不被抢走；新越界窗可救援；再次整理只追加新前像，旧基准仍有效。

W-11 全部未放置或混合结果计数守恒；预览、手动、托盘、自动使用相同原因。

W-12 重置后立即预览/执行/重启行为一致；产品默认必须进入测试输入，不能继续用 Grid 的库默认冒充默认模式验收。

W-13 多显卡、重复 source id、目标/源 mode 表混排不会误读 union 或错误认领显示器。

真实兼容最小集：Explorer、Edge/Chrome、VS Code/一个 Electron 应用、Windows Terminal、一个原生编辑器、带模态保存框的应用、固定尺寸工具、提权实例、一个实际 UWP/打包应用。记录版本、OS build、DPI、前后矩形/模式、焦点、结果原因；不能只用六个同类自建窗口宣布兼容完成。
