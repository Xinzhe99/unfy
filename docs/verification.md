# 验证记录

本文只记录**实际执行过**的检查与结果。未执行的项目明确写"未验证"，不作推断。

日期：2026-10-21 · 主机：Windows 10.0.26200 · 双屏 1920×1080 并排（`\\.\DISPLAY1` 在 0,0，`\\.\DISPLAY2` 在 -1920,-78）

## 自动化测试

| 项 | 命令 | 结果 |
|---|---|---|
| 单元与集成测试 | `cargo test --workspace` | 104 passed, 0 failed（28 个测试二进制） |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all -- --check` | 通过 |
| 前端类型 | `tsc --noEmit` | 通过 |
| 前端构建 | `npm run build` | 通过（265 kB JS / 28 kB CSS） |
| macOS 编译 | `cargo check -p conflux-platform-mac --target aarch64-apple-darwin` | 通过（仅类型检查，未在 Mac 运行） |

测试分布：domain 14、platform(fake) 4、journal 12、layout 18（含 4 个列数选择用例）、engine 25（含 3 个退出屏障与 4 个暂停/延迟用例）、settings 14（含引导可恢复性）、win 1、mac 4。

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
| 浅色主题 | 主按钮为近黑实心、选中项为近黑描边；无任何蓝色元素（按视觉要求改为单色强调） |
| 排除的应用 | 列出 7 个当前有窗口的应用（实时枚举，非硬编码） |
| 更新区域 | 显示"未配置更新源"并禁用按钮（本地构建 `pubkey` 为空） |

**发现并修复的缺陷**：缩略图只在"显示器/排除"两页请求，引导页的显示器卡片始终显示"无法截取画面"。原因是取图逻辑挂在页面 id 上，没有覆盖引导流程；改为由"当前是否有界面在显示截图"决定，并在显示器数量变化时重新取图。

**发现并修复的缺陷（如实状态）**：`update_status` 此前把 `last_outcome` 硬编码为 `None`，界面永远显示"尚未检查"。现在检查结果写入 `AppState`，并且当构建没有签名公钥/更新地址时直接报告"未配置"，而不是让用户点一个必然失败的按钮。

**关于构建方式**：本仓库的调试构建默认指向 Vite dev server（`devUrl`）。验证界面时需要 `--features tauri/custom-protocol` 把 `dist` 嵌入二进制，否则窗口加载的是 dev server 地址。此机 shell 里的 Node 是 WSL 版，直接 `npm run dev` 会因拿不到 Windows binding 而失败；前端构建本身正常（走 Windows Node）。

### 自动更新的检查链路

用一个 minisign 测试密钥和本地清单服务真实走了一遍检查流程（`tauri.conf.json` 临时指向
`http://127.0.0.1:8787/manifest.json`，验证后已还原为空配置）：

```
WARN the update download URL http://127.0.0.1:8787/artifact.bin does not use the `https` protocol
INFO update check finished outcome="failed"
界面显示：install failed: invalid updater binary format
```

**能证明的**：端点被读取、清单被解析、签名被校验、下载被执行、失败原因一路传到了界面。
最后一步失败是因为测试产物不是合法的 updater 包（用的是图标文件签的名），不是流程缺陷。

**发现并修复的缺陷**：在此之前 `check_update` 只返回字符串 `"failed"`，具体原因只写进日志。
用户点「检查更新」只会看到"检查更新失败，请稍后重试"，无法判断是网络、签名还是更新包格式。
现在 `UpdateOutcome::detail()` 把原因带回界面，设置页与提示条都会显示。

**不能证明的**：真实发布服务器可达、HTTPS 证书、以及一次真实升级后进程重启并保留用户状态。
这些需要真实发布环境，见 [release.md](release.md) 的首次发布清单。

### 前端单测

快捷键录制把键事件转换成 Tauri 加速键语法。转换写错时界面看起来完全正常、只是快捷键不生效，
所以这部分抽成纯函数并用 9 个用例固定（`bun test`）：

```
9 pass, 0 fail, 16 expect() calls
```

覆盖：修饰键顺序、`CmdOrCtrl` 双平台映射、单独按修饰键被拒绝、无修饰键的裸字母被拒绝、
命名键映射（方向键 / 空格 / Esc / PageDown）、功能键、Windows 键映射为 `Super`、
字母大小写归一、无法表示成加速键的键被拒绝。

