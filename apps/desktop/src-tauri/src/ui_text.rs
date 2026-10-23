//! The sentences the tray renders, in each interface language.
//!
//! # Why the tray needs its own table
//!
//! Everything else in the interface is rendered by the webview, which owns the
//! catalogue in `src/i18n.ts`. The tray menu, its tooltip and its state line are
//! built in Rust, where that catalogue is not available. This table carries only
//! those strings.
//!
//! The sentences that also appear in the window - the remote-session evidence, for
//! instance - exist in both tables. That duplication is deliberate: neither side
//! can read the other's module at runtime, and the alternative, formatting the
//! sentence in Rust and sending the text, is what produced Chinese text in an
//! English window.

use unfy_settings::LanguageName;

/// A language the interface can be rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Zh,
    En,
}

impl Locale {
    /// Parse an interface language, treating anything unknown as the source
    /// language.
    ///
    /// `system` cannot be resolved here: the webview resolves it from
    /// `navigator.language` and reports the result through `set_ui_locale`, so that
    /// one rule decides the language for the whole app rather than two that can
    /// disagree.
    pub fn from_setting(value: &LanguageName) -> Option<Locale> {
        match value {
            LanguageName::Zh => Some(Locale::Zh),
            LanguageName::En => Some(Locale::En),
            LanguageName::System => None,
        }
    }

    pub fn from_tag(tag: &str) -> Option<Locale> {
        let tag = tag.trim().to_ascii_lowercase();
        if tag.starts_with("en") {
            Some(Locale::En)
        } else if tag.starts_with("zh") {
            Some(Locale::Zh)
        } else {
            None
        }
    }
}

/// Every sentence the tray can show.
///
/// A struct rather than a lookup by key: the compiler then requires every language
/// to supply each field, so a string added to one language cannot be forgotten in
/// the other.
pub struct TrayText {
    /// Tooltip prefix while nothing is in progress.
    pub state_idle: &'static str,
    pub state_planning: &'static str,
    pub state_migrating: &'static str,
    pub state_active: &'static str,
    pub state_restoring: &'static str,
    pub state_degraded: &'static str,
    pub state_error: &'static str,
    /// Appended to the state while automation is paused.
    pub paused_suffix: &'static str,
    /// Prefix for the remote-session line; the evidence sentence follows it.
    pub remote_prefix: &'static str,
    pub gather: &'static str,
    pub re_gather: &'static str,
    pub restore: &'static str,
    pub rescue: &'static str,
    pub pause: &'static str,
    pub identify: &'static str,
    pub cycle: &'static str,
    pub remote_end: &'static str,
    pub remote_mark: &'static str,
    pub settings: &'static str,
    pub quit: &'static str,
    /// Shown when a display-dependent action has no display to work with.
    pub no_displays: &'static str,
    /// Remote-session evidence, mirroring `Evidence::key` in `unfy-detect`.
    pub evidence_rdp: &'static str,
    pub evidence_virtual_display: &'static str,
    pub evidence_tool_connected: &'static str,
    pub evidence_tool_running: &'static str,
    pub evidence_manual: &'static str,
}

pub const ZH: TrayText = TrayText {
    state_idle: "Unfy · 就绪",
    state_planning: "Unfy · 正在计划",
    state_migrating: "Unfy · 正在整理窗口",
    state_active: "Unfy · 布局已启用",
    state_restoring: "Unfy · 正在恢复",
    state_degraded: "Unfy · 部分受限",
    state_error: "Unfy · 需要处理",
    paused_suffix: "（已暂停）",
    remote_prefix: "远程会话：",
    gather: "整理到目标屏",
    re_gather: "重新整理到目标屏",
    restore: "恢复本次布局",
    rescue: "救援越界窗口",
    pause: "暂停自动整理",
    identify: "识别屏幕编号",
    cycle: "切换到下一块屏",
    remote_end: "结束远程会话并恢复",
    remote_mark: "标记为远程会话",
    settings: "设置…",
    quit: "退出",
    no_displays: "没有可用显示器",
    evidence_rdp: "远程桌面会话",
    evidence_virtual_display: "虚拟显示器 {name}",
    evidence_tool_connected: "{name} 已连接",
    evidence_tool_running: "{name} 正在运行",
    evidence_manual: "手动标记",
};

