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

const STORAGE_KEY = "unfy.locale";
/** Persisted across launches, unlike `STORAGE_KEY`; see `readCachedLocale`. */
const CACHE_KEY = "unfy-locale";

/**
 * The language the last run ended in.
 *
 * The settings file is the source of truth, but it arrives over IPC - after the first
 * render. With `active` hardcoded to `zh`, an English user watched the navigation paint in
 * Chinese for a frame and then swap; measured with a per-frame recorder, the two texts
 * appear in consecutive frames. Reading the cache at module load means the first render is
 * already right, and `applyLocale` corrects it if the two disagree.
 */
function readCachedLocale(): Locale | null {
  try {
    const stored = localStorage.getItem(CACHE_KEY);
    return stored === "zh" || stored === "en" ? stored : null;
  } catch {
    // Storage disabled: fall through to the source language.
    return null;
  }
}

let active: Locale = readCachedLocale() ?? "zh";

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
  try {
    localStorage.setItem(CACHE_KEY, locale);
  } catch {
    // Storage disabled: the cache is an optimisation, not the source of truth.
  }
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
  "监视中（未启用自动操作）": "Watching (automation off)",
  "监视中": "Watching",
  "整理到目标屏": "Gather to target",
  "重新整理到目标屏": "Gather again",
  "预览": "Preview",
  "恢复本次布局": "Restore layout",
  "尝试恢复": "Try restoring",
  "暂停": "Pause",
  "取消暂停": "Resume",
  "保留当前布局": "Keep current layout",
  "这次整理的恢复位置将被放弃，之后不能用本次记录恢复。":
    "The recovery positions from this gather will be discarded; this record cannot restore them afterwards.",
  "放弃恢复记录": "Discard the recovery record",
  "识别屏幕编号": "Show display numbers",
  "识别编号会在每块屏幕中央短暂显示数字，用来对照远程画面确认是哪一块。":
    "Shows a number briefly in the centre of each screen, so you can match it against the remote view.",
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
  "保存的目标显示器（{display}）已断开。请重新选择，Unfy 不会改用到主显示器。":
    "The saved target display ({display}) is disconnected. Choose again; Unfy will not switch to the primary display on its own.",
  "重新选择目标显示器": "Choose the target again",
  "布局预览": "Layout preview",
  "预览不会移动任何窗口。窗口有变化时会自动更新。":
    "Preview moves no windows and updates when the window list changes.",
  "{count} 个窗口": "{count} windows",
  "没有可整理的窗口": "Nothing to gather",
  "将移动 {count} 个窗口": "Will move {count} windows",
  "空间不足，窗口将叠放": "Not enough room; windows will overlap",
  "窗口按原大小移入，允许叠放；用任务栏或窗口切换访问。":
    "Windows keep their size and may overlap; use the taskbar or the window switcher to reach them.",
  "{count} 个窗口放不下，保持原位": "{count} windows do not fit and stay put",
  "已跳过 {count} 个窗口": "Skipped {count} windows",
  "这份预览的目标显示器已经不可用，请重新选择后再整理。":
    "This preview's target display is no longer available. Choose again before gathering.",
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
  "开机自启未能设置：{error}":
    "Autostart could not be set: {error}",
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
  "保存的目标显示器已断开。在下面重新选择一块屏；确认之前 Unfy 不会搬动窗口。":
    "The saved target display is disconnected. Pick a screen below; Unfy will not move windows until a target is confirmed.",
  "这块显示器的身份只能靠分辨率识别，恢复时会放回可见区域，而不是精确的原位置。":
    "This display is identified only by resolution, so a restore will place windows somewhere visible rather than exactly where they were.",
  "排列方式": "Arrangement",
  "等分排列，列数按窗口数量自动选择；焦点窗口放第一格。":
    "Even grid; the column count follows the number of windows, and the focused window takes the first cell.",
  "保留窗口大小和最大化状态，把其他屏的窗口移过来。目标屏上已有的窗口保持原样。":
    "Keeps each window's size and maximised state, and moves windows from the other screens onto the target. Windows already on the target stay where they are.",
  "布局方式": "Arrangement",
  "网格": "Grid",
  "移到目标屏": "Move to target screen",
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

  // ---- exclusions ------------------------------------------------------
  // (duplicate removed: 排除的应用)
  "以后整理时跳过这些应用；已经整理的窗口仍可恢复。":
    "These applications are skipped by future gathers; windows already gathered can still be restored.",
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
  "远程桌面（RDP）会话建立时整理、结束时恢复。其他远控软件需要手动整理。":
    "Gathers when an RDP session starts and restores when it ends. Other remote tools require manual gathering.",
  "远程端能看到": "Remote client sees",
  "一块屏": "One screen",
  "全部屏幕": "All screens",
  "既然全部屏幕都看得到，自动整理不会搬动任何窗口。":
    "Since every screen is visible, automatic gathering will not move anything.",
  "默认。远端只有一块屏，整理才有意义。":
    "Default. Gathering only makes sense when the remote client has a single screen.",
  "关闭": "Off",
  "只在可验证的远程桌面（RDP）会话开始和结束时自动整理、恢复。远控软件在后台运行、或存在虚拟显示器，都不算连接。":
    "Gathers and restores only when a verified Remote Desktop (RDP) session starts and ends. A remote tool running in the background, or a virtual display appearing, does not count as a connection.",
  "没有任何系统接口能查询「谁正在远程这台电脑」。这里只在远程桌面（RDP）会话真正建立和结束时自动动手；远控工具在后台运行、端口已连接、或存在虚拟显示器，都只作为信息显示，不会触发移动。":
    "No generic API identifies every remote controller. Background tools, connected ports and virtual displays are information only and do not trigger moves.",
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
  "自动更新": "Automatic updates",
  "有未完成的整理时会跳过这次安装，下次启动再检查":
    "An unfinished gather blocks installation; another check can run at the next launch",
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
  "新版本 {version} 已下载，但当前有未完成的整理，尚未安装。恢复完成后可以再试。":
    "Version {version} is downloaded, but a gather is unfinished, so it is not installed yet. Retry once the restore is done.",
  "新版本 {version} 已下载并验签，准备安装。":
    "Version {version} is downloaded and verified; ready to install.",
  "新版本已下载，但当前有未完成的整理，尚未安装。恢复完成后可以再试。":
    "The update is downloaded, but a gather is unfinished, so it is not installed yet. Retry once the restore is done.",
  "更新已安装，应用即将重启。": "Update installed; the app will restart.",
  "检查更新失败：{detail}": "Update check failed: {detail}",
  "已是最新": "Up to date",
  "已下载，等待安装": "Downloaded, waiting to install",
  "已安装": "Installed",
  "检查失败": "Check failed",
  "未知": "Unknown",
  "环境状态": "Environment",
  "Unfy 当前能不能正常工作。遇到问题时先看这里。":
    "Whether Unfy can currently do its job. Start here when something is wrong.",
  "正在检查…": "Checking…",
  "窗口控制权限": "Window-control permission",
  "可以移动其他应用的窗口。": "Other applications' windows can be moved.",
  "缺少「{name}」授权，整理不会生效。":
    "The \"{name}\" permission is missing, so gathering will not work.",
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
  "Unfy 把这些窗口收拢到你指定的那块屏；远程结束后，再按之前的快照放回去。":
    "Unfy gathers them onto the screen you choose, then puts them back from a snapshot when the session ends.",
  "它不传输画面，也不建立远程连接。远程软件照常使用。":
    "It does not stream video or create a remote connection. Your remote software works as usual.",
  "指定远程端能看到的那块屏": "Choose the screen the remote sees",
  "下面的画面就是每块屏现在的真实内容。如果分不清是哪一块，先在屏幕上显示编号，再对照远程画面。":
    "These are live images of each screen. If you cannot tell them apart, show the numbers on the screens and compare them against the remote view.",
  "让它自动开始和结束": "Let it start and stop by itself",
  "手动": "Manual",
  "只用快捷键与托盘，检测保持关闭":
    "Shortcuts and tray only; detection stays off",
  "Unfy 可以在可验证的远程会话开始时整理、结束时恢复。目前只有远程桌面（RDP）会话能提供这种可验证的生命周期；其他远控软件即使正在运行，也不会自动搬动窗口。":
    "Unfy can gather when a verifiable remote session starts and restore when it ends. Only a Remote Desktop (RDP) session currently provides that verifiable lifecycle; other remote tools never move windows automatically, even while running.",
  "远程桌面会话时自动": "Automatically on Remote Desktop sessions",
  "仅在确认的远程桌面（RDP）会话开始和结束时自动整理、恢复":
    "Gathers and restores only when a confirmed Remote Desktop (RDP) session starts and ends",
  "随时可以在「设置 → 远程会话检测」里改。改之前不会自动移动任何窗口。":
    "Change this any time in Settings → Remote-session detection. Nothing moves automatically before you do.",
  "有些窗口不会被移动": "Some windows will not move",
  "有些窗口不会接受新的位置：有的会自己纠正，有的大到放不进格子。整理时它们留在原处，不是失败：Unfy 会列出它们的名字，你可以逐个选择「不再整理此应用」。":
    "Some windows do not take the new position: a few snap back on their own, others are too large for the cell. They stay where they are, and that is not a failure: Unfy lists them and lets you mark an application \"Never gather this app\".",
  "这个选择只影响以后的整理；已经整理过的窗口仍然可以恢复。":
    "That choice only affects later gathers; windows that were already gathered can still be restored.",
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
  "已整理 {count} 个窗口": "Gathered {count} windows",
  "，{count} 个已在目标屏": ", {count} already on the target",
  "，{count} 个已关闭": ", {count} closed",
  "，{count} 个放不下留在原位": ", {count} did not fit and stayed put",
  "，{count} 个未能读取原位置，未移动":
    ", {count} had no readable original position and were not moved",
  "，跳过 {count} 个": ", skipped {count}",
  "；{apps} 没有接受位置调整": "; {apps} refused to move",
  "已恢复 {count} 个窗口": "Restored {count} windows",
  "保留你的改动 {count} 个": "kept your changes for {count}",
  "原屏不可用，回退到可见区域 {count} 个":
    "original display gone, {count} placed in a visible area",
  "{count} 个没有接受恢复位置": "{count} refused the restored position",
  "{count} 个原屏仍未恢复，暂放在可见区域":
    "{count} still owe their original screen and are kept in a visible area",
  "仍有 {count} 个待处理": "{count} still pending",
  "已暂停自动整理，当前布局保留。":
    "Automation paused; the current layout is kept.",
  "已恢复自动整理。": "Automation resumed.",
  "没有发现越界窗口。": "No off-screen windows found.",
  "已把 {count} 个窗口移回可见区域。":
    "Moved {count} windows back into view.",
  "活动历史": "Activity history",
  "记录了每次整理、恢复与救援的时间与窗口数量。清除后无法恢复，其他设置不受影响":
    "Records the time and window count of every gather, restore and rescue. Clearing it cannot be undone; no other setting is affected",
  "清除历史": "Clear history",
  "这些窗口没有接受新的位置：有的会自己纠正，有的大到放不进格子。以后整理时跳过某个应用可以避免每次都重试；这只影响之后的整理，已经整理的窗口仍然可以恢复。":
    "These windows did not take the new position: some snap back on their own, others are too large for the cell. Skipping an application in future gathers stops the retry; that only affects later gathers, and windows already gathered can still be restored.",
  "仍在原处：": "Still in place:",
  "不再整理此应用": "Never gather this app",
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
  "正在退出或安装更新，暂时不能整理。请等它完成。":
    "Quitting or installing an update, so gathering is unavailable right now. Wait for it to finish.",
  "还没有选择目标显示器。请先在「显示器」页选择远程画面中能看到的那块屏。":
    "No target display chosen yet. Pick the screen the remote client can see on the Displays page.",
  "保存的目标显示器当前不在桌面上。请重新选择，或先接回那块屏。":
    "The saved target display is not on the desktop right now. Choose again, or plug that screen back in.",
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
    "{count} windows could not be put back.",
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


  "显示器连接发生变化，已把 {count} 个窗口移回可见区域。":
    "Displays changed; {count} windows were moved back into view.",
  "未知原因": "unknown reason",

  "当前平台尚未实现窗口控制": "Window control is not implemented on this platform",
  "当前只检测到一块显示器，没有需要整理的窗口。接上第二块屏、或远程端开始投送多屏后再来。":
    "Only one display is connected, so there is nothing to gather. Connect a second display, or start streaming multiple screens from the remote client.",
  "界面可以浏览，但不会移动任何窗口。":
    "The interface works, but no windows will be moved.",

  "正在读取设置…": "Reading settings…",
  "窗口如何排列在目标屏上。空间不足时自动降级，不会压成不可用的小格。":
    "How windows are arranged on the target screen. When space runs out it degrades gracefully instead of squeezing them into unusable slivers.",
  "授权后才能查看窗口预览和移动窗口；显示器信息与诊断仍可使用。":
    "Window previews and moves require permission. Display information and diagnostics remain available.",
  "请求授权": "Grant permission",
  "前往系统设置": "Open system settings",
  "重新检查": "Check again",
  "上次整理未完成": "A previous gather did not finish",
  "稍后处理": "Later",
  "有 {count} 份恢复记录暂时无法读取。记录已保留，请重试检查或查看诊断。": "{count} recovery records cannot currently be read. They are retained; recheck or view diagnostics.",
  "恢复记录：{count} 个窗口": "Windows in this recovery record: {count}",
  "查看并恢复": "Review and restore",
  "已保存恢复位置": "Recovery positions saved",
  "恢复记录需要处理。请重试检查或恢复；记录会继续保留。": "Recovery records need attention. Recheck or retry restoring; the records remain saved.",
  "有窗口仍需处理。可以重试恢复，或查看最近一次结果。": "Some windows still need attention. Retry restoring or review the latest result.",
  "仍待恢复：": "Still awaiting restoration:",
  "收拢新窗口": "Gather new windows",
  "整理新窗口": "Gather new windows",
  "当前布局仍在生效：把之后打开或移出目标屏的窗口也整理过去，已整理的窗口保持不动。恢复记录会继续保留。":
    "The current layout is still in effect: windows opened later or dragged off the target are gathered too. Windows already gathered stay where they are, and the recovery record is kept.",
  "选择目标显示器": "Choose target display",
  "当前只有一块显示器；仍可找回越界窗口。": "Only one display is connected; off-screen windows can still be rescued.",
  "预览不会移动窗口；目标或设置变化后请重新预览。": "Preview moves no windows. Preview again after changing the target or settings.",
  "更多": "More",
  "影响 {count} 个窗口": "Affects {count} windows",
  "保留当前位置并结束本次整理": "Keep current positions and end this gather",
  "尚未选择目标显示器": "No target display selected",
  "远程连接已结束，{seconds} 秒后恢复。取消只停止本次计时，恢复记录会保留。": "The remote connection ended. Restoring in {seconds}s. Cancelling stops this countdown and keeps recovery records.",
  "立即恢复": "Restore now",
  "取消本次自动恢复": "Cancel this automatic restore",
  "恢复记录仍待处理": "Recovery records still need attention",
  "查看恢复记录": "View recovery records",
  "刷新画面": "Refresh images",
  "更换目标显示器": "Change target display",
  "将已整理的窗口移到显示器 {number}，并保留最初的恢复位置。": "Move gathered windows to display {number} while keeping their original recovery positions.",
  "移到这块屏": "Move to this display",
  "卡片显示 Unfy 编号，与屏幕上的编号一致。截图不可用时仍可按编号选择。": "Cards show Unfy numbers matching the on-screen labels. You can select by number when screenshots are unavailable.",
  "暂时无法获取预览，仍可按编号选择": "Preview unavailable; select by number",
  "可靠连接触发": "Verified connection trigger",
  "关闭后停止新的自动整理；已有恢复记录保留。": "Turning this off stops new automatic gathers and retains existing recovery records.",
  "只有已验证的远程桌面（RDP）连接能触发自动操作。远控进程、已连接端口和虚拟显示器都只提供信息。": "Only verified RDP connections can trigger automation. Remote processes, connected ports and virtual displays provide information only.",
  "仅影响断开连接后的自动恢复；手动恢复立即开始。": "Only affects automatic restoration after disconnecting; manual restoration starts immediately.",
  "开发版本，自动更新不可用。": "Development build. Automatic updates are unavailable.",
  "确认这台主机与目标显示器，然后开始使用。": "Confirm this host and the target display, then get started.",
  "移动预览：{placed} 个窗口按原大小移到 {width}×{height} 的区域": "Move preview: {placed} windows keep their sizes in a {width}×{height} area",
  "：已调整到可操作区域，可能超出屏幕边缘": ": positioned within reach; may extend beyond the screen edge",
  "{count} 个窗口会调整到可操作区域，保留大小。": "{count} windows will be positioned within reach while keeping their sizes.",
  "安装在被控主机": "Run on the computer being controlled",
  "请在需要移动窗口的被控电脑上运行 Unfy。它不传输画面，也不建立远程连接。": "Run Unfy on the remote computer whose windows you want to move. It does not stream video or create a remote connection.",
  "选择目标后，你可以手动整理一次，再恢复整理前布局。快捷键与自动化可稍后在设置中选择。": "After choosing a target, gather windows manually and restore their previous layout. Configure shortcuts and automation later in Settings.",
  "选择只保存目标，不会移动窗口。画面不可用时可用 Unfy 编号确认。": "Selecting only saves the target; no windows move. Use Unfy numbers when images are unavailable.",
  "如果需要窗口控制权限，进入概览后按提示授权；未授权时不会移动窗口。": "If window-control permission is needed, follow the prompt on Overview. Windows will not move without permission.",
  "已验证的 RDP 连接仅找回越界窗口，不重排可见窗口，也不会自动恢复手动整理。其他工具请手动整理和恢复。": "Verified RDP connections only rescue off-screen windows. Visible windows are not rearranged, and manual gathers are not automatically restored. Use manual gather and restore with other tools.",
  "当前平台仅支持手动整理与恢复，自动连接检测不可用。": "Automatic connection detection is unavailable on this platform. Use manual gather and restore where window control is supported.",
  "安装更新并重启": "Install update and restart",
  "发现新版本": "Update available",
  "已在目标屏": "Already on target",
  "此窗口组无法安全一起移动": "This window group cannot safely be moved together",
  "窗口位于其他桌面，请切回后再试": "Window is on another desktop; switch to it and retry",
  "无法确认窗口所在桌面，尚未移动": "Desktop could not be confirmed; window was not moved",
  "无法确认窗口身份，尚未移动": "Window identity could not be confirmed; not moved",
  "无法读取窗口位置，尚未移动": "Window position could not be read; not moved",
  "疑似全屏窗口，请退出全屏后再试": "Window may be full screen; exit full screen and retry",
  "最近一次实际结果": "Latest operation result",
  "已移入": "Moved onto target",
  "已恢复原位": "Restored to the original position",
  "保留外部改动": "Kept external changes",
  "已安全可见，原屏恢复待处理": "Safely visible; awaiting the original display",
  "窗口未接受位置调整，可重试": "Window did not accept the move; retry is available",
  "窗口已关闭，无需重试": "Window is closed; no retry needed",
  "本次跳过": "Skipped this time",
  "无法确认结果，恢复记录保留": "Result could not be confirmed; recovery record retained",
  "这块显示器需要在当前桌面重新确认。请点击对应卡片；确认前不会整理窗口。": "Confirm this display again on the current desktop by selecting its card. Gathering is unavailable until confirmed.",
  "需要确认": "Confirmation needed",
  "连接状态未知，自动操作已停止": "Connection state unknown; automation stopped",
  "帮助与诊断": "Help and diagnostics",
  "仍有窗口待处理": "Windows still need attention",
  "部分窗口无法移动": "Some windows cannot be moved",
  "以管理员权限运行的程序，普通权限无法移动它的窗口。重启为管理员后可以一并整理；重启不会丢失恢复记录。":
    "A normal-integrity process cannot move the windows of a program running as administrator. Restarting elevated lets Unfy gather those too; recovery records survive the restart.",
  "以管理员身份重启": "Restart as administrator",
  "正在以管理员身份重启…": "Restarting as administrator…",
  "设置文件无法读取，原文件已保留。请查看诊断并修复后重启；自动操作已停止。": "The settings file could not be read and has been retained. Review diagnostics, repair it and restart; automation is stopped.",
  "还有 {count} 个窗口没能恢复原位。恢复记录已保留，请查看结果并重试恢复。": "{count} windows could not be restored. Their recovery records are retained; review the results and retry restoring.",
  "当前整理仍有恢复记录，请查看当前状态后继续操作。": "This gather still has recovery records. Review the current state before continuing.",
  "自动整理已暂停；手动整理与恢复仍可使用。": "Automation is paused; manual gather and restore remain available.",
  "自动下载、安装并重启 Unfy；远程连接或恢复工作未结束时会等待。": "Downloads, installs and restarts Unfy automatically; waits while a remote connection or recovery work is unresolved.",
  "旧版主次布局已改为保留大小移入；旧版自动化已关闭。现有恢复记录保留，不会重新排列当前窗口。可在设置中重新选择已验证的连接触发。": "The legacy main-and-side layout has changed to moving windows while preserving their sizes, and legacy automation is off. Existing recovery records remain saved; current windows will not be rearranged. You can enable verified connection triggers in Settings.",
  "期望开机启动：开启": "Requested startup preference: on",
  "期望开机启动：关闭": "Requested startup preference: off",
  "系统启动注册状态无法确认": "The system startup registration could not be confirmed",
  "系统实际启动注册：已开启": "Actual system startup registration: on",
  "系统实际启动注册：未开启": "Actual system startup registration: off",
  "重新应用启动设置": "Reapply startup preference",

};