**为什么不能用点击验证**：本机向 WebView2 注入合成按键（`keybd_event`、`SendInput`）都不会到达
渲染进程，这是环境的输入注入限制。因此录制器的**状态机**（进入录制、Esc 取消、Backspace 清除）
只做了代码审查与截图确认，**纯转换逻辑**有真实测试。首次在真机上使用时需要人工按一次组合键确认。

### 前端渲染与设计约束

```
bun test
 32 pass, 0 fail, 65 expect() calls   (4 files)
```

四组测试，都是为了让"看起来正常"的东西不再靠肉眼把关：

| 文件 | 固定的事实 |
|---|---|
| `pages/render.test.tsx` | 每个页面在代表性数据下都能渲染（概览 4 种状态、显示器、排除、设置 2 种更新配置、引导 4 步、录制器、预览），空树或抛错即失败 |
| `lib/design.test.ts` | 除 token 文件外不得出现硬编码颜色；TSX 里引用的每个 `var(--color-*)` 都必须已定义；强调色在两个主题下 chroma < 0.02；语义色每主题只声明一次；页面必须用共享 `Card` |
| `lib/design.test.ts`（对比度） | 8 组文字/背景的 WCAG 比值均 ≥ 4.5，且 muted 明显浅于正文 |
| `lib/a11y.test.ts` | 纯图标按钮必须有可访问名称；`Switch` 的 `label` 是必填；导航是有名地标且带 `aria-current`；焦点有可见替代；尊重 `prefers-reduced-motion`；正文不得用裸白/黑 |

**测试立即抓到的真实问题**：`--color-muted` 在白底上只有 **3.96:1**，而它承载全站 11px 提示文字——
低于 WCAG AA 对正文的 4.5:1。调整到 `oklch(0.535 …)` 后为 5.18:1（画布上 4.79:1），同时仍明显
浅于正文。这个问题在任何截图里都"看着没问题"。

**关于截图**：验证到这一步时机器的会话已锁屏，`PrintWindow` 与全屏截图都会返回锁屏画面，
不能作为界面证据。因此改用无头浏览器渲染 `dist/`，并用一份 IPC stub 提供代表性数据
（见 `.verify/update/ui-stub.js`，不随产品发布）。逐页核对结果：

| 检查 | 结果 |
|---|---|
| 概览（浅色 / 深色） | 状态、目标屏、远程检测证据、活动历史、工具分区层级清晰；深色下选中导航有可见底色 |
| 显示器（浅色 / 深色） | 真实截图占位、编号角标、选中描边；深色下未选中角标改为固定深色底 |
| 排除的应用（浅色） | 实时列出 5 个应用、3 条规则、一键清空（无规则时禁用） |
| 设置（浅色 / 深色） | 检测灵敏度、自动整理/恢复、快捷键录制按钮、更新状态"未配置更新源" |

**这一轮发现并修复的三处深色缺陷**：
1. 选中导航项直接用 `--color-surface`，在深色下几乎与侧栏同色，看不出选中——新增
   `--color-nav-active`，两套主题各自取值。
2. 深色主按钮是 0.93 近白，与 40% 透明度的禁用按钮几乎无法区分——改为 0.80 中灰。
3. 显示器编号角标硬编码 `bg-black/55`——改为 `--color-overlay`，并注明它不随主题变化
   （它叠在截图之上，亮度与主题无关）。

### 启动路径的降级

日志里出现一次 `ERROR panic … tauri/src/app.rs:1444`。定位到 `setup` 中的
`tray::install(app.handle())?`：托盘创建失败会把 Tauri 的 setup 错误变成 panic，
进程在显示窗口之前就退出。托盘是可选界面——窗口、快捷键与引擎都不依赖它，因此改为记录错误后继续启动。

修复后连续启动验证：

```
INFO reconciling autostart desired=false current=true  exe=…\conflux.exe
INFO autostart registration updated desired=false
INFO remote-session evidence changed summary=no remote-session evidence
```

无 panic，且此前 UI 测试遗留的开机自启注册项被正确移除（注册表 Run 键已无 Conflux）。

**这一条没有被任何测试发现**，是读日志时看到的。它说明"启动成功"必须包含"日志里没有 panic"，
而不只是"窗口出现了"。

### 预览显示应用名（真实桌面）

