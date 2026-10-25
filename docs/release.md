# Unfy 发布说明

本文说明如何产出可分发安装包、如何配置签名与更新，以及每一项的风险边界。**不要把本文当作已验证的发布流程**：文中的签名、公证、商店审核均未在本机执行过，首次正式发布前必须逐项走通并记录证据。

## 0. 当前实际状态（先读这一节）

CI 已接通打包、签名验证、更新清单生成和 tag 发布；当前配置仍缺少真实发行身份与更新源。

| 项 | 当前值 | 后果 |
|---|---|---|
| `tauri.conf.json` 的 `plugins.updater.pubkey` | 空字符串 | 客户端无法验证任何更新签名，自动更新不可用 |
| `plugins.updater.endpoints` | 空数组 | 客户端不会发起更新查询 |
| `bundle.createUpdaterArtifacts` | CI 在 tag 构建时设为 `true`，手工烟测为 `false` | 发行产物与未签名烟测明确分开 |
| `TAURI_SIGNING_PRIVATE_KEY` | 未配置 | 没有可用的签名私钥 |

因此现在的 tag 构建会在打包前失败。门禁要求 tag 与应用版本一致、更新源指向当前仓库的 `releases/latest/download/latest.json`、存在 updater 公钥/私钥，以及对应平台的原生签名凭据。Windows 还要求证书指纹和时间戳服务。缺少任何一项均返回错误，不发布无法验证或无法更新的包。

把这些配置补齐之后，tag 构建才会真正走到打包步骤。补齐清单见 §3。

CI 与 tag 的关系：

- `push` 到 `main`/`master` 与所有 `pull_request`：只跑检查（`check`、`frontend`、`ui-audit`），不打包。
- `push` 一个 `v*` tag：跑全部检查，然后跑 `bundle`（Windows + macOS universal）。
- 手工 `workflow_dispatch`：只有勾选 `build_installers` 才运行 `bundle`，用于未签名安装烟测；不发布 release 或更新清单。

### CI 的发布产物

`bundle` 为每个平台上传安装包；tag 构建额外上传已经验签的发行输入：

| artifact | 内容 | 缺失时 |
|---|---|---|
| `unfy-Windows` / `unfy-macOS` | NSIS `.exe` / DMG | job 失败 |
| `release-Windows` / `release-macOS` | 已验签安装包、更新包、`.sig`、平台清单 | tag 构建缺失时 job 失败 |

Windows 检查应用与安装包的 Authenticode 签名、证书指纹和时间戳；macOS 检查签名、公证装订和 universal 架构。更新包同时验证 minisign 密钥 ID、包签名和 trusted comment 签名，并由实际包名生成下载 URL。

`release` job 合并三个必需平台键（Windows x64、macOS ARM64/x64）生成 `latest.json`，先创建包含全部文件的草稿，再发布。上述原生签名与 GitHub 发布尚未执行；本轮离线执行了当前验签脚本，有效 Ed/ED 签名通过，损坏包、错误密钥、错误密钥 ID 和损坏 trusted comment 均被拒绝。

### 真实桌面 e2e

`e2e-windows` 默认关闭（`if` 为假）。它需要一块真实桌面：托管 runner 只有一块虚拟显示器，也没有可交互的会话，跑出来的结论不成立。启用方式是手工 dispatch 时勾选 `run_e2e`，并且仓库里要有一个带 `self-hosted`、`windows`、`x64`、`unfy-e2e` 标签的 runner，满足：两块以上显示器（扩展而非复制）、runner 以交互式桌面会话运行（不是 Session 0 服务、不是无头）、会话未锁定且未断开 RDP。具体条件写在 `.github/workflows/ci.yml` 该 job 的注释里。条件不满足时保持关闭——在假桌面上跑绿，等于宣称一个用户并不拥有的环境可用。

## 1. 包型

| 平台 | 产物 | 命令 |
|---|---|---|
| Windows | NSIS 安装包（每用户，无需管理员） | `npm run tauri build -- --bundles nsis` |
| macOS | Universal app 与 DMG | `npm run tauri build -- --target universal-apple-darwin --bundles app,dmg` |
| Linux | **未实现**（v1 范围为 Windows + macOS） | — |

Windows 每用户安装刻意不要求管理员权限：应用本身不需要提权，只有用户自行选择安装到受管位置时才需要。

## 2. 签名

### Windows

需要 Authenticode 代码签名证书（OV 或 EV）。没有签名时：

- SmartScreen 会拦截首次运行，用户需要点击"更多信息 → 仍要运行"；
- 部分 EDR 产品会标记未签名二进制。

配置 `bundle.windows.certificateThumbprint` 与 `timestampUrl`；CI 使用 `WINDOWS_CERTIFICATE`（base64 PFX）和 `WINDOWS_CERTIFICATE_PASSWORD` 导入当前用户证书存储，并核对实际指纹。Tauri 按该指纹签名。

驱动签名与本产品无关：v1 不含 IDD 驱动。

### macOS

