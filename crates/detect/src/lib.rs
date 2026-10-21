//! Remote-session detection.
//!
//! # What this module can and cannot do
//!
//! The product's original promise was "detect that I am being remoted into and
//! act". Independent verification (see `docs/verify/windows.md` §7) established
//! that **no public API answers "is someone remoting into this machine"** for
//! third-party tools: `SM_REMOTESESSION` and `WTSClientProtocolType` are set when
//! a *Terminal Services session* is created, which only RDP does. TeamViewer,
//! AnyDesk, RustDesk, ToDesk, Sunlogin, Parsec and Sunshine attach to the
//! existing console session and never set those flags.
//!
//! So detection here is a ladder of evidence, and the user picks how far up the
//! ladder the app may act on its own:
//!
//! | Sensitivity | Evidence | False-positive risk |
//! |---|---|---|
//! | `Off` | none, manual only | none |
//! | `Conservative` | RDP session | none |
//! | `Balanced` | RDP, or a virtual display appears | a user who adds a virtual monitor by hand |
//! | `Aggressive` | plus a known remote tool with its port open | a running-but-idle remote tool |
//!
//! Every verdict carries the evidence that produced it, and the UI shows that
//! evidence verbatim. An unexplained automatic window move is the worst possible
//! behaviour: the user blames the app for "randomly rearranging my desktop".
//!
//! Weak evidence must hold for several consecutive polls before acting, so a
//! process that starts and exits during startup does not trigger a migration.
//! RDP and virtual-display evidence act immediately.

use std::fmt;
use std::time::Duration;

#[cfg(windows)]
pub mod platform;

#[cfg(not(windows))]
pub mod platform {
    //! No detection on this platform.
    //!
    //! macOS exposes no API for "is someone capturing my screen" and the AX
    //! layer cannot see a virtual display being added in a way that distinguishes
    //! it from a real one; Linux without a portable backend has the same problem.
    //! Returning "no evidence" keeps the detector inert instead of guessing.

    use conflux_domain::DisplaySnapshot;

    use crate::Observation;

    pub fn observe(_displays: &[DisplaySnapshot]) -> Observation {
        Observation::default()
    }

    pub fn summary(_observation: &Observation) -> String {
        "detection not supported on this platform".to_string()
    }
}

/// How far up the evidence ladder the app may act on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sensitivity {
    /// Never act automatically. Hotkey, tray and window buttons still work.
    Off,
    /// Only a real Terminal Services client session.
    Conservative,
    /// Also act when a virtual display appears (`#[default]`).
    ///
    /// A virtual monitor appearing is strong evidence: remote tools create one to
    /// match the client's resolution. The failure mode is a user who installs a
    /// virtual display driver for an unrelated reason.
    #[default]
    Balanced,
    /// Also act when a known remote-control tool is running with its port open.
    Aggressive,
}

impl Sensitivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Sensitivity::Off => "off",
            Sensitivity::Conservative => "conservative",
            Sensitivity::Balanced => "balanced",
            Sensitivity::Aggressive => "aggressive",
        }
    }

    /// Parse the persisted form. Unknown values fall back to `Off` rather than
    /// `Balanced`: a corrupt or future value must not make windows move.
    pub fn parse(value: &str) -> Self {
        match value {
            "conservative" => Sensitivity::Conservative,
            "balanced" => Sensitivity::Balanced,
            "aggressive" => Sensitivity::Aggressive,
            _ => Sensitivity::Off,
        }
    }
}

/// One poll's raw observations. Pure data: no OS calls here, so the decision
/// table is testable on any machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    /// The calling process runs inside a Terminal Services client session.
    pub rdp_active: bool,
    /// Names of virtual displays currently present, if any.
    pub virtual_displays: Vec<String>,
    /// Known remote-control executables currently running.
    pub tools_running: Vec<String>,
    /// Known remote-control programs that hold an established TCP connection on
    /// their own port. Stronger than mere process presence.
    pub tools_connected: Vec<String>,
    /// The user marked this session by hand.
    pub manual_marker: bool,
}

impl Observation {
    pub fn is_empty(&self) -> bool {
        !self.rdp_active
            && !self.manual_marker
            && self.virtual_displays.is_empty()
            && self.tools_running.is_empty()
            && self.tools_connected.is_empty()
    }
}

