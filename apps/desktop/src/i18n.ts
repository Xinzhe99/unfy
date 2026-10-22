/**
 * Interface language.
 *
 * # Why the Chinese source string is the key
 *
 * A key-based catalogue (`"overview.status.ready"`) needs a key invented for every
 * string and kept in sync across two dictionaries; a renamed key silently renders
 * its own name at runtime. Using the Chinese original as the lookup key means:
 *
 *   * a missing English entry falls back to Chinese rather than to `t.overview.…`;
 *   * the source reads naturally - `t("就绪")` instead of an indirection;
 *   * adding a string needs no registration step, only a translation when wanted.
 *
 * The cost is that a Chinese edit orphans its translation. That trade is right for
 * a two-language interface where the Chinese is the source of truth.
 *
 * # Interpolation
 *
 * `{name}` placeholders are substituted from the params object. The English entry
 * may reorder them freely, which is the point of not concatenating fragments.
 */

export type Locale = "zh" | "en";

/** What the user chose. `system` resolves from the OS at startup. */
export type LanguageSetting = "system" | "zh" | "en";

const STORAGE_KEY = "conflux.locale";

let active: Locale = "zh";

/** Detect the OS language once, at module load. */
function detectSystem(): Locale {
  // `navigator.language` is typed as `string` but is `undefined` in some embedded
  // webviews (Bun's test runtime included), so every access is checked rather than
  // trusted. The previous version called `toLowerCase()` on it and threw during
  // module load, which would have blanked the whole interface.
  const tags: unknown[] = [];
  if (typeof navigator !== "undefined") {
    tags.push(navigator.language);
    const list = navigator.languages;
    if (Array.isArray(list)) tags.push(list[0]);
  }
  for (const tag of tags) {
    if (typeof tag !== "string" || tag.length === 0) continue;
    if (tag.toLowerCase().startsWith("zh")) return "zh";
    return "en";
  }
  return "en";
}

export function resolveLocale(setting: LanguageSetting): Locale {
  if (setting === "zh") return "zh";
  if (setting === "en") return "en";
  return detectSystem();
}

export function currentLocale(): Locale {
  return active;
}

/**
 * Apply a locale and remember it for the session.
 *
 * Not persisted here: `settings.json` is the single source of truth, and a second
 * store would let the two disagree after a failed save.
 */
export function applyLocale(locale: Locale): void {
  active = locale;
  if (typeof document !== "undefined") {
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }
}

/** Stable across reloads within a session, used to force a re-render. */
export function localeKey(): string {
  if (typeof sessionStorage !== "undefined") {
    const stored = sessionStorage.getItem(STORAGE_KEY);
    if (stored) return stored;
  }
  return active;
}

export function rememberLocale(locale: Locale): void {
  if (typeof sessionStorage !== "undefined") {
    sessionStorage.setItem(STORAGE_KEY, locale);
  }
}

type Params = Record<string, string | number>;

/**
 * Translate, or return the Chinese source when no English entry exists.
 *
 * `params` are substituted into `{name}` placeholders. Numbers are rendered with
 * the locale's grouping only where it matters; here they are counts and sizes, so
 * plain digits are correct in both languages.
 */
export function t(zh: string, params?: Params): string {
  const template = active === "en" ? (EN[zh] ?? zh) : zh;
  return substitute(template, params);
}

function substitute(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, key: string) => {
    const value = params[key];
    return value === undefined ? match : String(value);
  });
}