需要 Apple Developer Program 会员、Developer ID Application 证书与公证凭据。

三项缺一不可：

1. **代码签名**：`codesign` 使用 Developer ID，开启 Hardened Runtime；
2. **公证**：上传到 Apple notary service 并等待通过；
3. **装订**：`xcrun stapler staple` 把票据附到 DMG 上，否则离线首启会被 Gatekeeper 拦截。

CI 中通过 `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` 注入。这些变量名对应 Tauri 的 macOS 签名文档，**首次配置时必须核对目标 Tauri 版本的文档**，参数名在不同版本间有过变化。

**TCC 授权的持久性依赖于签名身份**：用户授予的「辅助功能」权限与 bundle id + Team ID 绑定。更换证书或 bundle id 会让所有用户重新授权；用 ad-hoc 签名开发会让每次重建都丢权限（Apple TN3127 的 designated requirement 绑定单次 cdhash）。所以从一开始就用固定的 Developer ID 与固定的 `app.unfy.desktop`。

## 3. 自动更新

使用 `tauri-plugin-updater`。配置在 `tauri.conf.json` 的 `plugins.updater`：

- `pubkey`：minisign 公钥，**当前为空**。为空时更新检查会失败，这不影响本地功能，但意味着这个构建不能被自动更新。
- `endpoints`：更新清单 URL。当前为空，因此不会发起网络请求。

配置顺序：

1. `npx tauri signer generate -w ~/.tauri/unfy.key` 生成密钥对；
2. 把公钥填进 `plugins.updater.pubkey`；
3. 确认 CI 的 tag 构建覆盖 `bundle.createUpdaterArtifacts=true`；本地发行打包也必须开启；
4. 把 `plugins.updater.endpoints` 指向当前 GitHub 仓库的 `releases/latest/download/latest.json`；
5. 把私钥内容存入 CI secret `TAURI_SIGNING_PRIVATE_KEY`（有密码时连同 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）。

私钥永不入库。**私钥丢失意味着现有用户永远无法再收到更新**，必须离线备份。换用新私钥等于换了一把验证钥匙：已安装的旧版本会拒绝新签名，因此轮换私钥必须连同一次人工升级路径一起规划，不能只改 secret。

CI 生成并随安装包发布清单，格式如下；平台地址与签名来自本次已验证的更新包：

```json
{
  "version": "0.2.0",
  "notes": "修复：以最大化状态打开的窗口不再遮挡同屏其他窗口",
  "pub_date": "2026-11-01T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<content of unfy_0.2.0_x64-setup.exe.sig>",
      "url": "https://example.com/unfy/0.2.0/unfy_0.2.0_x64-setup.exe"
    },
    "darwin-aarch64": {
      "signature": "<content of Unfy.app.tar.gz.sig>",
      "url": "https://example.com/unfy/0.2.0/Unfy.app.tar.gz"
    }
  }
}
```

macOS 打的是 universal 包，`darwin-aarch64` 与 `darwin-x86_64` 两个键都要写，指向同一个 `.app.tar.gz` 与同一份 `.sig`；只写一个键会让另一半架构的机器永远收不到更新。

### 恢复屏障

“检查更新”只查询版本；安装是独立操作。后台安装遇到未恢复窗口、无法读取的记录或受支持的远程会话状态不明时延期，空闲后重试。下载完成后重新取得维护准入，阻止排队动作，再检查内存与递归磁盘恢复记录；仍有恢复责任就不安装。失败释放维护准入，保持应用可操作。

这条规则没有旁路：安全更新也一样等待。理由是一旦重启，用户面对的是一台布局错乱又没有任何解释的机器。

## 4. 首次发布检查清单

按顺序执行，每项都要有记录（命令输出、截图或链接）：

1. `cargo test --workspace` 全绿，`cargo clippy --workspace --all-targets -- -D warnings` 无告警。
2. `npx tsc --noEmit` 与 `npm run build` 通过。
3. §3 的五项 updater 配置全部就位——否则 `v*` tag 构建会在编译前直接失败，这是有意为之的闸门。
4. Windows 包在**干净虚拟机**上安装 → 运行 → 整理窗口 → 恢复 → 卸载，检查卸载后无残留进程与计划任务。
5. macOS 包在**干净系统**上安装 → 首次启动弹出辅助功能指引 → 授权 → 移动窗口 → 撤销授权 → 确认 UI 正确降级。
6. 更新演练：安装旧版本，指向测试清单，确认升级成功；再故意制造一次"有未恢复窗口"的状态，确认更新被推迟。
7. 用 `dump_positions`（`probe --dump`）对比升级前后窗口位置，确认升级本身没有移动任何窗口。
8. 检查诊断包内容：不得包含窗口标题、文件路径、屏幕画面、原始显示器序列号。
9. 确认 CI 的 `bundle` job 真的产出了安装包与 `.sig`：artifact 缺失时该 job 会失败，绿勾本身就是「有产物」的证据；但仍需人工打开 artifact 核对文件是否完整可安装。

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