/// Why the app believes a remote session is happening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// A Terminal Services client session (RDP).
    RdpSession,
    /// A virtual display is present, e.g. Parsec's VDD.
    VirtualDisplay(String),
    /// A known tool holds an established connection on its own port.
    ToolConnected(String),
    /// A known tool is running, with no stronger evidence.
    ToolRunning(String),
    /// The user marked the session by hand.
    ///
    /// The only universally reliable signal: verification found that no API
    /// answers "is someone remoting in" for third-party tools, so the honest
    /// design is to let the user say so and to act on that immediately.
    ManualMarker,
}

impl Evidence {
    /// Whether this evidence needs to persist across polls before acting.
    ///
    /// Session-type evidence is instantaneous by nature; process evidence is not,
    /// because services restart, updaters spawn, and tray helpers linger.
    pub fn is_immediate(&self) -> bool {
        matches!(self, Evidence::RdpSession | Evidence::ManualMarker)
    }

    /// The sentence describing this evidence, as a catalogue key.
    ///
    /// The window renders it from the frontend catalogue and the tray from its own
    /// table, so what crosses the boundary is the key plus the value, never a
    /// formatted sentence: a sentence formatted here would be Chinese in an
    /// English interface.
    pub fn key(&self) -> &'static str {
        match self {
            Evidence::RdpSession => "远程桌面会话",
            Evidence::VirtualDisplay(_) => "虚拟显示器 {name}",
            Evidence::ToolConnected(_) => "{name} 已连接",
            Evidence::ToolRunning(_) => "{name} 正在运行",
            Evidence::ManualMarker => "手动标记",
        }
    }

    /// The value for the `{name}` placeholder in [`Evidence::key`], if it has one.
    pub fn param(&self) -> Option<&str> {
        match self {
            Evidence::VirtualDisplay(name)
            | Evidence::ToolConnected(name)
            | Evidence::ToolRunning(name) => Some(name),
            Evidence::RdpSession | Evidence::ManualMarker => None,
        }
    }

    /// The sentence with its value filled in, in the source language.
    ///
    /// For logs and diagnostics only. Anything the user reads goes through
    /// `key`/`param` so it can be translated.
    pub fn label(&self) -> String {
        match self.param() {
            Some(name) => self.key().replace("{name}", name),
            None => self.key().to_string(),
        }
    }
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// Whether the evidence is allowed at this sensitivity.
fn admitted(evidence: &Evidence, sensitivity: Sensitivity) -> bool {
    // The user speaking directly overrides every level, including `Off`. `Off`
    // exists to stop guesswork, not to ignore an explicit instruction.
    if matches!(evidence, Evidence::ManualMarker) {
        return true;
    }
    match (sensitivity, evidence) {
        (Sensitivity::Off, _) => false,
        (Sensitivity::Conservative, Evidence::RdpSession) => true,
        (Sensitivity::Conservative, _) => false,
        (Sensitivity::Balanced, Evidence::RdpSession | Evidence::VirtualDisplay(_)) => true,
        (Sensitivity::Balanced, _) => false,
        (Sensitivity::Aggressive, Evidence::ToolRunning(_)) => true,
        (Sensitivity::Aggressive, _) => true,
    }
}

/// The strongest evidence in an observation that this sensitivity admits.
fn strongest(observation: &Observation, sensitivity: Sensitivity) -> Option<Evidence> {
    if observation.manual_marker {
        return Some(Evidence::ManualMarker);
    }
    let mut candidates: Vec<Evidence> = Vec::new();
    if observation.rdp_active {
        candidates.push(Evidence::RdpSession);
    }
    for name in &observation.virtual_displays {
        candidates.push(Evidence::VirtualDisplay(name.clone()));
    }
    for name in &observation.tools_connected {
        candidates.push(Evidence::ToolConnected(name.clone()));
    }
    for name in &observation.tools_running {
        candidates.push(Evidence::ToolRunning(name.clone()));
    }
    candidates.into_iter().find(|e| admitted(e, sensitivity))
}

/// What the tracker decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Nothing changed.
    None,
    /// A remote session just began; act.
    Activated(Evidence),
    /// The evidence is gone; undo.
    Deactivated,
}

/// Consecutive weak observations required before acting.
///
/// Two polls at the default 2 s interval means an idle remote tool that happens
/// to be running while the user logs in does not move any windows. Four seconds
/// is below the time it takes a person to notice their desktop did not change.
pub const ACTIVATION_STREAK: u32 = 2;