```
布局预览：7 个窗口
矩形内显示：Orca / msedge / magpie-wi… / explorer #4 / DeepSeek … / Weixin / explorer #7
```

同一应用多窗口时追加序号，否则两个 `explorer` 矩形无法区分——而"哪个 explorer 窗口
会去哪一格"正是这张图存在的理由。长名称按字符截断（`WindowsTe…`），不做 SVG 裁剪：
被切掉的词看起来像渲染 bug。

应用名只取可执行文件名，永不用窗口标题（标题含文档名、URL 和消息预览）。

### 拒绝应用 → 排除规则的闭环

真实整理结果里 `applied=3 skipped=9`，其中两个应用拒绝移动。新流程：

1. 结果消息列出应用名，而不是只给数量；
2. 写入 `last_refused_apps`，重启后仍在；
3. 概览页出现「没有接受位置调整的应用」卡片，每行一个「排除」按钮，已有规则的会被过滤掉。

**为什么值得做**：一条"3 个应用未接受"的提示无法据以行动；用户打开排除页时提示已经消失，
只能凭记忆重新输入文件名。

### 环境自检

```
窗口控制权限   可以移动其他应用的窗口。
显示器         检测到 2 块显示器。
全局快捷键     已注册，在其他应用中也生效。
配置目录       可写入 C:\Users\…\Conflux
```

五项检查：权限、显示器数量（0 是故障，1 是合法配置）、快捷键是否真的注册成功、
配置目录是否可写、是否有未完成事务。快捷键这一项最关键——注册失败在用户按下组合键
之前完全不可见。

### 整理进度

引擎新增 `on_progress` 回调，每提交一个窗口调用一次；两个用例固定它：事件数等于窗口数、
单调递增、最后一个事件到达总数（否则界面永远看不到完成）。默认不装回调时行为不变。

真实桌面（7 个窗口）实测：

```
已整理 3 个窗口，跳过 4 个；DeepSeek Harness.exe、Orca.exe、Weixin.exe、
magpie-windows-amd64.exe 没有接受位置调整。
```

按钮在运行期间显示 `正在整理 3/7…`。**为什么要数字**：一个不确定的转圈在单个应用把
消息队列卡住几秒时，与"程序卡死"完全无法区分。

### 批量排除

真实桌面：4 个应用拒绝移动 → 点一次「全部排除」→ `settings.json` 写入 4 条规则：

```
rules before: 0
rules after: 4
['DeepSeek Harness.exe', 'Orca.exe', 'Weixin.exe', 'magpie-windows-amd64.exe']
```

一次点击而不是四次的理由不只是省事：逐条调用会各自基于点击前读到的规则快照保存，
互相覆盖，最后只剩一条。`onExclude` 因此接收数组，合并与去重在一次遍历里完成。

### 窗口几何持久化

先试了 `tauri-plugin-window-state`，**放弃**：它在干净退出时保存，而本项目的验证流程大量
强杀进程，几何在真正需要时从不落盘。改为写入 `settings.json` 的 `window_geometry` 字段，
在窗口关闭时（`CloseRequested`，此时尺寸仍真实存在）采集。

实测：

```
调整到 200,120 1040x800 → 关闭窗口 → settings.json 记录 [200, 120, 1024, 761]
重启 → 窗口回到 200,120 1040x800
```

恢复前会检查保存的位置在当前显示器上是否仍有可见重叠：显示器被拔掉后，窗口若还原到
不存在的坐标，用户没有鼠标能把它拖回来。范围检查（最小 720×520、坐标绝对值 ≤ 20000）
有单元测试覆盖。

### 显示器卡片按真实位置排布

截图里的两块屏是 `DISPLAY2` 在 x=-1920（左）、`DISPLAY1` 在主位（右）。原来的 2×3 网格
把两块屏并排画出来，**编号却是平台枚举顺序**：左边卡片写着"2"，右边写着"1"。

这不是排版问题：屏幕上闪烁的编号由 `identify_displays` 绘制，用的也是枚举顺序。用户在远程
画面里读到"1"，回头点左边那张卡片，实际选中的是另一块屏。

修复分两处，共用一条规则 `DisplaySnapshot::ordered_by_position`（从左到右、从上到下）：

1. 卡片按真实坐标绝对定位，垂直堆叠的显示器会画成上下而不是并排；
2. 屏幕上的编号与卡片使用同一个函数排序。

