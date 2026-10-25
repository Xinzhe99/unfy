# Unfy 发布说明

本文说明如何产出可分发安装包、如何配置签名与更新，以及每一项的风险边界。**不要把本文当作已验证的发布流程**：文中的签名、公证、商店审核均未在本机执行过，首次正式发布前必须逐项走通并记录证据。

## 0. 当前实际状态（先读这一节）

自动更新只需要**一件事**：一个 minisign 密钥对。Tauri 的更新器对每个下载的更新包验签，没有私钥就签不出包，装好的客户端会拒绝安装。这就是全部必要条件。

原生代码签名（Windows Authenticode / Apple Developer ID + 公证）是**另一件事**：它消除安装时的 SmartScreen 警告和 macOS Gatekeeper 拦截，但**不影响自动更新能否工作**。早期版本把两者合成一道门禁，导致没有证书就完全无法发布——那是错的。

| 项 | 当前值 | 后果 |
|---|---|---|
| `plugins.updater.pubkey` | 已配置 | 客户端可验证更新签名 |
| `plugins.updater.endpoints` | `releases/latest/download/latest.json` | 客户端能查到更新 |
| `bundle.createUpdaterArtifacts` | `true` | 每次打包都产出 `.sig` |
| `TAURI_SIGNING_PRIVATE_KEY` | 已存入 CI secret | 可签名 |
| Windows Authenticode | **未配置** | SmartScreen 警告，不影响更新 |
| Apple 公证 | **未配置** | macOS 无产物，暂不涉及 |

因此 tag 构建的唯一硬门禁是：**私钥存在**。缺私钥则拒绝构建，因为产出的包没人能装。缺证书只是警告级别的问题，README 里如实写明。

CI 与 tag 的关系：

- `push` 到 `main` 与所有 `pull_request`：只跑检查（`check`、`frontend`），不打包。
- `push` 一个 `v*` tag：检查 → `bundle`（Windows + macOS universal，用官方 `tauri-action`）→ 两个平台都上传完后 `publish` 取消草稿。
- 手工 `workflow_dispatch`：勾选 `build_installers` 才打包，产物作为 workflow artifact，不创建 release。

### 为什么用 tauri-action

`bundle` job 只做四件事：装依赖、检查私钥存在、导入证书（如果配了）、调用 `tauri-apps/tauri-action`。构建、签名、生成 `latest.json`、上传 release 都由官方 action 完成。

早期版本手写了 220 行来做同样的事，包括一个 minisign 验签器和一份清单合并脚本——那是在重新实现上游已经维护好的逻辑，而且每次 Tauri 升级都要跟着改。

### 真实桌面 e2e

`e2e-windows` 默认关闭。它需要一块真实桌面：托管 runner 只有一块虚拟显示器，也没有可交互会话。启用方式是手工 dispatch 时勾选 `run_e2e`，并且仓库里有一个带 `self-hosted`、`windows`、`x64`、`unfy-e2e` 标签的 runner，满足两块以上显示器、交互式桌面会话、会话未锁定。条件不满足时保持关闭——在假桌面上跑绿等于宣称一个用户并不拥有的环境可用。

同理，`ui-audit` 也需要真实 WebView2 窗口，托管 runner 上必然失败，因此同样只在自托管 runner 上运行。

## 1. 包型

| 平台 | 产物 | 命令 |
|---|---|---|
| Windows | NSIS 安装包（每用户，无需管理员） | `npm run tauri build -- --bundles nsis` |
| macOS | Universal app 与 DMG | `npm run tauri build -- --target universal-apple-darwin --bundles app,dmg` |
| Linux | **未实现**（v1 范围为 Windows + macOS） | — |

Windows 每用户安装刻意不要求管理员权限：应用本身不需要提权，只有用户自行选择安装到受管位置时才需要。

## 2. 签名

两件独立的事，不要混为一谈。

### 2.1 更新包签名（必需）

`tauri-plugin-updater` 对每个下载的更新包校验 minisign 签名。这是自动更新的**前提**，不是可选项：没有它，客户端会拒绝安装任何更新。

```bash
npx tauri signer generate -w ~/.tauri/unfy.key
```

- 公钥 → `tauri.conf.json` 的 `plugins.updater.pubkey`（已配置）
- 私钥 → CI secret `TAURI_SIGNING_PRIVATE_KEY`（已配置，无密码）
- 每次 `tauri build` 自动为更新包生成 `.sig`

**私钥丢失意味着现有用户永远收不到更新**，必须离线备份。换新私钥等于换验证钥匙：已安装的旧版会拒绝新签名，因此轮换必须连同一次人工升级路径规划。

### 2.2 原生代码签名（可选，消除安装警告）

不影响自动更新，只影响安装时的系统提示。

