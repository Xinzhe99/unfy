//! Platform observations for the detector.
//!
//! Windows is the only implementation today; macOS and Linux return "no
//! evidence", which makes the detector inert rather than wrong there.
//!
//! Each function answers one question and returns plain data. A failure to
//! observe is reported as "no evidence", never as an error that stops the poll:
//! losing the ability to read the module list must not crash the app, and it must
//! not silently look like "a remote session just ended" either. The caller
//! distinguishes the two by comparing against the previous state.

use unfy_domain::DisplaySnapshot;

use crate::{is_known_tool, is_virtual_display, known_ports, Observation};

/// Whether this process is associated with a Terminal Services client session.
///
/// Two independent signals, because `SM_REMOTESESSION` is documented to
/// misreport under RemoteFX vGPU: the system metric, and the session's protocol
/// type. Either one being remote is enough.
pub fn rdp_session() -> bool {
    use windows::Win32::System::RemoteDesktop::{
        WTSClientProtocolType, WTSQuerySessionInformationW, WTS_CURRENT_SERVER_HANDLE,
        WTS_CURRENT_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};

    let metric = unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0;

    // `WTSClientProtocolType`: 0 = console, 2 = RDP. The buffer is freed by the
    // API, not by us, so it is leaked into a local variable and released with
    // `WTSFreeMemory`.
    let mut buffer = windows::core::PWSTR::null();
    let mut bytes: u32 = 0;
    let queried = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSClientProtocolType,
            &mut buffer,
            &mut bytes,
        )
    };
    let mut protocol_remote = false;
    if queried.is_ok() && !buffer.is_null() {
        if bytes as usize >= std::mem::size_of::<u16>() {
            protocol_remote = unsafe { std::ptr::read_unaligned(buffer.0) } == 2;
        }
        unsafe { windows::Win32::System::RemoteDesktop::WTSFreeMemory(buffer.0.cast()) };
    }

    metric || protocol_remote
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
pub fn remote_tools_running() -> Vec<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while has_entry {
        let end = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if let Some(tool) = is_known_tool(&name) {
            found.push(tool.to_string());
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }

    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
    found.sort();
    found.dedup();
    found
}

/// Known tools holding an established TCP connection on one of their own ports.
///
/// Only *established* connections to the tool's documented port count. A
/// listening socket does not: every tool that is merely installed listens on
/// startup, which would make "installed" indistinguishable from "in use".
pub fn remote_tools_connected() -> Vec<String> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCP_STATE_ESTAB, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows::Win32::Networking::WinSock::AF_INET;

    let ports = known_ports();
    if ports.is_empty() {
        return Vec::new();
    }

    let mut size: u32 = 0;
    // First call sizes the buffer; the documented return is
    // ERROR_INSUFFICIENT_BUFFER, which windows-rs surfaces as an error.
    unsafe {
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if size == 0 {
        return Vec::new();
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
        return Vec::new();
    }

    // The table starts with a row count, then a packed array of rows.
    let count = u32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let header = std::mem::size_of::<u32>();
    let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    if header + count * row_size > buffer.len() {
        return Vec::new();
    }

    let mut owning_pids: Vec<u32> = Vec::new();
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
        if ports.contains(&local_port) || ports.contains(&remote_port) {
            owning_pids.push(row.dwOwningPid);
        }
    }

    if owning_pids.is_empty() {
        return Vec::new();
    }

    owning_pids.sort_unstable();
    owning_pids.dedup();

    let mut found: Vec<String> = owning_pids
        .into_iter()
        .filter_map(executable_name)
        .filter_map(|name| is_known_tool(&name).map(str::to_string))
        .collect();
    found.sort();
    found.dedup();
    found
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
    Observation {
        rdp_active: rdp_session(),
        virtual_displays: virtual_displays(displays),
        tools_running: remote_tools_running(),
        tools_connected: remote_tools_connected(),
        // The OS cannot report a manual marker; the desktop layer owns that flag
        // and overwrites this field before feeding the tracker.
        manual_marker: false,
    }
}

/// Human-readable summary for the diagnostics bundle.
///
/// Contains tool names and display labels only: no window titles, no user name,
/// no addresses.
pub fn summary(observation: &Observation) -> String {
    if observation.is_empty() {
        return "no remote-session evidence".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
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