/** English strings, keyed by the Chinese source. */
const EN: Record<string, string> = {
  // ---- app shell -------------------------------------------------------
  "概览": "Overview",
  "显示器": "Displays",
  "排除的应用": "Exclusions",
  "设置": "Settings",
  "使用": "Use",
  "主导航": "Main navigation",
  "就绪": "Ready",
  "正在计划": "Planning",
  "正在整理": "Gathering",
  "正在整理窗口": "Gathering windows",
  "布局已启用": "Layout active",
  "正在恢复": "Restoring",
  "部分受限": "Partially limited",
  "需要处理": "Needs attention",
  "已暂停": "Paused",
  "部分待恢复": "Partly restored",
  "当前平台未实现窗口控制": "Window control is not implemented on this platform",

  // ---- overview --------------------------------------------------------
  "当前状态": "Current state",
  "目标显示器": "Target display",
  "已收拢窗口": "Windows gathered",
  "待恢复窗口": "Windows to restore",
  "布局": "Layout",
  "远程检测": "Remote detection",
  "已关闭": "Off",
  "监视中（已暂停，不会自动操作）": "Watching (paused, no automation)",
  "监视中（{level}）": "Watching ({level})",
  "监视中（{level}，未启用自动操作）":
    "Watching ({level}, automation off)",
  "整理到目标屏": "Gather to target",
  "重新整理到目标屏": "Gather again",
  "预览": "Preview",
  "恢复本次布局": "Restore layout",
  "尝试恢复": "Try restoring",
  "暂停": "Pause",
  "取消暂停": "Resume",
  "保留当前布局": "Keep current layout",
  "识别屏幕编号": "Show display numbers",
  "识别编号会在每块屏幕中央短暂显示数字，用来对照远程画面确认是哪一块。":
    "Shows a number briefly in the centre of each screen, so you can match it against the remote view.",
  "标记为远程会话": "Mark as remote session",
  "结束远程会话标记": "End remote-session marker",
  "远程会话": "Remote session",
  "读取设置文件失败，已改用默认设置。原文件保留为 settings.json.invalid，可以手工修复后重启。":
    "The settings file could not be read, so defaults are in use. The original is kept as settings.json.invalid - fix it by hand and restart to recover.",
  "上次操作失败：{message}": "Last operation failed: {message}",
  "知道了": "Dismiss",
  "有窗口没能恢复到原位。可以再试一次，或先关闭这些窗口。":
    "Some windows could not be restored. Try again, or close those windows first.",
  "出现了无法自动处理的问题。恢复与救援仍然可用；如果反复失败，请导出诊断包。":
    "Something could not be handled automatically. Restore and rescue still work; if it keeps failing, export a diagnostics bundle.",
  "现在只能从本窗口或托盘触发整理。设置一个快捷键、或打开远程检测，之后就不必先切回这个窗口。":
    "Gathering is only reachable from this window or the tray. Set a shortcut, or turn on remote detection, to avoid switching back here first.",
  "保存的目标显示器（{display}）已断开，整理时会改用主显示器。":
    "The saved target display ({display}) is disconnected; gathering will use the primary display.",
  "重新选择目标显示器": "Choose the target again",
  "布局预览": "Layout preview",
  "预览不会移动任何窗口。窗口有变化时会自动更新。":
    "Preview moves nothing. It is a snapshot from the moment you pressed the button; leaving this page recomputes it.",
  "{count} 个窗口": "{count} windows",
  "没有可整理的窗口": "Nothing to gather",
  "将移动 {count} 个窗口": "Will move {count} windows",
  "空间不足，窗口将叠放": "Not enough room; windows will overlap",
  "{count} 个窗口放不下，保持原位": "{count} windows do not fit and stay put",
  "已跳过 {count} 个窗口": "Skipped {count} windows",
  "你选择的显示器当前不可用，这次会整理到主显示器。":
    "The display you chose is unavailable; this gather will use the primary display.",
  "最近活动": "Recent activity",
  "最近 {count} 次": "Last {count}",
  "还没有执行过操作。整理或恢复之后，这里会记录每次的结果。":
    "Nothing has run yet. Gathers and restores are recorded here with their results.",
  "{count} 个未处理": "{count} unsettled",
  "今天 {time}": "Today {time}",
  "整理": "Gather",
  "恢复": "Restore",
  "救援越界窗口": "Rescue off-screen windows",
  "自动整理": "Auto gather",
  "自动恢复": "Auto restore",
  "暂停状态变更": "Pause changed",
  "没有接受位置调整的应用": "Applications that refused to move",
  "排除": "Exclude",
  "全部排除": "Exclude all",
  "工具": "Tools",
  "导出诊断包": "Export diagnostics",
  "查看诊断内容": "View diagnostics",
  "打开配置目录": "Open config folder",
  "诊断包内容（导出时就是这个文件）":
    "Diagnostics contents (this is exactly what gets written)",
  "「救援」把停留在不存在坐标上的窗口移回可见区域。诊断包只含版本、显示器分辨率与设置项，不含窗口标题、文件路径或屏幕画面。":
    "Rescue moves windows stranded at coordinates that no longer exist back into view. The diagnostics bundle contains the version, display resolutions and settings only - never window titles, file paths or screen contents.",
  "诊断包已写入 {path}（{size}）。": "Diagnostics written to {path} ({size}).",
  "无法打开配置目录。": "Could not open the config folder.",
  "已在文件管理器中打开配置目录。": "Opened the config folder.",

  // ---- displays --------------------------------------------------------
  "选择远程端能看到的那块屏。更换屏幕后需要重新选择。":
    "Pick the screen the remote client can see. Re-select after swapping monitors.",
  "切换到下一块屏": "Next display",
  "在屏幕上显示编号": "Show numbers on screens",
  "无法截取画面": "No capture",
  "主显示器": "Primary",
  "显示器 {number}": "Display {number}",
  "显示器 {number}（主）": "Display {number} (primary)",
  "使用中": "In use",
  "卡片上的数字与「在屏幕上显示编号」时出现的一致，用远程画面里的数字对照即可。":
    "The numbers on these cards match the ones shown on the screens, so you can compare them against the remote view.",
  "保存的目标显示器已断开，整理时会改用主显示器。在下面重新选择一块屏即可恢复。":
    "The saved target display is disconnected; gathering will fall back to the primary. Pick a screen below to fix it.",
  "这块显示器的身份只能靠分辨率识别，恢复时会放回可见区域，而不是精确的原位置。":
    "This display is identified only by resolution, so a restore will place windows somewhere visible rather than exactly where they were.",
  "排列方式": "Arrangement",
  "等分排列，列数按窗口数量自动选择；焦点窗口放第一格。":
    "Even grid; the column count follows the number of windows, and the focused window takes the first cell.",
  "焦点窗口占主区域，其余在侧栏排列。":
    "The focused window takes the main area; the rest fill a side column.",
  "按原屏幕上的位置顺序依次放入，不受焦点影响。":
    "Windows are placed in the order they occupied on their original screens, whatever has focus.",
  "布局方式": "Arrangement",
  "网格": "Grid",
  "主次": "Main + side",
  "顺序": "In order",
  "边距": "Margin",
  "窗口与屏幕边缘的距离": "Distance between windows and the screen edge",
  "像素": "px",
  "窗口间距": "Gap",
  "相邻窗口之间的距离": "Distance between neighbouring windows",
  "未检测到显示器。": "No displays detected.",
  "编号正在显示，请对照屏幕确认。": "The numbers are on screen - check them against the remote view.",
  "已在每块屏幕上显示编号。": "Numbers are showing on every screen.",
  "无法显示编号，请检查屏幕状态。":
    "Could not show the numbers; check the display state.",
  "已切换到显示器 {number}，原有位置记录已保留。":
    "Switched to display {number}; the original position record is preserved.",

  // ---- exclusions ------------------------------------------------------
  // (duplicate removed: 排除的应用)
  "这些应用的窗口不会在整理时被移动，也不会在恢复时被处理。":
    "Windows of these applications are neither moved when gathering nor touched when restoring.",
  "{count} 条规则": "{count} rules",
  "还没有排除任何应用。": "No applications excluded yet.",
  "移除": "Remove",
  "这条规则已经存在。": "That rule is already in the list.",
  "例如 game.exe": "e.g. game.exe",
  "添加": "Add",
  "匹配可执行文件名，不区分大小写。全屏应用与工具窗口本来就不会被移动。":
    "Matches the executable name, case-insensitively. Full-screen applications and tool windows are never moved anyway.",
  "正在运行": "Running now",
  "从当前有窗口的应用中选择，避免手写文件名出错。":
    "Pick from the applications that currently own a window, instead of typing a name.",
  "确认清空": "Clear all",
  "清空规则": "Clear rules",
  "没有检测到有窗口的应用。应用打开后回到这一页会自动刷新。":
    "No applications with windows right now. This page refreshes itself when you come back after opening one.",
  "排除 {name}": "Exclude {name}",

  // ---- settings --------------------------------------------------------
  "外观": "Appearance",
  "主题": "Theme",
  "跟随系统会在系统切换深浅色时自动变化":
    "System follows the OS when it switches between light and dark",
  "跟随系统": "System",
  "浅色": "Light",
  "深色": "Dark",
  "远程会话检测": "Remote-session detection",
  "自动检测能识别到的远程会话，并按你的选择收拢或恢复窗口。":
    "Acts on remote sessions it can identify, gathering or restoring windows as you choose.",
  "远程端能看到": "Remote client sees",
  "一块屏": "One screen",
  "全部屏幕": "All screens",
  "既然全部屏幕都看得到，自动整理不会搬动任何窗口。":
    "Since every screen is visible, automatic gathering will not move anything.",
  "默认。远端只有一块屏，整理才有意义。":
    "Default. Gathering only makes sense when the remote client has a single screen.",
  "检测灵敏度": "Detection sensitivity",
  "只识别 Windows 远程桌面会话。几乎不会误判。":
    "Only Windows Remote Desktop sessions. Almost never wrong.",
  "还识别虚拟显示器（Parsec 等）。推荐。":
    "Also recognises virtual displays such as Parsec. Recommended.",
  "还识别已安装的远控工具是否在运行或已连接。可能误判。":
    "Also acts when a known remote tool is running or connected. Can misfire.",
  "关闭": "Off",
  "保守": "Cautious",
  "均衡": "Balanced",
  "激进": "Eager",
  "没有任何系统接口能查询「谁正在远程这台电脑」。检测依靠可验证的信号：远程桌面会话、虚拟显示器、已知远控工具的运行与连接状态。级别越高覆盖的软件越多，误判也越多。":
    "No system API can answer \"who is remoting into this machine\". Detection relies on verifiable signals: Remote Desktop sessions, virtual displays, and whether a known remote tool is running or connected. Higher levels cover more tools and misfire more often.",
  "检测到远程时自动整理": "Gather when a session starts",
  "关闭时会先把窗口恢复原位，再停止自动整理":
    "Turning this off restores the windows first, then stops automatic gathering",
  "关闭时只提示，不移动窗口":
    "When off it only notifies, never moves windows",
  "断开后自动恢复": "Restore when the session ends",
  "远程会话结束后把窗口放回原位":
    "Puts windows back after a remote session ends",
  "启动与运行": "Startup and behaviour",
  "开机自动启动": "Start at login",
  "登录后静默启动，不打开窗口":
    "Starts silently at login, without opening a window",
  "自动检查更新": "Check for updates automatically",
  "下载后等待窗口全部恢复再安装":
    "Downloads first; installs only once every window is restored",
  "快捷键": "Shortcuts",
  "使用建议组合": "Use suggested pair",
  "整理窗口": "Gather windows",
  "在其他应用中按下也能触发": "Works from any application",
  "恢复布局": "Restore layout",
  "把窗口放回整理前的位置": "Puts windows back where they were",
  "点一下再按组合键即可录制，Esc 取消，Backspace 清除。被系统或其他应用占用时会在这里显示原因。":
    "Click, then press the combination. Esc cancels, Backspace clears. If something else owns the combination, the reason appears here.",
  "未设置": "Not set",
  "请按下组合键…": "Press the combination…",
  "还需要一个非修饰键": "Needs a non-modifier key",
  "清除": "Clear",
  "录制快捷键": "Record shortcut",
  "恢复行为": "Restore behaviour",
  "恢复延迟": "Restore delay",
  "等这么多秒再放回窗口。期间如果又检测到远程会话，这次恢复会取消——避免断开重连时来回搬两次":
    "Waits this long before putting windows back. If a session is detected during the wait, the restore is cancelled - which avoids moving everything twice on a flaky connection.",
  "秒": "s",
  "更新": "Updates",
  "已配置更新源": "Update source configured",
  "未配置更新源": "No update source",
  "当前版本": "Version",
  "尚未检查过更新。": "No update check has run yet.",
  "最近一次结果：{outcome}（{version}）":
    "Last result: {outcome} ({version})",
  "此构建没有签名公钥与更新地址，检查更新不会成功。发布版才会启用。":
    "This build has no signing key or endpoint, so checking for updates cannot succeed. Release builds have both.",
  "检查更新": "Check for updates",
  "当前已是最新版本。": "Already up to date.",
  "更新已下载，将在窗口全部恢复后安装。":
    "Update downloaded; it will install once every window is restored.",
  "更新已安装，应用即将重启。": "Update installed; the app will restart.",
  "检查更新失败：{detail}": "Update check failed: {detail}",
  "已是最新": "Up to date",
  "已下载，等待安装": "Downloaded, waiting to install",
  "已安装": "Installed",
  "检查失败": "Check failed",
  "未知": "Unknown",
  "环境状态": "Environment",
  "Conflux 当前能不能正常工作。遇到问题时先看这里。":
    "Whether Conflux can currently do its job. Start here when something is wrong.",
  "正在检查…": "Checking…",
  "窗口控制权限": "Window-control permission",
  "可以移动其他应用的窗口。": "Other applications' windows can be moved.",
  "缺少「{name}」授权，整理不会生效。":
    "The \"{name}\" permission is missing, so gathering will not work.",
  "没有检测到显示器。": "No displays detected.",
  "只有一块显示器，整理没有可移动的目标。":
    "Only one display, so there is no target to gather onto.",
  "检测到 {count} 块显示器。": "{count} displays detected.",
  "全局快捷键": "Global shortcuts",
  "已注册，在其他应用中也生效。":
    "Registered; they work from any application.",
  "未设置。可通过托盘或本窗口触发。":
    "Not set. Gathering is available from the tray or this window.",
  "配置目录": "Config folder",
  "可写入 {path}": "Writable: {path}",
  "无法写入 {path}：设置和日志都不会保存。":
    "Cannot write to {path}: settings and logs will not be saved.",
  "未完成的整理": "Unfinished gather",
  "有 {count} 个窗口的快照等待恢复。":
    "{count} window snapshots are waiting to be restored.",
  "重置": "Reset",
  "恢复除目标显示器以外的所有设置为出厂默认。":
    "Restores every setting except the target display to its factory default.",
  "会清除排除规则、快捷键、自启、检测设置与恢复延迟。目标显示器保留，因为那是你对着屏幕确认过的结果。":
    "Clears exclusion rules, shortcuts, autostart, detection settings and the restore delay. The target display is kept, because you confirmed it by looking at your screens.",
  "恢复默认设置": "Restore defaults",
  "确认清除": "Yes, clear it",
  "取消": "Cancel",
  "已恢复默认设置，目标显示器已保留。":
    "Defaults restored; the target display was kept.",
  "隐私": "Privacy",
  "使用统计": "Usage statistics",
  "当前版本没有任何上传代码，这个开关只记录你的选择。将来若实现，只发送功能使用次数与错误类型，不含窗口标题与屏幕内容":
    "This build contains no upload code at all; the switch only records your choice. If it is ever implemented, it will send feature counts and error types only - never window titles or screen contents.",
  "语言": "Language",
  "界面语言": "Interface language",
  "跟随系统语言，可在需要时手动指定":
    "Follows the OS language; you can override it here",

  // ---- onboarding ------------------------------------------------------
  "跳过引导": "Skip",
  "第 {step} 步，共 {total} 步": "Step {step} of {total}",
  "上一步": "Back",
  "下一步": "Next",
  "开始使用": "Get started",
  "请先选择目标显示器。": "Choose a target display first.",
  "这个工具做什么": "What this tool does",
  "当你在只有一块屏的设备上远程回这台电脑时，其他屏幕上的窗口是看不见的。":
    "When you remote into this machine from a single-screen device, the windows on your other screens are invisible.",
  "Conflux 把这些窗口收拢到你指定的那块屏；远程结束后，再按之前的快照放回去。":
    "Conflux gathers them onto the screen you choose, then puts them back from a snapshot when the session ends.",
  "它不传输画面，也不建立远程连接。远程软件照常使用。":
    "It does not stream video or create a remote connection. Your remote software works as usual.",
  "指定远程端能看到的那块屏": "Choose the screen the remote sees",
  "下面的画面就是每块屏现在的真实内容。如果分不清是哪一块，先在屏幕上显示编号，再对照远程画面。":
    "These are live images of each screen. If you cannot tell them apart, show the numbers on the screens and compare them against the remote view.",
  "让它自动开始和结束": "Let it start and stop by itself",
  "Conflux 能识别一部分远程会话，在开始时整理、结束时恢复。检测依靠可验证的信号，级别越高覆盖越多，但也越容易误判。":
    "Conflux recognises some remote sessions: it gathers when one starts and restores when it ends. Detection relies on verifiable signals, so higher levels cover more tools but can misfire.",
  "手动": "Manual",
  "只用快捷键与托盘，检测保持关闭":
    "Shortcuts and tray only; detection stays off",
  "自动（推荐）": "Automatic (recommended)",
  "识别远程桌面会话与虚拟显示器，开始时整理、结束时恢复":
    "Recognises Remote Desktop sessions and virtual displays; gathers when one starts, restores when it ends",
  "随时可以在「设置 → 远程会话检测」里改。改之前不会自动移动任何窗口。":
    "Change this any time in Settings → Remote-session detection. Nothing moves automatically before you do.",
  "有些窗口不会被移动": "Some windows will not move",
  "有些窗口不会接受新的位置：有的会自己纠正，有的大到放不进格子。整理时它们留在原处。这不是失败：Conflux 会把它们的名字列出来，并给一个「排除」按钮。":
    "Some windows do not take the new position: a few snap back on their own, others are too large for the cell. They stay where they are, and that is not a failure: Conflux lists them and offers an Exclude button.",
  "排除之后，这些应用既不会被移动，也不会被列入每次的未处理项。":
    "Once excluded, they are neither moved nor counted as unsettled.",
  "全屏应用、最小化的窗口和系统外壳本来就会跳过。":
    "Full-screen applications, minimised windows and shell windows are skipped regardless.",
  "随时可以纠正": "You can always correct it",
  "整理之后，如果发现远程端看到的不是那块屏，用这两个入口立即纠正：":
    "If the remote client turns out to be showing a different screen, correct it immediately with either of these:",
  "托盘菜单": "Tray menu",
  "：右键托盘图标，选择「识别屏幕编号」或「重新整理到目标屏」。":
    ": right-click the tray icon and choose \"Show display numbers\" or \"Gather again\".",
  // (duplicate removed: 快捷键)
  "：在「设置」页录制。默认不占用任何组合键。":
    ": record them on the Settings page. No combination is taken by default.",
  "恢复入口永远免费可用，不需要任何账号。":
    "Restoring is always free and needs no account.",

  // ---- errors ----------------------------------------------------------
  "还有 {count} 个窗口没能恢复原位，已停止重新整理。请再点一次「恢复本次布局」，或先关闭这些窗口。":
    "{count} windows could not be returned. Re-gathering stopped. Try Restore layout again, or close those windows first.",
  "窗口仍处于整理状态。请先点「恢复本次布局」，再重新整理到新的目标屏。":
    "A layout is still active. Restore it before gathering onto a different screen.",
  "自动整理已暂停。在托盘菜单或概览页取消暂停后重试。":
    "Automation is paused. Resume it from the tray or the overview, then retry.",
  "系统拒绝了窗口操作权限，请检查授权后重试。":
    "The system denied window control. Check the permission and retry.",
  "已取消这次恢复，因为期间开始了新的整理。":
    "This restore was cancelled because a new gather started.",
  "自动处理失败：{message}": "Automation failed: {message}",
  "检测到{evidence}，已整理 {count} 个窗口。":
    "Detected {evidence}; gathered {count} windows.",
  "检测到{evidence}。自动整理已关闭，可点「整理到目标屏」。":
    "Detected {evidence}. Automatic gathering is off; press Gather to target.",
  "检测到{evidence}，但 30 秒内已经整理过一次，这次跳过。需要时可直接点「整理到目标屏」。":
    "Detected {evidence}, but a gather already ran within 30 seconds, so this one was skipped. Press Gather to target if you need it.",
  "远程会话已结束，已恢复 {count} 个窗口。":
    "Remote session ended; restored {count} windows.",
  "还有窗口没有恢复，已取消退出。在概览页可以再试一次恢复，或点「保留当前布局」。":
    "Some windows were not restored, so quitting was cancelled. Retry from the overview, or press Keep current layout.",
  "已整理 {count} 个窗口": "Gathered {count} windows",
  "，{count} 个已关闭": ", {count} closed",
  "，{count} 个放不下留在原位": ", {count} did not fit and stayed put",
  "，跳过 {count} 个": ", skipped {count}",
  "；{apps} 没有接受位置调整": "; {apps} refused to move",
  "已恢复 {count} 个窗口": "Restored {count} windows",
  "保留你的改动 {count} 个": "kept your changes for {count}",
  "原屏不可用，回退到可见区域 {count} 个":
    "original display gone, {count} placed in a visible area",
  "{count} 个没有接受恢复位置": "{count} refused the restored position",
  "仍有 {count} 个待处理": "{count} still pending",
  "已暂停自动整理，当前布局保留。":
    "Automation paused; the current layout is kept.",
  "已恢复自动整理，并补做了暂停期间欠下的恢复。":
    "Automation resumed, and the restore owed from the pause was completed.",
  "已恢复自动整理。": "Automation resumed.",
  "没有发现越界窗口。": "No off-screen windows found.",
  "已把 {count} 个窗口移回可见区域。":
    "Moved {count} windows back into view.",
  "已标记为远程会话。开启了自动整理的话，窗口会按设置收拢。":
    "Marked as a remote session. If automatic gathering is on, windows will be gathered accordingly.",
  "已结束远程会话标记。": "Remote-session marker cleared.",
  "活动历史": "Activity history",
  "记录了每次整理、恢复与救援的时间与窗口数量。清除后无法恢复，其他设置不受影响":
    "Records the time and window count of every gather, restore and rescue. Clearing it cannot be undone; no other setting is affected",
  "清除历史": "Clear history",
  "这些窗口没有接受新的位置：有的会自己纠正，有的大到放不进格子。排除它们可以避免每次都重试。":
    "These windows did not take the new position: some snap back on their own, others are too large for the cell. Excluding them stops the retry.",
  "仍在原处：": "Still in place:",
  "排除这些应用": "Exclude these apps",
  "显示全部 {count} 条": "Show all {count}",
  "收起": "Show fewer",
  "活动历史已清除。": "Activity history cleared.",
  "虚线框的窗口比格子需要更大，会略微压到相邻窗口（{count} 个）":
    "Dashed windows need more room than their cell and will overlap their neighbours slightly ({count})",
  "：比它的格子需要更大，会压到相邻窗口":
    ": needs more room than its cell, so it will overlap its neighbours",
  "帮助": "Help",
  "重新显示引导": "Show the guide again",
  "重新显示": "Show again",
  "五步走一遍：这个工具做什么、哪块屏是远端能看到的那块、以及如何随时纠正":
    "Walks through five steps: what this tool does, which screen the remote client sees, and how to correct it",
  "设置已保存，但当前整理还在进行中，新的布局会在下次整理时生效。":
    "Settings saved, but a gather is still running; the new layout applies from the next gather.",
  "已关闭自动整理，但恢复没有完成：{error}。可以在概览页重试。":
    "Automatic gathering is off, but the restore did not finish: {error}. You can retry from the overview.",
  "无效的组合键：{error}": "Not a valid shortcut: {error}",
  "{name} 已连接": "{name} connected",
  "虚拟显示器 {name}": "Virtual display {name}",
  "{name} 正在运行": "{name} running",
  "远程桌面会话": "Remote Desktop session",
  "手动标记": "Marked by hand",
  "{count} 个": "{count}",
  "正在载入…": "Loading…",
  "正在恢复布局…": "Restoring the layout…",
  "显示器连接发生变化。当前布局可能已不适用，建议重新点「整理到目标屏」。":
    "The display setup changed. The current layout may no longer fit, so gather to the target again.",
  "，": ", ",
  "。": ".",
  " · 已暂停": " · paused",
  "已请求授权。授权后请点「重新检查」。":
    "Permission requested. Press Recheck once it is granted.",
  "已隐藏提示，恢复记录仍然保留。": "Hidden. The restore record is kept.",
  "没有可整理的窗口。": "No windows to gather.",
  "已在每块屏幕上显示编号，4 秒后消失。":
    "Screen numbers are showing and disappear after 4 seconds.",
  "有 {count} 个窗口的快照仍待处理{note}。":
    "Snapshots for {count} windows are still pending{note}.",
  "，其中部分可能已被移动": ", and some of them may already have moved",
  "快捷键 {keys}，点击重新录制": "Shortcut {keys}; click to record again",
  "布局预览：{placed} 个窗口排入 {width}×{height} 的区域":
    "Layout preview: {placed} windows fitted into {width}×{height}",
  "窗口 {number}": "Window {number}",
  "未生效：{reason}": "Not in effect: {reason}",
  "已检测到": "Detected",
  "正在恢复…": "Restoring…",
  "没有画面": "No picture",
  "需要「{name}」权限才能移动窗口":
    "Moving windows requires the {name} permission",
  "辅助功能": "Accessibility",
  "窗口控制": "Window control",
  "无法写入恢复记录，为避免窗口位置丢失，整理已停止。请确认磁盘空间与目录权限，然后重启应用。":
    "The restore record could not be written, so gathering stopped rather than risk losing window positions. Check free disk space and folder permissions, then restart the app.",
  "目标显示器当前不可用，请在「显示器」页重新选择。":
    "The target display is unavailable. Choose one on the Displays page.",
  "没有可用的显示器": "No display available",
  "没有检测到显示器": "No displays detected",
  "自动整理已暂停": "Automatic gathering is paused",
  "还有未完成的整理，请先恢复":
    "A previous gather has not been restored yet",
  "恢复已被新的整理请求取消": "The restore was cancelled by a new gather",
  "不可见": "Not visible",
  "全屏": "Full screen",
  "权限不足": "Permission denied",
  "已最小化": "Minimised",
  "已排除此应用": "Excluded app",
  "还有 {count} 个窗口没能恢复原位":
    "{count} windows could not be put back, so re-gathering was stopped.",
  "、": ", ",
  "{list} 等": "{list} and more",
  "{reason} · {count} 个窗口": "{reason} · {count} windows",
  "（{apps}）": "({apps})",
  "关闭自动检测，只用快捷键、托盘和界面按钮。":
    "Turns detection off; gather from a shortcut, the tray or this window.",
  "检测到远程会话": "Remote session detected",
  "已选择": "Selected",
  "不可用": "Unavailable",
  "先恢复了 {count} 个窗口": "{count} windows restored first",
  "已保存的显示器": "the saved display",
  "正在整理 {done}/{total}…": "Gathering {done}/{total}…",
  "正在整理…": "Gathering…",
  "已保留当前布局，之后可以重新整理到目标屏。":
    "Current layout kept; you can gather again at any time.",
  "接受了当前布局，不再恢复": "Accepted the current layout; no restore",

  "跟随主显示器": "Follow the primary display",
  "（快捷键）": " (shortcut)",
  "（托盘）": " (tray)",
  "{name}{trigger} · {count} 个窗口": "{name}{trigger} · {count} windows",

  "{year}年{month}月{day}日 {time}":
    "{year}-{month}-{day} {time}",
  "{month}月{day}日 {time}": "{month}/{day} {time}",


  "{seconds} 秒后恢复布局。点「保留当前布局」可以取消。":
    "Restoring in {seconds}s. Choose Keep current layout to cancel.",
  "显示器连接发生变化，已把 {count} 个窗口移回可见区域。":
    "Displays changed; {count} windows were moved back into view.",
  "新版本 {version} 已下载，将在窗口全部恢复后安装。":
    "Version {version} downloaded; it will install once every window is restored.",
  "未知原因": "unknown reason",

  "当前平台尚未实现窗口控制": "Window control is not implemented on this platform",
  "当前只检测到一块显示器，没有需要整理的窗口。接上第二块屏、或远程端开始投送多屏后再来。":
    "Only one display is connected, so there is nothing to gather. Connect a second display, or start streaming multiple screens from the remote client.",
  "界面可以浏览，但不会移动任何窗口。":
    "The interface works, but no windows will be moved.",

  "正在读取设置…": "Reading settings…",
  "窗口如何排列在目标屏上。空间不足时自动降级，不会压成不可用的小格。":
    "How windows are arranged on the target screen. When space runs out it degrades gracefully instead of squeezing them into unusable slivers.",
  "预览、查看显示器与导出诊断仍可使用，只是不会移动窗口。":
    "Preview, display listing and diagnostics still work; windows simply will not move.",
  "请求授权": "Grant permission",
  "前往系统设置": "Open system settings",
  "重新检查": "Check again",
  "上次整理未完成": "A previous gather did not finish",
  "稍后处理": "Later",

};