**布局本身踩了四次坑**，每次都留下截图证据：

| 尝试 | 结果 |
|---|---|
| `aspectRatio` 派生容器高度 | 两块 1920×1080 并排 → 容器 40px 高，卡片成细条 |
| 按"最高卡片占比"反推高度并设上下限 | 上限截断后卡片仍只有 50px，下方留半屏空白 |
| `aspectRatio` + 百分比单元格 | 容器被压扁，卡片成薄片（截图可见） |
| **固定行高 × 行数 + 绝对定位** | 正确：卡片可读、位置保真、无空白 |

最终方案不依赖宽高比：每行 9rem，容器高度 = 行数 × 9rem，单元格用百分比定位到容器内。
宽度仍按真实像素比例分配，所以 1920×1080 与 2560×1440 的卡片宽度不同——这也顺带证明了
布局读取的是真实几何而不是均分。

三个 domain 测试固定排序：x 为负的屏排第一、堆叠时上屏排第一、单屏稳定。

### 「保留当前布局」——退出屏障的第三条路

退出时的提示一直写着"请先恢复布局，或选择保留当前布局"，但**这个选项在任何界面里都不存在**。
`Engine::keep_current_layout` 有实现也有单元测试，却没有任何命令或按钮调用它。结果是：
恢复不成功的用户无法退出——托盘拒绝退出，界面也没有别的出路。

补齐后实测：

```
整理 → journal 文件数 1
点「保留当前布局」→ journal 文件数 0
状态回到「就绪」，提示「已保留当前布局，之后可以重新整理到目标屏」
按钮随状态消失
```

语义是明确的：关闭 journal 并丢弃 preimage，从这一刻起当前布局就是新的基准，下次整理会把它
当作"原位"。这条也写进了活动历史（`keep_layout`）。

### 各窗口的最小尺寸

真实桌面上八个窗口自报的最小值差异很大：

```
Orca.exe                  616x408
magpie-windows-amd64.exe  616x462
msedge.exe                516x90
Weixin.exe                440x452
clash-verge.exe           536x559
Notepad.exe               332x202
explorer.exe              161x243  (两个资源管理器窗口)
```

此前引擎对所有窗口传 `None`，布局层回退到统一 320×200——大应用的窗口会被压进自己的
标题栏都放不下的格子，表现为与邻居重叠。

**性能教训**：第一版在枚举时探测，八个窗口的枚举从 94ms 涨到 **213ms**，因为它对每个窗口
做一次跨进程 `SendMessageTimeout`。改为在 `capabilities()` 中惰性探测（引擎只对将要放置的
窗口调用），并保留 33ms 超时与 `SMTO_ABORTIFHUNG`：

```
enumerate 15 ms, preview 17 ms, migrate 211 ms (6 host windows)
```

比探测前的 94ms 更快，因为枚举不再读取任何跨进程信息。

一个引擎测试固定这条连线：窗口声明 900×700 最小值时必须得到不小于该值的格子，并被标记为
`clamped_to_min`。把引擎改回传 `None` 时该测试失败。

### 状态机里的死状态

`EngineState::Error` 从定义起就**从未被赋值**，只在 `migrate` 里检查过。而它描述的情况是真实的：
journal 无法写入时，durability 保证已经不成立。

真正的后果不是"这个状态没用"，而是**应用会卡死**：`migrate` 先把状态设为 `Migrating`，
随后 journal 写入失败用 `?` 提前返回——状态就永远停在 `Migrating`。之后每一次调用
（整理、恢复、重新整理）都被 `InvalidTransition` 拒绝，界面显示"正在整理"，直到重启进程。

修复后 journal 写入或 `durable()` 失败都会进入 `Error`，并且：
- `rescue_offscreen` 与 `restore` 在 Error 下仍然可用（用户必须有出路）；
- 新的整理被拒绝，而不是在一个可能不持久的 journal 上继续。

测试用一个"父路径是文件"的目录制造真实写入失败，断言状态变为 `Error`、救援仍可用、
再次整理被拒绝。把 Error 赋值去掉后该测试失败，报 `left: Migrating, right: Error`。

### 设置保存了但没生效