**Windows**：Authenticode 证书（OV 或 EV）。没有签名时 SmartScreen 拦截首次运行，用户需点「更多信息 → 仍要运行」；部分 EDR 会标记未签名二进制。

配置方式：设置 `bundle.windows.certificateThumbprint` 与 `timestampUrl`，并把 `WINDOWS_CERTIFICATE`（base64 PFX）、`WINDOWS_CERTIFICATE_PASSWORD` 存入 secret。CI 的导入步骤只在 `WINDOWS_CERTIFICATE` 非空时运行，因此当前未配置也不阻塞构建。

**macOS**：需要 Apple Developer Program、Developer ID Application 证书与公证凭据。三项缺一不可——代码签名、公证、装订（`xcrun stapler staple`）。通过 `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` 注入。

**TCC 授权的持久性依赖签名身份**：用户授予的「辅助功能」权限与 bundle id + Team ID 绑定。换证书或 bundle id 会让所有用户重新授权。所以从一开始就用固定的 Developer ID 与固定的 `app.unfy.desktop`。

## 3. 自动更新

更新源指向本仓库的 `releases/latest/download/latest.json`，由 `tauri-action` 在每次 tag 发布时自动生成。清单格式：

### 恢复屏障

“检查更新”只查询版本；安装是独立操作。后台安装遇到未恢复窗口、无法读取的记录或受支持的远程会话状态不明时延期，空闲后重试。下载完成后重新取得维护准入，阻止排队动作，再检查内存与递归磁盘恢复记录；仍有恢复责任就不安装。失败释放维护准入，保持应用可操作。

这条规则没有旁路：安全更新也一样等待。理由是一旦重启，用户面对的是一台布局错乱又没有任何解释的机器。

## 4. 首次发布检查清单

按顺序执行，每项都要有记录（命令输出、截图或链接）：

1. `cargo test --workspace` 全绿，`cargo clippy --workspace --all-targets -- -D warnings` 无告警。
2. `npx tsc --noEmit` 与 `npm run build` 通过。
3. §2.1 的更新签名就位（公钥、endpoint、`TAURI_SIGNING_PRIVATE_KEY`）。缺私钥时 tag 构建会直接失败，因为签不出客户端能接受的包。原生证书（§2.2）不是发布前提。
4. Windows 包在**干净虚拟机**上安装 → 运行 → 整理窗口 → 恢复 → 卸载，检查卸载后无残留进程与计划任务。
5. macOS 包在**干净系统**上安装 → 首次启动弹出辅助功能指引 → 授权 → 移动窗口 → 撤销授权 → 确认 UI 正确降级。
6. 更新演练：安装旧版本，指向测试清单，确认升级成功；再故意制造一次"有未恢复窗口"的状态，确认更新被推迟。
7. 用 `dump_positions`（`probe --dump`）对比升级前后窗口位置，确认升级本身没有移动任何窗口。
8. 检查诊断包内容：不得包含窗口标题、文件路径、屏幕画面、原始显示器序列号。
9. 确认 release 页面同时有安装包、`.sig` 和 `latest.json`。`tauri-action` 缺任一产物会让 job 失败，但文件是否完整可安装仍需人工下载核对。
10. 在具备交互式桌面的 Windows 机器上手工运行 `bun scripts/ui-audit.mjs`（24 个组合）。托管 runner 没有交互式会话，WebView2 不会创建窗口，因此该检查**不会**在 CI 里跑；`workflow_dispatch` 的 `run_ui_audit` 只在注册了 `unfy-e2e` 自托管 runner 后可用。

## 5. 已知限制

| 限制 | 影响 | 处置 |
|---|---|---|
| macOS 无法移动原生全屏窗口 | 全屏应用不参与整理 | 跳过并在结果中报告 `fullscreen` |
| macOS 无法跨 Space 操作 | 其他桌面上的窗口完全不可见 | 不承诺；AX 只暴露当前 Space |
| Windows 无法操作提权窗口 | 以管理员运行的程序保持原位 | 跳过并计入 `permission_denied` |
| 部分应用拒绝或回弹 | 请求提交后几何或模式未稳定 | 有界回读后报告真实结果；未知请求不叠加重试，恢复记录保留 |
| 无法得知远程端在看哪块屏 | 目标屏需要用户指定 | 显式选择；弱身份仅在本次运行的已确认拓扑有效 |
| 自动检测覆盖有限 | 第三方软件的进程、端口、虚拟屏不能证明入站连接 | 已验证 RDP 生命周期只触发离屏救援；其他软件保留手动操作 |

## 6. 未做的事

- 无 Linux 支持。
- 无虚拟显示器、无强制单屏模式。
- 无特权 helper，因此提权窗口不会被动到。
- 无遥测上传，也不保留无效的遥测设置项。
- macOS/Linux 没有已验证的远程连接检测，返回 Unknown；不能据此宣称支持自动整理。
- 无 License/支付。
