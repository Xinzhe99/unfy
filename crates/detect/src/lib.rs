//! Remote-session detection.
//!
//! Process, TCP, display and RDP protocol observations are diagnostic candidates.
//! Only an explicit WTS connection state for this process's session identifies
//! a connected/disconnected RDP lifecycle. RDP is a session, not a mirror: its
//! automation policy must rescue offscreen windows without gathering visible ones.

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

    use unfy_domain::DisplaySnapshot;

    use crate::Observation;

    pub fn observe(_displays: &[DisplaySnapshot]) -> Observation {
        Observation::default()
    }

    pub fn summary(_observation: &Observation) -> String {
        "detection not supported on this platform".to_string()
    }
}

/// How much evidence the app may act on by itself.
///
/// The previous four-level ladder admitted "a known remote tool is running" and
/// "a virtual display exists" as permission to move every window. Neither proves
/// that a client is connected to *this* machine: a tool can be idle in the tray,
/// and a virtual display outlives the session that created it. Automatic actions
/// are therefore limited to an evidence source that reports an inbound session
/// lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sensitivity {
    /// Never act automatically. Hotkey, tray and window buttons still work.
    #[default]
    Off,
    /// Act only on an inbound connection lifecycle reported by a verified source.
    Verified,
}

impl Sensitivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Sensitivity::Off => "off",
            Sensitivity::Verified => "verified",
        }
    }

    /// Parse the persisted form. Unknown values fall back to `Off`: a corrupt or
    /// future value must not make windows move.
    pub fn parse(value: &str) -> Self {
        match value {
            "verified" => Sensitivity::Verified,
            _ => Sensitivity::Off,
        }
    }
}

/// An explicit WTS connection state for the session hosting this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RdpSession {
    pub id: u32,
    pub connected: bool,
}

/// One poll's raw observations. Pure data: no OS calls here, so the decision
/// table is testable on any machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    /// The calling process runs inside a Terminal Services client session.
    pub rdp_active: bool,
    /// Explicit current-session lifecycle, absent for console/protocol-only probes.
    pub rdp_session: Option<RdpSession>,
    /// Names of virtual displays currently present, if any.
    pub virtual_displays: Vec<String>,
    /// Known remote-control executables currently running.
    pub tools_running: Vec<String>,
    /// Known remote-control programs that hold an established TCP connection on
    /// their own port.
    pub tools_connected: Vec<String>,
    /// Whether the probe could answer at all this poll.
    ///
    /// A failed query is not "no evidence". Treating the two the same made a
    /// transient permission or IO error look like the end of a session, which
    /// restored every window while the user was still working remotely.
    pub available: bool,
}

impl Observation {
    /// An observation from a probe that ran successfully.
    pub fn observed() -> Self {
        Self {
            available: true,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.rdp_active
            && self.rdp_session.is_none()
            && self.virtual_displays.is_empty()
            && self.tools_running.is_empty()
            && self.tools_connected.is_empty()
    }
}

/// Why the app believes a remote session is happening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// This process belongs to an RDP session. Protocol membership does not prove
    /// that a client is currently connected or that this is a mirror session.
    RdpSession,
    /// A known tool holds an established connection on its own port.
    ///
    /// Still not proof of *inbound control* - it can be an outbound session where
    /// this machine is the viewer - so it is reported as a candidate only.
    ToolConnected(String),
    /// A known tool is running, with no stronger evidence.
    ToolRunning(String),
    /// A virtual display is present, e.g. Parsec's VDD.
    VirtualDisplay(String),
}

impl Evidence {
    /// A descriptive label alone never authorises movement; admission requires
    /// the separate current-session lifecycle and its identity.
    pub fn authorises_automation(&self) -> bool {
        false
    }