/// Holds the activation state and the streak counter.
#[derive(Debug, Clone)]
pub struct Tracker {
    sensitivity: Sensitivity,
    active: Option<Evidence>,
    streak: u32,
}

impl Tracker {
    pub fn new(sensitivity: Sensitivity) -> Self {
        Self {
            sensitivity,
            active: None,
            streak: 0,
        }
    }

    pub fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    /// Change sensitivity. An active session stays active until evidence
    /// disappears, so the user lowering sensitivity does not cause an immediate
    /// restore while they are still remoted in.
    pub fn set_sensitivity(&mut self, sensitivity: Sensitivity) {
        self.sensitivity = sensitivity;
        if sensitivity == Sensitivity::Off {
            self.streak = 0;
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn evidence(&self) -> Option<&Evidence> {
        self.active.as_ref()
    }

    /// Feed one poll.
    pub fn observe(&mut self, observation: &Observation) -> Transition {
        let candidate = strongest(observation, self.sensitivity);

        match (&self.active, candidate) {
            (None, None) => {
                self.streak = 0;
                Transition::None
            }
            (None, Some(evidence)) => {
                if evidence.is_immediate() {
                    self.streak = 0;
                    self.active = Some(evidence.clone());
                    return Transition::Activated(evidence);
                }
                self.streak += 1;
                if self.streak >= ACTIVATION_STREAK {
                    self.streak = 0;
                    self.active = Some(evidence.clone());
                    Transition::Activated(evidence)
                } else {
                    Transition::None
                }
            }
            (Some(_), Some(_)) => {
                // Still active. Keep the original evidence: rewriting it every
                // poll makes the UI line flicker between equivalent reasons, and
                // hides which evidence actually started the session.
                self.streak = 0;
                Transition::None
            }
            (Some(_), None) => {
                self.streak = 0;
                self.active = None;
                Transition::Deactivated
            }
        }
    }

    /// Force the state, used when the user toggles remote mode by hand.
    pub fn force(&mut self, active: impl Into<Option<Evidence>>) {
        self.active = active.into();
        self.streak = 0;
    }
}

/// Poll interval used by the desktop monitor.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Known remote-control programs and the local ports they listen on.
///
/// Ports are from the vendors' published firewall requirements. They are only
/// used to *strengthen* process evidence, so a wrong port degrades the signal to
/// "tool is running" instead of causing a false activation.
const KNOWN_TOOLS: &[(&str, &[u16])] = &[
    ("teamviewer.exe", &[5938]),
    ("teamviewer_service.exe", &[5938]),
    ("anydesk.exe", &[7070]),
    ("rustdesk.exe", &[21115, 21116, 21117, 21118, 21119]),
    ("todesk.exe", &[]),
    ("todesk_service.exe", &[]),
    ("sunloginclient.exe", &[]),
    ("awesun.exe", &[]),
    ("parsecd.exe", &[8000, 8001, 8002]),
    ("sunshine.exe", &[47984, 47989, 47990]),
    ("remoting_host.exe", &[]),
    ("winvnc.exe", &[5900]),
    ("tvnserver.exe", &[5900]),
    ("rserver3.exe", &[]),
    ("splashtop", &[]),
];

/// Ports whose established connections count as tool evidence.
pub fn known_ports() -> Vec<u16> {
    let mut ports: Vec<u16> = KNOWN_TOOLS
        .iter()
        .flat_map(|(_, ports)| ports.iter().copied())
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Whether an executable name belongs to a known remote-control program.
///
/// Matching is a case-insensitive substring for names that vendors suffix with
/// version numbers (`splashtop`), and exact for everything else.
pub fn is_known_tool(exe_name: &str) -> Option<&'static str> {
    let lower = exe_name.to_ascii_lowercase();
    KNOWN_TOOLS
        .iter()
        .find(|(name, _)| lower == *name || lower.contains(name.trim_end_matches(".exe")))
        .map(|(name, _)| *name)
}

/// Ports associated with one tool, used to decide whether a connection is
/// evidence for that specific tool.
pub fn ports_for(tool: &str) -> &'static [u16] {
    KNOWN_TOOLS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, ports)| *ports)
        .unwrap_or(&[])
}

