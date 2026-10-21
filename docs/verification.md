# 验证记录

本文只记录**实际执行过**的检查与结果。未执行的项目明确写"未验证"，不作推断。

日期：2026-10-21 · 主机：Windows 10.0.26200 · 双屏 1920×1080 并排（`\\.\DISPLAY1` 在 0,0，`\\.\DISPLAY2` 在 -1920,-78）

## 自动化测试

| 项 | 命令 | 结果 |
|---|---|---|
| 单元与集成测试 | `cargo test --workspace` | 95 passed, 0 failed |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all -- --check` | 通过 |
| 前端类型 | `npx tsc --noEmit` | 通过 |
| 前端构建 | `npm run build` | 通过（245 kB JS / 19 kB CSS） |
| macOS 编译 | `cargo check -p conflux-platform-mac --target aarch64-apple-darwin` | 通过（仅类型检查，未在 Mac 运行） |

测试分布：domain 14、platform(fake) 4、journal 12、layout 18（含 4 个列数选择用例）、engine 25（含 3 个退出屏障与 4 个暂停/延迟用例）、settings 13、win 1、mac 4。

## 真实桌面验证

### 跨进程移动

```
probe --move
before: visible 626x473+137+130  outer 640x480+130+130
observed: 626x473+-1783+52  delta=[0,0,0,0]  PASS
restore delta=[0,0,0,0]  PASS
```

第一次运行时 delta 为 28 px，原因是不见边框（inset）方向写反；修正后为 0 px。

### 端到端迁移与恢复（`e2e` 二进制）

```
source \\.\DISPLAY1   1920x1032 @ 0,0
target \\.\DISPLAY2   1920x1032 @ -1920,-78
captured 6 windows
migration: applied=7 rejected=5 skipped=7 (desktop-wide)
migration PASS: every window is inside the target work area
restore: complete=false pending=2 (ours=0 others=2)
restore PASS: every window returned to its original rectangle (tolerance 4 px)
journal PASS: unresolved transaction kept 12 preimages for later recovery
E2E PASS (6 windows, real desktop)
```

`applied`/`rejected`/`skipped` 覆盖整块桌面的合格窗口，因为引擎的正确行为就是整理全部合格窗口。断言只覆盖测试自己创建的 6 个窗口。

`pending=2` 是桌面上两个第三方窗口（Chrome / Electron 系）拒绝被移动。产品行为正确：它们的 preimage 保留在 journal 中，事务保持未完成，而不是假装成功。**这也说明真实使用中，"全部恢复"通常不是 100%** —— UI 会显示待处理数量与进程号，而不是一个笼统的成功。

### 性能（100 窗口）

`e2e --stress 100` 在真实桌面上创建 100 个自己的窗口后测量：

| 运行 | 迁移 | 恢复 |
|---|---|---|
| 1 | 187 ms | 125 ms |
| 2 | 200 ms | 94 ms |
| 3 | 181 ms | 95 ms |

目标屏 1920×1032，容量 53 个窗口，其余 47 个按设计留在原位而不是压成不可用的小格。

**发现并修复的性能缺陷**：初版 40 窗口迁移耗时 **728 ms**，远超 300 ms 预算。定位到三处：

1. 每个窗口 3 次 `sync_all()`（preimage / intent / result）。改为按阶段批量：所有 preimage 一次 fsync 后才开始移动窗口，intent 每 16 个一批，result 在首轮结束后一次。
2. 重试在提交循环内同步 sleep，一个慢应用阻塞整批。改为两趟：首轮全部异步提交 + 读回，第二趟只重试偏离的窗口。
3. 每窗口重试 3 次且每次都 sleep。改为 2 次，且第一次重试不等待——观测中第三次尝试从未成功过。

改后 40 窗口 150 ms、100 窗口约 190 ms。

### 崩溃恢复

迁移中途 `Stop-Process -Force` 杀掉应用 → 重启：

```
WARN adopted unfinished transaction preimages=5 may_have_moved=true
```

UI 显示"上次整理未完成 · 有 5 个窗口的快照仍待处理"。点击"尝试恢复"后：

```
已恢复 5 个窗口。
```

窗口 `hwnd=4524076` 从 `-1908,442 602x500` 回到 `148,143 1363x826`（与崩溃前完全一致），journal 目录清空。

**发现并修复的缺陷**：修复前"尝试恢复"是空操作——`Engine::recover` 只读取 journal 报告状态，没有接管事务，因此 `restore()` 没有 preimage 可恢复。新增 `Journal::reopen` + `Engine::recover_latest` 让引擎接管事务，并加了两个回归测试。

### 全局热键

设置 `hotkey_gather=Control+Alt+G`、`hotkey_restore=Control+Alt+Shift+G` 后以 `--tray` 启动，注册无告警。

| 操作 | 结果 |
|---|---|
| 发送 `Ctrl+Alt+G` | 3 个窗口被移动到目标屏 |
| 发送 `Ctrl+Alt+Shift+G` | **11 of 11 个窗口回到原始矩形** |

### 安装包

```
tauri build --debug
Finished 1 bundle at: target\debug\bundle\nsis\Conflux_0.1.0_x64-setup.exe (4.56 MiB)
```

| 步骤 | 结果 |
|---|---|
| 静默安装 `/S` | 安装到 `%LOCALAPPDATA%\Conflux`，生成 `conflux.exe` + `uninstall.exe` |
| 启动安装版 | 窗口正常显示，2 个显示器被识别，目标屏选择持久化到 `settings.json` |
| 迁移 | 13 个窗口中 8 个被移动，全部落在 DISPLAY2 |
| 恢复 | 11/13 精确复原（差异项是测试工具 Orca 自己的窗口，非本产品问题） |
| 静默卸载 `/S` | 二进制移除，进程终止，无残留 |

Tauri 打包需要从 GitHub 下载 NSIS/WiX；本机直连超时，通过 `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR` 指向本地镜像完成。

### 重复整理拦截

已经在 Active 状态下再次点击"整理到目标屏"：

```
窗口仍处于整理状态。请先点「恢复本次布局」，再重新整理到新的目标屏。
```

**发现并修复的缺陷**：修复前第二次整理会新建事务并用**当前**（已被移动的）位置作为 preimage，导致之后恢复把窗口送回错误的位置——引擎的单写者与"原始位置"不变量被破坏。现在 `migrate` 在事务未关闭时返回 `TransactionAlreadyOpen`，并有回归测试 `second_migration_keeps_the_original_preimage` 覆盖"恢复后再整理"的正确路径。

### 部分恢复

一次真实恢复的结果：

```
已恢复 5 个窗口，保留你的改动 1 个，仍有 1 个待处理。
```

journal 保留在磁盘上，因为仍有窗口未达到安全终态。这是设计行为：删除 journal 会丢弃那些窗口的 preimage。

### 开机自启

在 UI 里勾选"登录时启动"后：

```
INFO reconciling autostart desired=true current=false exe=...\target\debug\conflux.exe
INFO autostart registration updated desired=true
HKCU\...\Run\Conflux = "F:\...\conflux.exe --tray"
```

取消勾选并重启后：

```
INFO reconciling autostart desired=false current=true
INFO autostart registration updated desired=false
Run key entry removed
```

**发现并修复的缺陷（死锁）**：修复前勾选"登录时启动"会让命令线程永久阻塞——`save_settings` 持有设置锁的同时调用了会再次读取设置的 `apply_autostart`，而 `std::sync::Mutex` 不可重入。症状是设置文件已写入、注册表没写、UI 卡住。修复方式是把辅助函数改成接收目标值而不是自己取锁，并用 `drop(settings)` 明确表态；另加了一个回归测试固定这个约束。

### 退出屏障

托盘"退出"在还有未恢复窗口时会先尝试恢复，失败则取消退出并提示。该分支无法用 UI 自动化点托盘图标，所以在引擎层用 3 个用例固定：

- 刚启动的引擎不阻塞退出；
- 整理后 `has_pending_work()` 为真，恢复成功后解除；
- 接管的崩溃事务同样阻塞退出，恢复并清理 journal 后解除。

前端也补上了 `conflux://exit-blocked` 与 `conflux://update` 的监听——此前 Rust 会发这两个事件，但界面没有任何反应，用户点退出"没反应"且看不到原因。为验证而临时加的命令与辅助函数在验证后已删除，没有留在产品 API 里。

