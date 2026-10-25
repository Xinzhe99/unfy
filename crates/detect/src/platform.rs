//! Platform observations for the detector.
//!
//! Failed queries remain unavailable; they never become empty successful polls.
//! RDP membership, process names and sockets are diagnostic candidates only.

use unfy_domain::DisplaySnapshot;

use crate::{is_known_tool, is_virtual_display, known_ports, ports_for, Observation, RdpSession};

/// Query only the session hosting this process. Console membership is not inbound
/// RDP evidence; transitional states and failed queries remain unknown.
pub fn rdp_session() -> Option<Option<RdpSession>> {
    use windows::Win32::System::RemoteDesktop::{
        ProcessIdToSessionId, WTSActive, WTSClientProtocolType, WTSConnectState, WTSDisconnected,
        WTSGetActiveConsoleSessionId, WTSSessionInfoEx, WTSINFOEXW, WTS_CONNECTSTATE_CLASS,
        WTS_SESSIONSTATE_UNLOCK,
    };
    let mut id = 0;
    unsafe { ProcessIdToSessionId(std::process::id(), &mut id) }.ok()?;
    let protocol: u16 = query_session(id, WTSClientProtocolType)?;
    if protocol != 2 {
        return Some(None);
    }
    if unsafe { WTSGetActiveConsoleSessionId() } == id {
        return None;
    }
    let state: WTS_CONNECTSTATE_CLASS = query_session(id, WTSConnectState)?;
    if state == WTSActive {
        let info: WTSINFOEXW = query_session(id, WTSSessionInfoEx)?;
        if info.Level != 1 {
            return None;
        }
        let session = unsafe { info.Data.WTSInfoExLevel1 };
        if session.SessionId != id
            || session.SessionState != WTSActive
            || session.SessionFlags != WTS_SESSIONSTATE_UNLOCK as i32
        {
            return None;
        }
        Some(Some(RdpSession {
            id,
            connected: true,
        }))
    } else if state == WTSDisconnected {
        Some(Some(RdpSession {
            id,
            connected: false,
        }))
    } else {
        None
    }
}

fn query_session<T: Copy>(
    id: u32,
    class: windows::Win32::System::RemoteDesktop::WTS_INFO_CLASS,
) -> Option<T> {
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTS_CURRENT_SERVER_HANDLE,
    };
    let mut buffer = windows::core::PWSTR::null();
    let mut bytes = 0;
    let queried = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            id,
            class,
            &mut buffer,
            &mut bytes,
        )
    };
    let result =
        if queried.is_ok() && !buffer.is_null() && bytes as usize >= std::mem::size_of::<T>() {
            Some(unsafe { std::ptr::read_unaligned(buffer.0.cast::<T>()) })
        } else {
            None
        };
    if !buffer.is_null() {
        unsafe { WTSFreeMemory(buffer.0.cast()) };
    }
    result
}

/// Names of virtual display devices among the current topology.
///
/// The device path contains the monitor's EDID manufacturer id, which is how a
/// software display is distinguished from real hardware.
pub fn virtual_displays(displays: &[DisplaySnapshot]) -> Vec<String> {
    let mut found: Vec<String> = displays
        .iter()
        .filter(|d| is_virtual_display(&d.key.native_id))
        .map(|d| d.label.clone())
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Running processes that belong to known remote-control tools.
pub fn remote_tools_running() -> Option<Vec<String>> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return None;
    };

    let mut found = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
    while next.is_ok() {
        let end = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if let Some(tool) = is_known_tool(&name) {
            found.push(tool.to_string());
        }
        next = unsafe { Process32NextW(snapshot, &mut entry) };
    }

    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
    if next.err()?.code() != windows::Win32::Foundation::ERROR_NO_MORE_FILES.to_hresult() {
        return None;
    }
    found.sort();
    found.dedup();
    Some(found)
}