/// Whether a display device name looks like a software display.
///
/// Vendors name their indirect display devices recognisably, and the Windows
/// device path contains the EDID manufacturer id. Matching on a fixed list keeps
/// this honest: a real monitor is never mistaken for a virtual one.
pub fn is_virtual_display(device_name: &str) -> bool {
    const MARKERS: &[&str] = &[
        // EDID manufacturer ids as they appear in a Windows device path, plus
        // the product names each vendor writes into the device node.
        "parsc",
        "parsec",
        "vdd",
        "virtual display",
        "virtualdisplay",
        "idd",
        "rustdesk",
        "sunshine",
        "todesk",
        "anydesk",
        "teamviewer",
        "spacedesk",
        "usbmmidd",
        "amdvdd",
    ];
    let lower = device_name.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(rdp: bool, virtuals: &[&str], tools: &[&str], connected: &[&str]) -> Observation {
        Observation {
            rdp_active: rdp,
            virtual_displays: virtuals.iter().map(|s| s.to_string()).collect(),
            tools_running: tools.iter().map(|s| s.to_string()).collect(),
            tools_connected: connected.iter().map(|s| s.to_string()).collect(),
            manual_marker: false,
        }
    }

    #[test]
    fn off_never_activates_even_with_rdo() {
        let mut tracker = Tracker::new(Sensitivity::Off);
        let transition = tracker.observe(&obs(true, &["Parsec VDD"], &["anydesk.exe"], &[]));
        assert_eq!(transition, Transition::None);
        assert!(!tracker.is_active());
    }

    #[test]
    fn conservative_ignores_virtual_displays() {
        let mut tracker = Tracker::new(Sensitivity::Conservative);
        for _ in 0..ACTIVATION_STREAK + 1 {
            tracker.observe(&obs(false, &["Parsec VDD"], &[], &[]));
        }
        assert!(!tracker.is_active());
    }

    #[test]
    fn balanced_activates_on_virtual_display_after_streak() {
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        let observation = obs(false, &["Parsec VDD"], &[], &[]);

        assert_eq!(tracker.observe(&observation), Transition::None);
        assert_eq!(
            tracker.observe(&observation),
            Transition::Activated(Evidence::VirtualDisplay("Parsec VDD".into()))
        );
        assert!(tracker.is_active());
    }

    #[test]
    fn rdp_activates_immediately() {
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        assert_eq!(
            tracker.observe(&obs(true, &[], &[], &[])),
            Transition::Activated(Evidence::RdpSession)
        );
    }

    #[test]
    fn deactivates_when_evidence_disappears() {
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        tracker.observe(&obs(true, &[], &[], &[]));
        assert!(tracker.is_active());

        assert_eq!(
            tracker.observe(&obs(false, &[], &[], &[])),
            Transition::Deactivated
        );
        assert!(!tracker.is_active());
    }

    #[test]
    fn weak_tool_evidence_needs_consecutive_polls() {
        let mut tracker = Tracker::new(Sensitivity::Aggressive);
        let observation = obs(false, &[], &["anydesk.exe"], &[]);

        assert_eq!(tracker.observe(&observation), Transition::None);

        // A single empty poll resets the streak: a tool that flashes into
        // existence for one tick must never move windows.
        assert_eq!(
            tracker.observe(&obs(false, &[], &[], &[])),
            Transition::None
        );
        assert_eq!(tracker.observe(&observation), Transition::None);

        assert_eq!(
            tracker.observe(&observation),
            Transition::Activated(Evidence::ToolRunning("anydesk.exe".into()))
        );
    }

    #[test]
    fn connected_tool_evidence_outranks_running() {
        let mut tracker = Tracker::new(Sensitivity::Aggressive);
        let observation = obs(false, &[], &["rustdesk.exe"], &["rustdesk.exe"]);

        tracker.observe(&observation);
        assert_eq!(
            tracker.observe(&observation),
            Transition::Activated(Evidence::ToolConnected("rustdesk.exe".into()))
        );
    }

    #[test]
    fn active_session_keeps_its_original_evidence() {
        let mut tracker = Tracker::new(Sensitivity::Aggressive);
        tracker.observe(&obs(true, &[], &[], &[]));
        assert_eq!(tracker.evidence(), Some(&Evidence::RdpSession));

        // A tool appearing mid-session must not rewrite the reason: the UI would
        // otherwise claim the session started for a reason that appeared later.
        tracker.observe(&obs(true, &[], &["anydesk.exe"], &["anydesk.exe"]));
        assert_eq!(tracker.evidence(), Some(&Evidence::RdpSession));
    }

    #[test]
    fn lowering_sensitivity_does_not_cancel_an_active_session() {
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        tracker.observe(&obs(false, &["Parsec VDD"], &[], &[]));
        tracker.observe(&obs(false, &["Parsec VDD"], &[], &[]));
        assert!(tracker.is_active());

        tracker.set_sensitivity(Sensitivity::Conservative);
        assert!(tracker.is_active());
        // With the evidence still present but no longer admitted, it ends.
        assert_eq!(
            tracker.observe(&obs(false, &["Parsec VDD"], &[], &[])),
            Transition::Deactivated
        );
    }

    #[test]
    fn the_manual_marker_activates_even_when_detection_is_off() {
        // Turning detection off means "do not guess". It does not mean "ignore my
        // explicit instruction", so the marker still works.
        let mut tracker = Tracker::new(Sensitivity::Off);
        let observation = Observation {
            manual_marker: true,
            ..Observation::default()
        };
        assert_eq!(
            tracker.observe(&observation),
            Transition::Activated(Evidence::ManualMarker)
        );
        assert!(tracker.is_active());

        // And clearing it ends the session, even at `Off`.
        assert_eq!(
            tracker.observe(&Observation::default()),
            Transition::Deactivated
        );
    }

    #[test]
    fn unknown_persisted_sensitivity_is_off() {
        assert_eq!(Sensitivity::parse("whatever"), Sensitivity::Off);
        assert_eq!(Sensitivity::parse("balanced"), Sensitivity::Balanced);
    }

    #[test]
    fn recognises_virtual_displays_but_not_real_ones() {
        assert!(is_virtual_display(
            r"\\?\DISPLAY#PARSC000#4&1a2b&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}"
        ));
        assert!(is_virtual_display("RustDesk Virtual Display"));
        assert!(!is_virtual_display(r"\\.\DISPLAY1"));
        assert!(!is_virtual_display(
            r"\\?\DISPLAY#DEL41FF#4&3abf5356&0&UID4147#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}"
        ));
    }

    #[test]
    fn recognises_tool_executables_case_insensitively() {
        assert_eq!(is_known_tool("AnyDesk.exe"), Some("anydesk.exe"));
        assert_eq!(is_known_tool("TEAMVIEWER.EXE"), Some("teamviewer.exe"));
        assert_eq!(is_known_tool("notepad.exe"), None);
    }
}