### 单实例

连续启动两次 `conflux.exe`：

```
processes=1
```

第二次启动由 `tauri-plugin-single-instance` 接管并立即退出，只把已有窗口带到前台。这是引擎"单一写入者"前提的必要条件：两个进程同时操作同一批窗口会各自写一份 journal，恢复语义互相覆盖。

### 诊断导出

点击"导出诊断包"：

```
诊断包已写入 ...\Conflux\diagnostics\conflux-<uuid>.json（707 字节）
```

内容核对：只有版本、平台、显示器尺寸/缩放/主屏标记、显示器 ID 的 FNV 摘要、设置项名称与开关状态。**没有窗口标题、文件路径、屏幕画面、原始显示器设备路径**。

### 隐私开关

"发送匿名使用统计"默认关闭，勾选后 `settings.json` 的 `telemetry` 变为 `true`。当前构建**没有任何上传代码**，该字段只记录用户选择。

### 应用排除

真实桌面：规则 `msedge.exe`。

```
excluding msedge.exe at Rect { left: 960, top: 0, right: 1920, bottom: 1032 }
applied=3 skipped=9
  skipped msedge.exe reason=ExcludedApp
  ...
after: Some(Rect { left: 960, top: 0, right: 1920, bottom: 1032 })
PASS: excluded app did not move
```