    /// The sentence describing this evidence, as a catalogue key.
    pub fn key(&self) -> &'static str {
        match self {
            Evidence::RdpSession => "远程桌面会话",
            Evidence::VirtualDisplay(_) => "虚拟显示器 {name}",
            Evidence::ToolConnected(_) => "{name} 已连接",
            Evidence::ToolRunning(_) => "{name} 正在运行",
        }
    }

    /// The value for the `{name}` placeholder in [`Evidence::key`], if it has one.
    pub fn param(&self) -> Option<&str> {
        match self {
            Evidence::VirtualDisplay(name)
            | Evidence::ToolConnected(name)
            | Evidence::ToolRunning(name) => Some(name),
            Evidence::RdpSession => None,
        }
    }

    /// The sentence with its value filled in, in the source language.
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

/// What the detector concluded from one observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The probe could not answer; keep the previous state and say nothing.
    Unknown,
    /// No evidence that anyone is connected.
    Idle,
    /// Something is present that may indicate a session, but it does not prove
    /// inbound control. Shown to the user; never acted on automatically.
    Candidate(Evidence),
    /// A verified inbound session is alive.
    Connected(Evidence),
}

/// The strongest evidence present, regardless of whether it may authorise action.
fn weak_evidence(observation: &Observation) -> Option<Evidence> {
    if observation.rdp_active {
        return Some(Evidence::RdpSession);
    }
    if let Some(name) = observation.tools_connected.first() {
        return Some(Evidence::ToolConnected(name.clone()));
    }
    if let Some(name) = observation.virtual_displays.first() {
        return Some(Evidence::VirtualDisplay(name.clone()));
    }
    observation
        .tools_running
        .first()
        .map(|name| Evidence::ToolRunning(name.clone()))
}

/// What the tracker decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Nothing changed.
    None,
    /// A verified session just began; act.
    Activated(Evidence),
    /// This verified session explicitly disconnected; retain unrelated work.
    Deactivated,
}

/// Tracks the last explicit current-session state without inventing end events.
#[derive(Debug, Clone)]
pub struct Tracker {
    sensitivity: Sensitivity,
    active: Option<Evidence>,
    session: Option<RdpSession>,
    observation_known: bool,
}

impl Tracker {
    pub fn new(sensitivity: Sensitivity) -> Self {
        Self {
            sensitivity,
            active: None,
            session: None,
            observation_known: false,
        }
    }