/// Known tools holding an established TCP connection on one of their own ports.
///
/// Only *established* connections to the tool's documented port count. A
/// listening socket does not: every tool that is merely installed listens on
/// startup, which would make "installed" indistinguishable from "in use".
pub fn remote_tools_connected() -> Option<Vec<String>> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCP_STATE_ESTAB, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows::Win32::Networking::WinSock::AF_INET;

    let mut size: u32 = 0;
    // GetExtendedTcpTable returns a Win32 status code, including on its sizing call.
    let sizing = unsafe {
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if sizing != 0 && sizing != windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER.0 {
        return None;
    }
    if size < std::mem::size_of::<u32>() as u32 {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];
    let ok = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if ok != 0 {
        // Non-zero is a Win32 error code (no ERROR_SUCCESS).
        return None;
    }

    // The table starts with a row count, then a packed array of rows.
    let count = u32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let header = std::mem::size_of::<u32>();
    if size as usize > buffer.len() || (size as usize) < header {
        return None;
    }
    let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    if count > (size as usize - header) / row_size {
        return None;
    }

    let mut found = Vec::new();
    let known = known_ports();
    for index in 0..count {
        let offset = header + index * row_size;
        let row: MIB_TCPROW_OWNER_PID =
            unsafe { std::ptr::read_unaligned(buffer[offset..].as_ptr().cast()) };
        if row.dwState != MIB_TCP_STATE_ESTAB.0 as u32 {
            continue;
        }
        // Ports are stored in network byte order in the low 16 bits.
        let local_port = u16::from_be((row.dwLocalPort & 0xffff) as u16);
        let remote_port = u16::from_be((row.dwRemotePort & 0xffff) as u16);
        if !known.contains(&local_port) && !known.contains(&remote_port) {
            continue;
        }
        let name = executable_name(row.dwOwningPid)?;
        let Some(tool) = is_known_tool(&name) else {
            continue;
        };
        let ports = ports_for(tool);
        if ports.contains(&local_port) || ports.contains(&remote_port) {
            found.push(tool.to_string());
        }
    }

    found.sort();
    found.dedup();
    Some(found)
}

fn executable_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;

    let mut buffer = [0u16; 512];
    let mut size = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
    if queried.is_err() || size == 0 {
        return None;
    }

    let full = String::from_utf16_lossy(&buffer[..size as usize]);
    full.rsplit(['\\', '/']).next().map(str::to_string)
}

/// One poll: every observation the detector consumes.
pub fn observe(displays: &[DisplaySnapshot]) -> Observation {
    let (Some(rdp_session), Some(tools_running), Some(tools_connected)) = (
        rdp_session(),
        remote_tools_running(),
        remote_tools_connected(),
    ) else {
        return Observation::default();
    };
    Observation {
        available: true,
        rdp_active: rdp_session.is_some(),
        rdp_session,
        virtual_displays: virtual_displays(displays),
        tools_running,
        tools_connected,
    }
}

/// Human-readable summary for the diagnostics bundle.
///
/// Contains tool names and display labels only: no window titles, no user name,
/// no addresses.
pub fn summary(observation: &Observation) -> String {
    if !observation.available {
        return "remote-session observation unavailable".to_string();
    }
    if observation.is_empty() {
        return "no remote-session evidence".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(session) = observation.rdp_session {
        parts.push(format!(
            "session={} connected={}",
            session.id, session.connected
        ));
    }
    if observation.rdp_active {
        parts.push("rdp".to_string());
    }
    if !observation.virtual_displays.is_empty() {
        parts.push(format!(
            "virtual={}",
            observation.virtual_displays.join(",")
        ));
    }
    if !observation.tools_connected.is_empty() {
        parts.push(format!(
            "connected={}",
            observation.tools_connected.join(",")
        ));
    }
    if !observation.tools_running.is_empty() {
        parts.push(format!("running={}", observation.tools_running.join(",")));
    }
    parts.join(" ")
}
