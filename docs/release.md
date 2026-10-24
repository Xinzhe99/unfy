# Unfy 发布说明

本文说明如何产出可分发安装包、如何配置签名与更新，以及每一项的风险边界。**不要把本文当作已验证的发布流程**：文中的签名、公证、商店审核均未在本机执行过，首次正式发布前必须逐项走通并记录证据。

## 0. 当前实际状态（先读这一节）

本仓库当前的发行链是**可达但故意不完整的**：

| 项 | 当前值 | 后果 |
|---|---|---|
| `tauri.conf.json` 的 `plugins.updater.pubkey` | 空字符串 | 客户端无法验证任何更新签名，自动更新不可用 |
| `plugins.updater.endpoints` | 空数组 | 客户端不会发起更新查询 |
| `bundle.createUpdaterArtifacts` | 未设置 | 不产出 `.sig` 与 macOS `.app.tar.gz` |
| `TAURI_SIGNING_PRIVATE_KEY` | 未配置 | 没有可用的签名私钥 |

因此**现在的 tag 构建会失败，这是设计意图，不是回归**。CI 的 `bundle` job 在编译之前检查上述四项，缺任何一项就以非零退出，并在日志里逐条列出缺什么。理由很直接：如果放行，产物会带着一个空的 updater 配置发布，而安装后的客户端既查不到更新，也无法验证更新；发布说明里一旦写了「支持自动更新」，就是假陈述。宁可让 tag 构建红，也不要发出一个自称能自更新的包。

把这些配置补齐之后，tag 构建才会真正走到打包步骤。补齐清单见 §3。

CI 与 tag 的关系：

- `push` 到 `main`/`master` 与所有 `pull_request`：只跑检查（`check`、`frontend`、`ui-audit`），不打包。
- `push` 一个 `v*` tag：跑全部检查，然后跑 `bundle`（Windows + macOS universal）。
- 手工 `workflow_dispatch`：同样会跑 `bundle`，但**不要求** updater 配置——用于在没配密钥的分支上做一次冒烟打包。这种构建只能用于本机验证，不能当发布件。

### CI 的发布产物

`bundle` job 每个平台产出两个 artifact：

| artifact | 内容 | 缺失时 |
|---|---|---|
| `unfy-Windows` / `unfy-macOS` | NSIS `.exe`（+ 可选 MSI）；DMG | **job 失败**（`if-no-files-found: error`） |
| `unfy-Windows-updater` / `unfy-macOS-updater` | `*.sig` 与 macOS `*.app.tar.gz` | tag 构建下 **job 失败**；手工 dispatch 下允许为空 |

打包完成后还有一步 `Verify release artifacts` 直接遍历 `target/`，按平台核对「安装包存在」，tag 构建再核对「至少一个 `.sig`，macOS 另有 `.app.tar.gz`」。这一层是必要的：上传步骤只对它自己被要求上传的模式负责，若只产出了安装包而没有更新包，两个上传都不会报错，但这次发布仍然是有缺口的。

**注意**：即使这些检查都通过，也**没有**任何步骤生成或上传更新清单（`latest.json`）。清单目前需要人工按 §3 的格式写并上传到 `endpoints` 指向的位置；CI 只负责把签名和更新包交出来。

### 真实桌面 e2e

`e2e-windows` 默认关闭（`if` 为假）。它需要一块真实桌面：托管 runner 只有一块虚拟显示器，也没有可交互的会话，跑出来的结论不成立。启用方式是手工 dispatch 时勾选 `run_e2e`，并且仓库里要有一个带 `self-hosted`、`windows`、`x64`、`unfy-e2e` 标签的 runner，满足：两块以上显示器（扩展而非复制）、runner 以交互式桌面会话运行（不是 Session 0 服务、不是无头）、会话未锁定且未断开 RDP。具体条件写在 `.github/workflows/ci.yml` 该 job 的注释里。条件不满足时保持关闭——在假桌面上跑绿，等于宣称一个用户并不拥有的环境可用。


## 1. 包型

| 平台 | 产物 | 命令 |
|---|---|---|
| Windows | NSIS 安装包（每用户，无需管理员）；可选 MSI | `npm run tauri build` |
| macOS | DMG（含 Universal app）；PKG 用于批量分发 | `npm run tauri build -- --target universal-apple-darwin` |
| Linux | **未实现**（v1 范围为 Windows + macOS） | — |

Windows 每用户安装刻意不要求管理员权限：应用本身不需要提权，只有用户自行选择安装到受管位置时才需要。

## 2. 签名

### Windows

需要 Authenticode 代码签名证书（OV 或 EV）。没有签名时：

- SmartScreen 会拦截首次运行，用户需要点击"更多信息 → 仍要运行"；
- 部分 EDR 产品会标记未签名二进制。

签名通过环境变量或在 `tauri.conf.json` 的 `bundle.windows.certificateThumbprint` 配置。**证书指纹对应的证书必须已导入构建机的证书存储**，Tauri 会按指纹查找。

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

补齐顺序（四项缺一不可，CI 按这个顺序检查并报错）：

1. `npx tauri signer generate -w ~/.tauri/unfy.key` 生成密钥对；
2. 把公钥填进 `plugins.updater.pubkey`；
3. 在 `bundle.createUpdaterArtifacts` 设为 `true`，否则不产出 `.sig` 与 macOS `.app.tar.gz`；
4. 把 `plugins.updater.endpoints` 指向真正会托管清单的 URL；
5. 把私钥内容存入 CI secret `TAURI_SIGNING_PRIVATE_KEY`（有密码时连同 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）。

私钥永不入库。**私钥丢失意味着现有用户永远无法再收到更新**，必须离线备份。换用新私钥等于换了一把验证钥匙：已安装的旧版本会拒绝新签名，因此轮换私钥必须连同一次人工升级路径一起规划，不能只改 secret。

清单需要人工生成并托管：CI 只产出 `*.sig` 与更新包，不产出清单文件。更新清单是一个静态 JSON，每个平台/架构一条，包含版本、下载地址、签名与发布日期。示例：

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

更新安装会重启进程，因此 `update::check_and_install` 在下载完成后、安装之前检查 `captured_window_count()`。只要还有窗口被当前事务持有，更新就会被推迟并发出 `unfy://update` 事件，UI 显示"恢复完成后再安装"。

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
| 部分 Chromium 系应用会回弹 | 少数窗口拒绝移动 | 有界重试后标记 `rejected`，UI 显示进程号 |
| 无法得知远程端在看哪块屏 | 目标屏需要用户指定 | 首次向导 + 热键循环 + 托盘提示 |
| 自动检测覆盖不完整 | TeamViewer/AnyDesk 等不置位任何系统标志 | 分级检测：RDP 与虚拟显示器可靠，进程级信号需连续两次确认；默认不自动移动窗口 |

## 6. 未做的事

- 无 Linux 支持。
- 无虚拟显示器、无强制单屏模式。
- 无特权 helper，因此提权窗口不会被动到。
- 无遥测上传（设置项存在，但没有任何上传代码）。
- 自动检测不覆盖 macOS/Linux：`unfy-detect` 在这两个平台返回空观测，检测器保持惰性。
- 无 License/支付。