    pub fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    /// Changing settings never constitutes a reliable connection end.
    pub fn set_sensitivity(&mut self, sensitivity: Sensitivity) {
        self.sensitivity = sensitivity;
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn evidence(&self) -> Option<&Evidence> {
        self.active.as_ref()
    }

    /// A currently observed connected session; unavailable samples revoke admission.
    pub fn connected_session(&self) -> Option<u32> {
        self.session
            .filter(|session| self.observation_known && session.connected)
            .map(|session| session.id)
    }

    pub fn is_unknown(&self) -> bool {
        !self.observation_known
    }

    /// Feed one poll.
    pub fn observe(&mut self, observation: &Observation) -> Transition {
        self.observation_known = observation.available;
        if !observation.available {
            return Transition::None;
        }
        let Some(session) = observation.rdp_session else {
            self.observation_known = self.session.is_none();
            return Transition::None;
        };
        let previous = self.session;
        if !session.connected && previous.is_some_and(|old| old.id != session.id) {
            self.observation_known = false;
            return Transition::None;
        }
        self.session = Some(session);
        if session.connected {
            self.active = Some(Evidence::RdpSession);
            if previous != Some(session) {
                return Transition::Activated(Evidence::RdpSession);
            }
        } else if previous.is_some_and(|old| old.id == session.id && old.connected) {
            self.active = None;
            return Transition::Deactivated;
        }
        Transition::None
    }

    /// What the current observation means, for display.
    ///
    /// Diagnostic candidates describe the latest available observation.
    pub fn verdict(&self, observation: &Observation) -> Verdict {
        if !observation.available || !self.observation_known {
            return Verdict::Unknown;
        }
        if let Some(evidence) = &self.active {
            return Verdict::Connected(evidence.clone());
        }
        match weak_evidence(observation) {
            Some(evidence) => Verdict::Candidate(evidence),
            None => Verdict::Idle,
        }
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
    // Explicit executable names: unrelated prefix/suffix matches are not evidence.
    ("gameviewer.exe", &[]),
    ("gameviewerserver.exe", &[]),
    ("parsecd.exe", &[8000, 8001, 8002]),
    ("sunshine.exe", &[47984, 47989, 47990]),
    ("remoting_host.exe", &[]),
    ("winvnc.exe", &[5900]),
    ("tvnserver.exe", &[5900]),
    ("rserver3.exe", &[]),
    ("splashtop.exe", &[]),
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
/// Match the complete executable name, ignoring ASCII case.
pub fn is_known_tool(exe_name: &str) -> Option<&'static str> {
    KNOWN_TOOLS
        .iter()
        .find(|(name, _)| exe_name.eq_ignore_ascii_case(name))
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
            available: true,
            rdp_active: rdp,
            rdp_session: None,
            virtual_displays: virtuals.iter().map(|s| s.to_string()).collect(),
            tools_running: tools.iter().map(|s| s.to_string()).collect(),
            tools_connected: connected.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn unknown() -> Observation {
        Observation::default()
    }

    #[test]
    fn tools_require_complete_executable_names() {
        assert_eq!(
            is_known_tool("GameViewerServer.exe"),
            Some("gameviewerserver.exe")
        );
        assert_eq!(is_known_tool("GameViewer.exe"), Some("gameviewer.exe"));
        assert_eq!(is_known_tool("ANYDESK.EXE"), Some("anydesk.exe"));
        for unrelated in [
            "UUBooster.exe",
            "notanydesk.exe",
            "anydesk.exe.bak",
            "teamviewer-notes.exe",
        ] {
            assert_eq!(is_known_tool(unrelated), None);
        }
    }

    #[test]
    fn candidate_signals_never_activate_off_mode() {
        let mut tracker = Tracker::new(Sensitivity::Off);
        let transition = tracker.observe(&obs(
            true,
            &["Parsec VDD"],
            &["anydesk.exe"],
            &["anydesk.exe"],
        ));
        assert_eq!(transition, Transition::None);
        assert!(!tracker.is_active());
    }

    #[test]
    fn rdp_protocol_is_a_candidate_not_a_connection_lifecycle() {
        let mut tracker = Tracker::new(Sensitivity::Verified);
        let observation = obs(true, &[], &[], &[]);
        for _ in 0..300 {
            assert_eq!(tracker.observe(&observation), Transition::None);
        }
        assert!(!tracker.is_active());
        assert_eq!(
            tracker.verdict(&observation),
            Verdict::Candidate(Evidence::RdpSession)
        );
    }

    fn session(id: u32, connected: bool) -> Observation {
        Observation {
            available: true,
            rdp_active: true,
            rdp_session: Some(RdpSession { id, connected }),
            ..Observation::default()
        }
    }

    #[test]
    fn explicit_session_lifecycle_deduplicates_and_reconnects() {
        let mut tracker = Tracker::new(Sensitivity::Verified);
        assert_eq!(
            tracker.observe(&session(7, true)),
            Transition::Activated(Evidence::RdpSession)
        );
        assert_eq!(tracker.connected_session(), Some(7));
        assert!(!tracker.is_unknown());
        assert_eq!(tracker.observe(&session(7, true)), Transition::None);
        assert_eq!(tracker.observe(&unknown()), Transition::None);
        assert_eq!(tracker.connected_session(), None);
        assert!(tracker.is_unknown());
        assert_eq!(tracker.observe(&session(8, false)), Transition::None);
        assert_eq!(tracker.observe(&session(7, true)), Transition::None);
        assert!(!tracker.is_unknown());
        assert_eq!(tracker.observe(&session(7, false)), Transition::Deactivated);
        assert_eq!(tracker.observe(&session(7, false)), Transition::None);
        assert_eq!(
            tracker.observe(&session(7, true)),
            Transition::Activated(Evidence::RdpSession)
        );
    }

    #[test]
    fn off_does_not_invent_a_disconnect_but_keeps_reconnect_evidence() {
        let mut tracker = Tracker::new(Sensitivity::Verified);
        tracker.observe(&session(7, true));
        tracker.set_sensitivity(Sensitivity::Off);
        assert_eq!(tracker.observe(&session(7, true)), Transition::None);
        assert_eq!(tracker.connected_session(), Some(7));
    }

    #[test]
    fn verified_never_activates_on_process_or_virtual_display() {
        // The regression this replaces: a running remote tool, or a virtual
        // display left behind by an old session, moved every window.
        let mut tracker = Tracker::new(Sensitivity::Verified);
        for _ in 0..300 {
            assert_eq!(
                tracker.observe(&obs(
                    false,
                    &["Parsec VDD"],
                    &["anydesk.exe"],
                    &["anydesk.exe"]
                )),
                Transition::None
            );
        }
        assert!(!tracker.is_active());
        // It is still visible as a candidate, which is what the UI shows.
        assert_eq!(
            tracker.verdict(&obs(false, &[], &["anydesk.exe"], &[])),
            Verdict::Candidate(Evidence::ToolRunning("anydesk.exe".into()))
        );
        // A connected socket outranks mere presence in the explanation, while
        // still not being enough to act.
        assert_eq!(
            tracker.verdict(&obs(false, &[], &["rustdesk.exe"], &["rustdesk.exe"])),
            Verdict::Candidate(Evidence::ToolConnected("rustdesk.exe".into()))
        );
    }

    #[test]
    fn a_failed_probe_is_not_a_disconnect() {
        // A query error used to be indistinguishable from "no evidence", which
        // restored every window in the middle of a live session.
        let mut tracker = Tracker::new(Sensitivity::Verified);
        tracker.observe(&session(7, true));
        assert!(tracker.is_active());
        assert_eq!(tracker.observe(&unknown()), Transition::None);
        assert!(tracker.is_active());
        assert!(tracker.is_unknown());
        assert_eq!(tracker.verdict(&unknown()), Verdict::Unknown);
    }

    #[test]
    fn losing_candidate_evidence_cannot_end_a_retained_session() {
        let mut tracker = Tracker::new(Sensitivity::Verified);
        tracker.observe(&session(7, true));
        for _ in 0..3 {
            assert_eq!(
                tracker.observe(&obs(false, &[], &[], &[])),
                Transition::None
            );
        }
        assert!(tracker.is_active());
        assert!(tracker.is_unknown());
    }

    #[test]
    fn lowering_sensitivity_does_not_cancel_an_active_session() {
        let mut tracker = Tracker::new(Sensitivity::Verified);
        tracker.observe(&session(7, true));
        assert!(tracker.is_active());
        tracker.set_sensitivity(Sensitivity::Off);
        assert!(tracker.is_active());
        assert_eq!(tracker.observe(&obs(true, &[], &[], &[])), Transition::None);
        assert_eq!(
            tracker.observe(&obs(false, &[], &[], &[])),
            Transition::None
        );
        assert!(tracker.is_active());
    }

    #[test]
    fn unknown_values_parse_to_off() {
        // A value from a newer build must never enable automatic window moves.
        assert_eq!(Sensitivity::parse("aggressive"), Sensitivity::Off);
        assert_eq!(Sensitivity::parse("balanced"), Sensitivity::Off);
        assert_eq!(Sensitivity::parse("verified"), Sensitivity::Verified);
        assert_eq!(Sensitivity::parse(""), Sensitivity::Off);
    }

    #[test]
    fn current_probes_do_not_authorise_automation() {
        assert!(!Evidence::RdpSession.authorises_automation());
        assert!(!Evidence::ToolConnected("anydesk.exe".into()).authorises_automation());
        assert!(!Evidence::ToolRunning("anydesk.exe".into()).authorises_automation());
        assert!(!Evidence::VirtualDisplay("Parsec VDD".into()).authorises_automation());
    }
}
