//! A user-facing sentence, assembled by the backend and worded by the frontend.
//!
//! # Why this exists
//!
//! The interface is bilingual, and the backend used to write whole Chinese
//! sentences into DTOs. Those appeared verbatim in the English interface: an
//! English user saw `先恢复了 2 个窗口`. Formatting the sentence here cannot work,
//! because the backend does not know the interface language and, more
//! importantly, does not own the words.
//!
//! So the backend states *which* sentence and supplies the values; the frontend
//! looks the key up in its catalogue and substitutes the parameters. The key is
//! the Chinese source string, the same convention the frontend catalogue uses,
//! so a missing translation falls back to Chinese rather than to a key name.
//!
//! `param` values are substituted verbatim: they are values (a display count, a
//! file path, an operating-system error), not sentences.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A sentence for the interface to render in the user's language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The Chinese source sentence, with `{name}` placeholders for parameters.
    pub key: String,
    /// Values for the placeholders. Empty when the sentence has none.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

impl Message {
    /// A sentence with no parameters.
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            params: BTreeMap::new(),
        }
    }

    /// A sentence with one value for a `{name}` placeholder.
    ///
    /// Every message in this app has at most one parameter, so the common case
    /// gets a named constructor rather than a map literal at each call site.
    pub fn one(key: impl Into<String>, name: &str, value: impl Into<String>) -> Self {
        let mut params = BTreeMap::new();
        params.insert(name.to_string(), value.into());
        Self {
            key: key.into(),
            params,
        }
    }
}

/// The message for a piece of remote-session evidence.
///
/// The window and the tray both show this sentence, from two different tables, so
/// the key is what travels.
pub fn evidence_message(evidence: &conflux_detect::Evidence) -> Message {
    match evidence.param() {
        Some(name) => Message::one(evidence.key(), "name", name),
        None => Message::new(evidence.key()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_message_carries_no_parameters() {
        let message = Message::new("就绪");
        assert_eq!(message.key, "就绪");
        assert!(message.params.is_empty());
    }

    #[test]
    fn a_parameterised_message_names_its_placeholder() {
        let message = Message::one("检测到 {count} 块显示器。", "count", "2");
        assert_eq!(message.params.get("count").map(String::as_str), Some("2"));
    }

    #[test]
    fn evidence_keeps_its_value_out_of_the_sentence() {
        let message = evidence_message(&conflux_detect::Evidence::ToolConnected("AnyDesk".into()));
        assert_eq!(message.key, "{name} 已连接");
        assert_eq!(
            message.params.get("name").map(String::as_str),
            Some("AnyDesk")
        );

        let plain = evidence_message(&conflux_detect::Evidence::RdpSession);
        assert_eq!(plain.key, "远程桌面会话");
        assert!(plain.params.is_empty());
    }

    /// The activity log is JSON on disk and is read back on the next launch.
    #[test]
    fn round_trips_through_json() {
        let message = Message::one("先恢复了 {count} 个窗口", "count", "2");
        let json = serde_json::to_string(&message).expect("serialise");
        let read: Message = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(read, message);
    }

    #[test]
    fn serialises_as_key_and_params() {
        // The frontend reads `{ key, params }`; a rename here silently empties
        // every notice in the interface, so the shape is pinned.
        let json = serde_json::to_string(&Message::one("先恢复了 {count} 个窗口", "count", "2"))
            .expect("serialise");
        assert_eq!(
            json,
            r#"{"key":"先恢复了 {count} 个窗口","params":{"count":"2"}}"#
        );
    }
}