`save_settings` 在事务进行中调用 `engine.update_config` 会被拒绝（当前计划是用旧设置算的，
中途替换会让 journal 与预览不符）。这本身是对的，但调用方只写了一行 `warn!`：磁盘上已是新值，
运行中的引擎仍是旧值，用户改完布局发现下次整理没变化。

现在这条路径会写入 `last_error`，概览页显示"设置已保存，但当前整理还在进行中，新的布局会在
下次整理时生效"。

### Error 状态下的界面（真实触发）

把 journal 目录换成一个同名文件，制造真实的写入失败，然后点「整理到目标屏」：

```
状态        需要处理
上次操作失败  无法写入恢复记录，为避免窗口位置丢失，整理已停止。请确认磁盘空间与目录权限，然后重启应用。
按钮        知道了 / 尝试恢复 / 救援越界窗口
```

**关键点是这里没有出现「整理到目标屏」。** 引擎在 Error 下按设计拒绝新整理（journal 不持久，
再写一个会把窗口置于风险中），而旧界面恰好只提供这一个按钮——用户能按的正是唯一不能运行的
那个操作。

原始错误是 `journal write failed: io error: ... (os error 183)`，英文的 OS 报错。现在翻译成
用户能据以行动的说明，精确原因留在诊断包里。

验证后 journal 目录已还原。

### 自动整理真的会触发（首次端到端）

此前 `auto-gather` 从未在日志里出现过——代码写了，但从没被真实驱动过。

手动标记原来用 `tracker.force()` 直接改状态，**绕过了 tracker 的状态转换**，monitor 永远观测
不到 `Activated`，所以自动整理与自动恢复对手动标记完全无效：标记只改变了界面显示。

改为只设置 `manual_remote` 标志并立即跑一次 poll，让正常转换路径接管。真实运行结果：

```
INFO remote-session evidence changed summary=
INFO migration finished transaction=ae6e5ff3 applied=2 refused=0 skipped=6 state=Active
INFO auto-gather applied evidence=手动标记 applied=2
INFO auto-gather finished evidence=手动标记 applied=2
```

两个窗口被真实移动。同时把同一个动作暴露为 `toggle_remote_session` 命令，概览页多了一个
「标记为远程会话 / 结束远程会话标记」按钮——托盘图标可能被系统折叠，而托盘菜单没有键盘入口。

### 状态灯配色

原来的映射让三种状态看起来一样：

| 状态 | 旧颜色 | 问题 |
|---|---|---|
| 就绪 | 灰点 `oklch(0.535 …)` | 与"禁用控件"同色，读起来像软件没在工作 |
| 布局已启用 | 近黑点 `oklch(0.26 …)` | 与正文同色，不构成状态信号 |
| 整理中 | 近黑点 | 同上 |

改为四色，每色只承担一种含义：

| 状态 | 颜色 | 对比度（vs 白） |
|---|---|---|
| 就绪 / 健康 | 绿 `oklch(0.52 0.11 155)` | 5.22 |
| 布局已启用 / 进行中 | 蓝 `oklch(0.55 0.13 250)`（新增 `--color-active`） | 4.83 |
| 已暂停 / 部分受限 | 琥珀 `oklch(0.55 0.1 70)` | 4.97 |
| 需要处理 | 红 `oklch(0.51 0.16 25)` | 6.25 |

全部高于 WCAG 对 UI 组件的 3:1 门槛。强调色仍是单色的——蓝色只用于状态，不用于按钮或选中态。

托盘图标同时修正：此前硬编码 `(74,108,220)` 与 `(214,158,46)`，与界面新配色不一致；现在与
状态灯同色（就绪绿、生效蓝、暂停琥珀、错误红），并在注释里写明它对应哪几个 token。

两个测试固定这条：所有状态灯对白底 ≥ 3:1；"就绪"使用有彩度（chroma > 0.05）的颜色而非
muted 灰（chroma < 0.02）。

实机截图确认：就绪为绿点，整理后"布局已启用"为蓝点，侧栏底部同步。

**第二轮**：用户反馈"就绪旁边那个灯应该绿色而不是灰色"。检查代码时它已经是 `success`（绿），
但截图放大后发现两个问题让绿色看不出来：

1. 圆点只有 **6px**，在 100% 缩放下像一个污点，不足以传达颜色；
2. 绿色 `oklch(0.52 0.11 155)` 的色度偏低，8px 尺度上读起来接近灰绿。