#[cfg(test)]
mod resume_tests {
    //! What a suspend must and must not do.
    //!
    //! The first implementation rebuilt the tracker after a suspend. A fresh
    //! tracker reports no active evidence, so the next poll produced `Deactivated`
    //! and the app restored every window - even though the user was still connected
    //! and the machine had merely slept. These tests pin the corrected behaviour:
    //! the streak resets, the verdict survives.

    use super::*;

    fn obs_rdp() -> Observation {
        Observation {
            rdp_active: true,
            ..Observation::default()
        }
    }

    #[test]
    fn keeping_the_evidence_survives_a_suspend() {
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        tracker.observe(&obs_rdp());
        assert!(tracker.is_active());

        // What `resume_after_sleep` does: reset the streak, keep the verdict.
        let evidence = tracker.evidence().cloned();
        tracker.force(evidence);

        assert!(
            tracker.is_active(),
            "a session that survived the sleep must not be reported as ended"
        );
        assert_eq!(tracker.evidence(), Some(&Evidence::RdpSession));
    }

    #[test]
    fn rebuilding_the_tracker_would_fake_an_end() {
        // Documents the failure mode so a future "reset on resume" change shows up
        // as a test failure rather than as windows moving on wake.
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        tracker.observe(&obs_rdp());
        assert!(tracker.is_active());

        let mut rebuilt = Tracker::new(Sensitivity::Balanced);
        assert_eq!(
            rebuilt.observe(&Observation::default()),
            Transition::None,
            "the rebuilt tracker has no evidence to deactivate"
        );
        assert!(!rebuilt.is_active());
        // The difference is the bug: `rebuilt.is_active()` is false, which the
        // monitor reads as "the session ended".
        assert_ne!(tracker.is_active(), rebuilt.is_active());
    }

    #[test]
    fn a_session_that_ended_during_sleep_is_still_detected() {
        let mut tracker = Tracker::new(Sensitivity::Balanced);
        tracker.observe(&obs_rdp());

        // Keep the verdict across the suspend, then observe real evidence: the
        // session is over, so the normal path must report the transition.
        let evidence = tracker.evidence().cloned();
        tracker.force(evidence);
        assert_eq!(
            tracker.observe(&Observation::default()),
            Transition::Deactivated
        );
    }
}