界面上添加规则后写入 `settings.json` 的 `excluded_apps`，即时生效。

### 日志

第一次实现把日志交给 `tracing_subscriber::fmt()`，GUI 构建没有控制台，日志全部丢失。改为按日滚动文件（5 个 × 2 MiB）。

**第二个缺陷**：过滤器写成 `conflux=info`，而所有 crate 叫 `conflux_*`，因此引擎日志仍被丢弃——文件存在但只有两行。按 crate 逐条列出后，成功记录到：

```
INFO migration finished transaction=376f4b7d applied=4 refused=3 skipped=4 state=Active
INFO restore finished transaction=376f4b7d restored=6 kept_user_change=1 pending=0 state=Idle
```

### 结果里的应用名

结果卡片此前显示 `pid:hwnd`。现在按名称显示：`DeepSeek Harness.exe、Orca.exe、python.exe 没有接受位置调整`。

### 界面迭代后的回归

UI 重写后在同一台机器上重跑主流程：

```
已整理 4 个窗口，跳过 5 个。
窗口已收拢到目标屏。结束后点「恢复本次布局」。
已恢复 4 个窗口。
exact restore: 12/12
```

暂停、恢复、识别屏幕编号、诊断导出均在界面上实际点击验证。

### 布局列数（预览暴露的缺陷）

预览把 5 个窗口画成 5 条竖条。根本原因是网格评分里"空槽数量"权重高于"单元格长宽比"，于是 5×1（零空槽）胜出。

修正后按优先级：最小尺寸违反 → 浪费整行/整列 → 长宽比 → 空槽。9 个窗口曾落到 4 列（浪费一整列），一并修正为 3 列。新增 4 个用例固定 4/5/6/9 窗口的列数。

### 最终构建的完整回归

在性能重构之后重新构建并走完整流程：