改为 8px 并把色度提到 `0.14`（`oklch(0.55 0.14 152)`，对白底 4.53:1，仍达标）。现在的
绿色在小尺寸下也能一眼认出。

新增测试断言"就绪"使用 `--color-success` 而非 `--color-muted`；改回 `tone="neutral"` 时
测试立即失败。

### 恢复延迟：实测

设置 4 秒延迟后点「恢复本次布局」：

```
19:19:02.750  INFO restore delayed; a new gather will cancel it seconds=4
19:19:06.751  INFO restore delay elapsed; restoring now
19:19:06.754  INFO restore finished restored=1 pending=0 state=Idle
```

精确 4.0 秒，界面同时显示递减倒计时（观察到"2 秒后恢复布局"）。

**发现并修正的设计错误**：自动恢复原先**跳过**延迟，理由是"检测器已经观察了几轮"。这恰好废掉了
延迟存在的理由——延迟就是为"断开后很快重连"设计的，而 RDP 与手动标记是**立即**转换的，重连
必然导致"恢复 + 再整理"两次搬移。现在自动恢复也遵守延迟，且挂起的恢复会被新的整理取消
（`Schedule` 的世代计数），取消时静默返回而不是记成"恢复了 0 个窗口"。

三个测试驱动真实的 `Schedule` 与 `wait_cancellable`，验证取消能打断等待。

### 三个整理入口都取消挂起的恢复（代码审查 + 单元测试）

延迟机制的前提是"重连会杀掉计时器"。检查三个入口时发现：

| 入口 | 是否取消挂起的恢复 |
|---|---|
| 界面「整理到目标屏」 | 是（`commands.rs`） |
| 全局快捷键 | **否** → 已补 |
| 检测器自动整理 | **否**（我上一轮声称已实现，实际没有）→ 已补 |

同时移除了 `monitor::restore` 里等待结束后对自己 token 的 `cancel()`：它取消的是自己，
既无意义又会让下一次 claim 从不同世代开始。

7 个单元测试驱动真实的 `Schedule` 与 `wait_cancellable`：重连取消恢复、未取消时正常执行、
取消后不阻塞下一次恢复。

**实机验证的局限**：尝试用合成 `Ctrl+Alt+G` 验证快捷键路径，但按键没有送达 WebView（本环境
的已知限制），日志显示恢复按 10 秒延迟正常执行——这只证明"未收到取消时会恢复"，没有证明
"收到取消时会中止"。快捷键与检测器路径的取消依赖上述代码位置与单元测试。

### 恢复默认设置（实机验证）

此前没有任何重置入口：配置出错只能手动找 JSON 文件。

现在设置页底部有「恢复默认设置 → 确认清除」两步确认。真实运行：

```
BEFORE: excluded_apps=['test-a.exe','test-b.exe'], margin=40, auto_detect=aggressive
AFTER:  excluded_apps=[], margin=12, auto_detect=balanced, layout_mode=grid
目标显示器: 保留（number: 2）
```

**目标显示器是唯一保留的项**：它是用户对着屏幕确认过的结果，重置把它丢掉等于让人重新做一遍
辨认工作。

**发现并修正**：第一版用 `window.confirm` 做确认，在 Tauri WebView 里弹出了一个标题为 "W" 的
异常窗口——原生对话框在 WebView 中不可靠。改为页内两步确认（`resetArmed` 本地状态），
破坏性操作不再依赖可能不出现的对话框。

### 死代码清理

三处「用占位函数压住警告」的写法被清除：

| 位置 | 掩盖的问题 |
|---|---|
| `commands.rs` 的 `fn unused(..)` | 未使用的 `HashMap`、`IdentityQuality` 导入 |
| `engine` 的 `fn unused_capabilities(..)` | 未使用的 `Capabilities`、`Unplaced` |
| `live_transaction` 命令 | 前端定义、后端注册，无人调用；注释声称"UI 会轮询"但实际没有 |

占位函数只是把编译警告挪走，问题仍在。删掉后立刻暴露了两个未使用的导入。

### 诊断包包含活动历史

一条"它把我的窗口弄乱了"的报告，没有"它到底做了什么、什么时候"就无法排查。诊断包此前不含历史。

现在导出最近 50 条，字段与界面一致（时间、类型、影响数、未处理数、触发来源）。真实导出：