pub const EN: TrayText = TrayText {
    state_idle: "Unfy · Ready",
    state_planning: "Unfy · Planning",
    state_migrating: "Unfy · Gathering windows",
    state_active: "Unfy · Layout active",
    state_restoring: "Unfy · Restoring",
    state_degraded: "Unfy · Partly limited",
    state_error: "Unfy · Needs attention",
    paused_suffix: " (paused)",
    remote_prefix: "Remote session: ",
    gather: "Gather to target",
    re_gather: "Gather to target again",
    restore: "Restore this layout",
    rescue: "Rescue off-screen windows",
    pause: "Pause automatic gathering",
    identify: "Show screen numbers",
    cycle: "Switch to the next display",
    remote_end: "End remote session and restore",
    remote_mark: "Mark as a remote session",
    settings: "Settings…",
    quit: "Quit",
    no_displays: "No display available",
    evidence_rdp: "Remote Desktop session",
    evidence_virtual_display: "Virtual display {name}",
    evidence_tool_connected: "{name} connected",
    evidence_tool_running: "{name} running",
    evidence_manual: "Marked by hand",
};

/// The tray sentences for a locale.
pub fn tray(locale: Locale) -> &'static TrayText {
    match locale {
        Locale::Zh => &ZH,
        Locale::En => &EN,
    }
}

/// Render an evidence sentence in a locale.
///
/// Mirrors `Evidence::key`/`Evidence::param`; the keys are asserted against this
/// table in the tests below, because a new kind of evidence would otherwise show
/// the source language in the tray.
pub fn evidence(text: &TrayText, key: &str, param: Option<&str>) -> String {
    let template = match key {
        "远程桌面会话" => text.evidence_rdp,
        "虚拟显示器 {name}" => text.evidence_virtual_display,
        "{name} 已连接" => text.evidence_tool_connected,
        "{name} 正在运行" => text.evidence_tool_running,
        "手动标记" => text.evidence_manual,
        // Unknown key: the tray shows the key rather than nothing, so a new kind of
        // evidence is visible before anyone translates it.
        other => other,
    };
    match param {
        Some(value) => template.replace("{name}", value),
        None => template.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every evidence key from `unfy-detect` must reach this table, in both
    /// languages. Without this, a new signal would appear as its Chinese key in an
    /// English tray and nobody would notice until a screenshot.
    #[test]
    fn every_evidence_key_is_translated() {
        let keys = [
            "远程桌面会话",
            "虚拟显示器 {name}",
            "{name} 已连接",
            "{name} 正在运行",
            "手动标记",
        ];
        for key in keys {
            // In the source language the sentence *is* the key, so only the English
            // table can show a missing entry. `evidence` returns the key unchanged
            // when it does not recognise one.
            let english = evidence(tray(Locale::En), key, Some("X"));
            assert_ne!(english, key, "English has no sentence for {key}");
            let chinese = evidence(tray(Locale::Zh), key, Some("X"));
            assert!(!chinese.is_empty(), "Chinese has no sentence for {key}");
        }
    }

    #[test]
    fn evidence_substitutes_its_value() {
        let rendered = evidence(tray(Locale::En), "{name} 已连接", Some("AnyDesk"));
        assert_eq!(rendered, "AnyDesk connected");
        // The template keeps its placeholder when a value arrives for a key that
        // has none, which is a bug in the caller rather than a crash here.
        let plain = evidence(tray(Locale::En), "远程桌面会话", None);
        assert_eq!(plain, "Remote Desktop session");
    }

    #[test]
    fn an_unknown_evidence_key_shows_the_key() {
        assert_eq!(evidence(tray(Locale::En), "新的信号", None), "新的信号");
    }

    #[test]
    fn language_tags_map_to_a_locale() {
        assert_eq!(Locale::from_tag("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_tag("zh-CN"), Some(Locale::Zh));
        assert_eq!(Locale::from_tag("ZH"), Some(Locale::Zh));
        // Anything else stays unresolved so the caller can fall back to the saved
        // setting rather than guessing.
        assert_eq!(Locale::from_tag("fr"), None);
    }

    #[test]
    fn a_saved_language_resolves_except_for_system() {
        assert_eq!(Locale::from_setting(&LanguageName::Zh), Some(Locale::Zh));
        assert_eq!(Locale::from_setting(&LanguageName::En), Some(Locale::En));
        assert_eq!(Locale::from_setting(&LanguageName::System), None);
    }

    /// The tray shows the state as a sentence; a missing one would be a blank
    /// tooltip, which is worse than a wrong word.
    #[test]
    fn no_tray_sentence_is_empty() {
        for locale in [Locale::Zh, Locale::En] {
            let text = tray(locale);
            for value in [
                text.state_idle,
                text.state_planning,
                text.state_migrating,
                text.state_active,
                text.state_restoring,
                text.state_degraded,
                text.state_error,
                text.paused_suffix,
                text.remote_prefix,
                text.gather,
                text.re_gather,
                text.restore,
                text.rescue,
                text.pause,
                text.identify,
                text.cycle,
                text.remote_end,
                text.remote_mark,
                text.settings,
                text.quit,
            ] {
                assert!(!value.is_empty(), "{locale:?} has an empty tray string");
            }
        }
    }
}
