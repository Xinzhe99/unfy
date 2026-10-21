# 验证记录

本文只记录**实际执行过**的检查与结果。未执行的项目明确写"未验证"，不作推断。

日期：2026-10-21 · 主机：Windows 10.0.26200 · 双屏 1920×1080 并排（`\\.\DISPLAY1` 在 0,0，`\\.\DISPLAY2` 在 -1920,-78）

## 自动化测试

| 项 | 命令 | 结果 |
|---|---|---|
| 单元与集成测试 | `cargo test --workspace` | 82 passed, 0 failed |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all -- --check` | 通过 |
| 前端类型 | `npx tsc --noEmit` | 通过 |
| 前端构建 | `npm run build` | 通过（245 kB JS / 19 kB CSS） |
| macOS 编译 | `cargo check -p conflux-platform-mac --target aarch64-apple-darwin` | 通过（仅类型检查，未在 Mac 运行） |

测试分布：domain 14、platform(fake) 4、journal 11、layout 14、engine 21（含 3 个退出屏障用例）、settings 12、win 1、mac 4（macOS 目标下编译）。

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

### 设置持久化

`settings.json` 在 `%APPDATA%\Conflux\`，写入后包含目标显示器的 `monitorDevicePath`：

```json
"target_display": {
  "native_id": "\\\\?\\DISPLAY#XMIC003#4&3abf5356&0&UID49#{e6f07b5f-...}",
  "label": "\\\\.\\DISPLAY2"
}
```

重启后仍指向同一块屏。

## 未验证

| 项 | 原因 | 需要什么 |
|---|---|---|
| macOS 全部功能 | 无 Mac 硬件 | 真机：TCC 授权、AX 移动、`CGDisplayCreateUUIDFromDisplayID`、Retina 坐标 |
| 自动更新安装 | `plugins.updater.pubkey` 与 `endpoints` 为空 | 生成 minisign 密钥 + 托管更新清单 |
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