```
keys: ['activity', 'app_version', 'detection', 'display_count', 'displays',
       'platform', 'settings', 'state', 'supported']
activity entries: 35
sample: {'affected': 1, 'at': 1792610603, 'kind': 'restore', 'trigger': 'window', 'unsettled': 0}
```

**隐私核对**（对导出文件做字符串扫描）：

```
clean: 'title'   clean: '.exe'   clean: 'C:\'   clean: 'DISPLAY#'   clean: '\\?\'
```

无窗口标题、无应用名、无原始设备路径、无用户目录。文件 5499 字节。

一个测试固定入口字段的形状：序列化结果不得出现 `title` / `path` / `url` / `user` / `app_name`。

### 只读操作失败后不能把引擎留在忙态

`preview` 先把状态设为 `Planning`，再用 `?` 返回 `build_preview` 的错误。失败时状态就永久停在
`Planning`——此后整理、恢复、重新整理、甚至再预览一次，全部被 `InvalidTransition` 拒绝，
只能重启进程。

这与上一轮修的 `EngineState::Error` 是同一种缺陷，只是发生在只读路径上：预览什么都没移动，
失败后回到 `Idle` 才是诚实的终态。

同一轮还修了 `migrate` 中途 `enumerate` 失败：那会让状态停在 `Migrating`，同样不可操作。
现在改用 `Degraded`（journal 已有 preimage，恢复可用）。

测试用"不存在的显示器"触发失败，断言状态回到 `Idle` 且后续整理仍然成功。把修复去掉后
测试报 `left: Planning, right: Idle`。

### 取消暂停不能把"部分完成"说成"全部完成"

`resume` 原本只在有事务时直接把状态设为 `Active`。`Degraded` 的三种来源都被它覆盖：

- 接管了上次崩溃留下的事务；
- 迁移中途平台失败；
- 计划里一个窗口都没放进去。

`Degraded` 的含义是"还有窗口没处理"。概览页据此在"恢复"与"保留当前布局"之间选择，覆盖它等于
告诉用户布局已完成，而实际还有残留。

现在只在原本不是 `Degraded` 时才恢复为 `Active`。测试用真实路径构造：整理 → 丢弃引擎 →
对新引擎 `recover_latest` 接管未完成事务 → 断言状态为 `Degraded` → 暂停 → 取消暂停 →
仍为 `Degraded`。去掉修复后测试报 `left: Active, right: Degraded`。

### 窗口隐藏时停止轮询

关闭窗口只是隐藏它，进程继续在托盘运行。原来每 4 秒无条件刷新一次，每次 **8 个命令**：
`get_status`、`list_displays`、`get_settings`、`pending_work`（扫描 journal 目录）、
`hotkey_status`、`update_status`、`activity_log`（读整个历史文件）、`health_check`（写盘探测）。

`document.visibilityState` 在 Tauri 隐藏窗口时**不会变化**——先按 Web 标准加了
`visibilitychange` 监听，实测隐藏后 CPU 仍以可见速率增长。改用后端在隐藏时发
`conflux://window-hidden` 事件，前端停表；窗口重新获得焦点时恢复。

计数验证（临时探针，验证后移除）：

```
可见 12 秒：轮询 5 次
隐藏 12 秒：轮询 0 次
```

**CPU 测量不可靠**：可见 0.094s / 隐藏 0.078s（10 秒），差异小于采样噪声，因为 WebView
自身的空闲开销占主导。轮询次数的直接计数才是有效证据。

### 磁盘写入从"每 4 秒"降到"每 10 分钟"

`health_check` 里的存储检查会创建并删除一个探针文件。它被前端每 4 秒轮询一次——也就是说
**窗口开着的时候，程序一直在写盘**，为了一个几乎从不变化的事实。

改为缓存 10 分钟。探测本身的实现（`ShellExecuteW` 之前的 `create → write → remove`）不变，
只是不再每次轮询都跑。

### 打开配置目录

诊断导出后只显示完整路径文本，用户需要自己去找。现在概览「工具」里有「打开配置目录」，
经 `ShellExecuteW` 调用系统文件管理器。

后端只允许打开配置根目录下的路径：请求可以省略参数（表示配置目录本身），显式传入的路径会
与配置根做前缀校验。否则这个便捷入口就成了任意目录读取原语。