```
已整理 3 个窗口，1 个应用未接受，跳过 8 个。
已恢复 4 个窗口。
exact restore: 12/12
```

12 个窗口全部精确回到原始矩形。

### 设置持久化

`settings.json` 在 `%APPDATA%\Conflux\`，写入后包含目标显示器的 `monitorDevicePath`：

```json
"target_display": {
  "native_id": "\\\\?\\DISPLAY#XMIC003#4&3abf5356&0&UID49#{e6f07b5f-...}",
  "label": "\\\\.\\DISPLAY2"
}
```

重启后仍指向同一块屏。

### 界面重做后的真实操作

新版界面（中性灰底、左侧分组导航、白卡片）在真实窗口上逐页截图核对，并用 OS 级点击走完全流程：

| 项 | 结果 |
|---|---|
| 引导第 2 步 | 两块屏各显示真实截图，角标 1/2 与屏幕上的编号一致 |
| 目标屏选择 | 点第 1 块后卡片显示"使用中"，`settings.json` 写入 `monitorDevicePath` |
| 迁移（点击「整理到目标屏」） | `3 已移动 / 0 拒绝 / 0 关闭 / 8 跳过`，状态转为"布局已启用" |
| 恢复（点击「恢复本次布局」） | 提示"已恢复 3 个窗口"，结果 `3 精确恢复 / 0 保留改动 / 0 回退可见 / 0 待处理` |
| 深色主题 | 切换后立即生效，卡片、边框、状态点对比正常；重启后保持 |
| 排除的应用 | 列出 7 个当前有窗口的应用（实时枚举，非硬编码） |
| 更新区域 | 显示"未配置更新源"并禁用按钮（本地构建 `pubkey` 为空） |

**发现并修复的缺陷**：缩略图只在"显示器/排除"两页请求，引导页的显示器卡片始终显示"无法截取画面"。原因是取图逻辑挂在页面 id 上，没有覆盖引导流程；改为由"当前是否有界面在显示截图"决定，并在显示器数量变化时重新取图。

**发现并修复的缺陷（如实状态）**：`update_status` 此前把 `last_outcome` 硬编码为 `None`，界面永远显示"尚未检查"。现在检查结果写入 `AppState`，并且当构建没有签名公钥/更新地址时直接报告"未配置"，而不是让用户点一个必然失败的按钮。

**关于构建方式**：本仓库的调试构建默认指向 Vite dev server（`devUrl`）。验证界面时需要 `--features tauri/custom-protocol` 把 `dist` 嵌入二进制，否则窗口加载的是 dev server 地址。此机 shell 里的 Node 是 WSL 版，直接 `npm run dev` 会因拿不到 Windows binding 而失败；前端构建本身正常（走 Windows Node）。

## 未验证

| 项 | 原因 | 需要什么 |
|---|---|---|
| macOS 全部功能 | 无 Mac 硬件 | 真机：TCC 授权、AX 移动、`CGDisplayCreateUUIDFromDisplayID`、Retina 坐标 |
| 自动更新安装 | `plugins.updater.pubkey` 与 `endpoints` 为空；界面会如实显示"未配置" | 生成 minisign 密钥 + 托管更新清单 |
| 代码签名与公证 | 无证书 | Windows Authenticode 证书；Apple Developer Program |
| 托盘菜单点击 | 托盘图标无法被 UI 自动化识别 | 人工点击验证，或加一个测试专用的命令触发入口 |
| 混合 DPI | 本机两块屏都是 100% | 一台 150% 的屏 |
| 提权窗口 | 未构造高完整性测试窗口 | 以管理员启动一个记事本作为目标 |
| 大规模窗口 | 最多测过 13 个 | 100 窗压力测试 |

## 复现方式

```bash
cargo test --workspace
cargo run -p conflux-platform-win --bin e2e      # 真实桌面，创建并移动自己的窗口
cargo run -p conflux-platform-win --bin probe    # 只读
```