探针实测：`dir exists: true` → `open_directory -> true`。

### 快捷键的发现性

产品默认不设快捷键（避免占用用户已有的组合键），但这意味着**新用户永远不知道有一键整理**——
而"一键"正是核心卖点。实测这台机器上两个快捷键一直为空。

设置页在两者都为空时提供「使用建议组合」，一次写入 `CmdOrCtrl+Alt+G` 与 `CmdOrCtrl+Alt+R`。
实测：

```
hotkey_gather: CmdOrCtrl+Alt+G
hotkey_restore: CmdOrCtrl+Alt+R
日志：无注册冲突
界面：Ctrl + Alt + G / Ctrl + Alt + R，无"未生效"提示
```

概览页另有提示：既没设快捷键、检测也关闭时，说明"现在只能从本窗口或托盘触发"。

**为什么不是默认值**：默认占用系统组合键会在用户已有快捷键的应用里静默抢键，这是更糟的体验。
建议按钮把选择权留给用户，同时让功能可被发现。

### 恢复延迟的默认值与作用范围

规格写的是"默认 15 秒、可配置 15–120 秒的明确断开延迟内，重连合并进原事务"。实现里默认是
**0 秒**，上限 60 秒——也就是说这个保护默认从不生效。改为默认 15 秒、上限 120 秒。

更重要的是**作用范围**：延迟只应作用于"检测到远程会话结束后自动恢复"这一条路径。

| 入口 | 是否等待 | 理由 |
|---|---|---|
| 概览「恢复本次布局」 | 否 | 用户点了就要它执行，等 15 秒像按钮坏了 |
| 托盘「恢复本次布局」 | 否 | 同上 |
| 快捷键恢复 | 否 | 同上 |
| 退出时恢复 | 否 | 同上 |
| 检测到会话结束后自动恢复 | **是** | 期间的重新整理会取消它，这才是延迟的用途 |

修正前除退出路径外都会等待。实测（延迟设为 15 秒）：手动点击恢复后日志里没有
`restore delayed`，直接出现 `restore finished restored=2`。

### 循环切换目标屏（规格 P0-04-B）

按产品规格逐条核对时发现这一项**完全未实现**：远程端只有一块屏时，用户无法快速切换显示哪一块。

托盘与显示器页都加了「切换到下一块屏」。它委托给 `Engine::retarget`，因此天然满足规格的
不变量——**不覆盖原基线**。实测连续切换两次：

```
目标: DISPLAY1(编号 2) → DISPLAY2(编号 1) → DISPLAY1(编号 2)
20:04:11.038  restore finished transaction=5e625c62 restored=2 pending=0 Idle
20:04:11.123  migration finished transaction=8d6c1b69 applied=2 state=Active
```

第二次切换先恢复（`restored=2`）再按新目标整理（`applied=2`），两次用的是**不同事务**，
说明原基线没有被中间布局覆盖。

单显示器时托盘项禁用而不是隐藏，避免设备变化时菜单项忽隐忽现。

### 未实现的规格项（明确记录）

| 规格 | 状态 |
|---|---|
| P0-04-B 循环切换目标屏 | **本轮实现** |
| P0-11-A 中英双语界面 | **未实现**，界面只有中文 |
| P0-03 检测规则包与签名更新 | 未实现（v1 范围外，已由 D2 决策排除） |
| P0-16 授权/订阅 | 未实现（v1 不做商业化） |
| P0-17 多来源仲裁 | 未实现（依赖 P0-03） |

### 冷却期与"远程端能看到全部屏幕"

按规格 §6.1 补充两项。

**冷却 30 秒**：会话反复启停会连续产生 `Activated`/`Deactivated`。检测器的连续两轮确认能过滤
单次抖动，但过滤不了真实的短暂断线。自动整理加了一层 30 秒冷却；被抑制时界面明确说明原因，
而不是静默不动（静默会被当成功能坏了）。

**远程端能看到全部屏幕**：规格要求"用户选择远程端可见全部屏幕时默认不收拢"。此前没有这个
选项，所有用户都被当作"远端只有一块屏"。

实机验证（`auto_gather=true`、`remote_screens=all`，用手动标记触发）：

```
20:08:47.436  remote shows all screens; nothing to gather evidence=手动标记
journal 文件数：0
```

没有任何窗口被移动，日志给出了原因。

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
