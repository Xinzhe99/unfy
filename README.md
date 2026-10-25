<div align="center">

<img src="apps/desktop/src-tauri/icons/128x128@2x.png" width="96" alt="Unfy">

# Unfy

**Bring every window onto the display your remote client can actually see.**

Working from a single-screen laptop into a multi-monitor desktop leaves most of
your windows off-screen. Unfy moves them onto the display you can see, then puts
them back exactly where they were.

[![CI](https://github.com/Xinzhe99/unfy/actions/workflows/ci.yml/badge.svg)](https://github.com/Xinzhe99/unfy/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Xinzhe99/unfy)](https://github.com/Xinzhe99/unfy/releases)
[![Downloads](https://img.shields.io/github/downloads/Xinzhe99/unfy/total)](https://github.com/Xinzhe99/unfy/releases)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows-lightgrey)](#platform-support)

**English** · [简体中文](README.zh-CN.md)

[Download](#download) · [Features](#features) · [How it works](#how-it-works) · [FAQ](#faq) · [Development](#development)

</div>

![Unfy display picker](docs/screenshot-displays.png)

## The problem

Your desktop has three monitors. You connect from a laptop that has one. The
remote client shows a single screen, so the browser, editor and chat windows on
the other two are invisible — you cannot see them, click them, or even guess
where they are.

Most remote tools offer a "switch display" button. That helps, but you still have
to press it every time, and switching the view does not move the windows. Unfy
moves the windows.

## Features

### One action, original window state preserved

Pick the display your remote client shows, press Gather. Windows move over at
their existing size and relative position; maximized windows stay maximized.
Windows already on the target display are left untouched.

### Recovery records: undo is always available

Before anything moves, each window's position, size, display state and monitor
identity are written to an append-only journal. After a gather you can:

- press **Restore layout** to return every window to where it was
- crash, lose power, or kill the process — the next launch can still restore
- move or resize a window yourself during the session; that change is detected
  and kept instead of being overwritten

The journal is flushed to disk *before* the first window moves, so there is no
window of time in which windows have moved but their original positions are gone.

### Two layout modes

| Mode | Behaviour |
|---|---|
| **Move onto target** (default) | Keeps each window's size and relative position. Windows may overlap. |
| **Grid** | Splits the target display into equal cells, one window per cell. |

### Exclude applications

Rules are matched against the executable name, chosen from what is currently
running on the **Exclusions** page — no typing file names from memory.

## Platform support

| Platform | Status |
|---|---|
| **Windows 10 / 11** | Fully supported. Window movement, display identity, recovery records, global hotkeys, tray and autostart are verified on real hardware. |
| **macOS** | Backend implemented against the Accessibility API. Type-checked only, **never run on a Mac**, no installer published. |
| **Linux** | No window backend. |

## Download

Grab the latest `Unfy_x.y.z_x64-setup.exe` from
[Releases](https://github.com/Xinzhe99/unfy/releases).

**The installer is not Authenticode-signed**, so Windows SmartScreen will block
the first run: choose **More info → Run anyway**. Update packages *are* signed
with minisign, and every client verifies them.

Silent install, for rolling out to several machines:

```powershell
Unfy_0.1.0_x64-setup.exe /S
```

Per-user install, no administrator rights required, and no leftover entries after
uninstalling.

## How it works

```
pick target → snapshot eligible windows → journal + fsync → move windows → read back
                                                                         ↓
                          restore replays the journal, verifying geometry and state
```

Three decisions are worth explaining:

**Journal first, move second.** Each window's original rectangle is flushed to
disk before a move is submitted. It costs a few milliseconds per gather; it buys
the guarantee that killing the process at any instant cannot lose the original
positions.

**Submitting is not succeeding.** After every move the window is re-read, and the
result only counts as success if the geometry and display state match the plan.
An application that refuses, snaps back, or cannot be read is reported honestly
and its recovery record is kept.

**Windows we cannot reason about are skipped, with a reason.** Minimized windows,
windows on another virtual desktop, elevated windows and borderless fullscreen
windows are left alone and reported. Moving a window the user cannot see only
adds confusion.

## FAQ

<details>
<summary><b>Some windows did not move. Why?</b></summary>

The **latest result** card lists a verdict per window. Common causes:

- **Minimized** — you could not see it after the move either, so it is skipped
- **Another virtual desktop** — not reachable from the current desktop
- **Running elevated** — a normal-integrity process cannot move an elevated window
- **Refused** — some applications (Chrome, some Electron apps) snap their windows
  back. Unfy reports this and keeps the recovery record.

</details>

<details>
<summary><b>Isn't the remote tool's "switch display" enough?</b></summary>

It is enough to *look* at another screen. It does not move anything, so the
windows stay where they are and you keep switching back and forth. Unfy moves the
windows themselves.

</details>

<details>
<summary><b>Can it detect when a remote session starts?</b></summary>

Only for **RDP**, and only partially. RDP creates a real Terminal Services
session, which Windows reports through a documented API. TeamViewer, AnyDesk,
RustDesk, ToDesk, Sunlogin, Parsec and Sunshine attach to the existing console
session and set no such flag.

There is no public Windows API that answers "is someone controlling this machine
right now" for those tools. Unfy therefore does not guess: it watches for
evidence (a known tool running, a virtual display appearing), shows that evidence
in the interface, and leaves automatic movement to the cases it can actually
verify. Manual gather and restore work regardless, and can be bound to hotkeys.

</details>

<details>
<summary><b>Are my window positions uploaded anywhere?</b></summary>

No. There is no telemetry and no network request except an update check you
trigger. Recovery records live in `%APPDATA%\Unfy\journal`, and the diagnostics
bundle contains no window titles, file paths or screen captures.

</details>

## Development

Requires Rust stable with the MSVC toolchain and Node 22+. The first bundle
downloads NSIS/WiX from GitHub; if that is blocked, point
`TAURI_BUNDLER_TOOLS_GITHUB_MIRROR` at a mirror.

```bash
cargo test --workspace                          # unit and integration tests
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p unfy-platform-win --bin e2e        # real desktop: creates and moves its own windows
npm --prefix apps/desktop run build             # typecheck + tests + bundle
```

`e2e` only touches windows it created itself, so it is safe to run on a desktop
you are using.

### Layout

```
crates/domain/       geometry, identity, state machine; no OS dependency
crates/layout/       pure layout planning
crates/journal/      append-only transaction log, CRC + crash replay
crates/engine/       gather/restore state machine; the only writer
crates/platform/     backend traits + a fake backend
crates/platform/win/ Windows implementation
crates/platform/mac/ macOS implementation
crates/detect/       remote-session evidence collection
crates/settings/     settings persistence and migration
apps/desktop/        Tauri + React interface
```

### Documentation

- [Release and signing](docs/release.md) — keys, certificates, CI gates
- [Verification record](docs/verification.md) — what was actually run, and what was not
- [Product specification](docs/spec/usability-review/README.md) — behaviour contracts

## Deliberately out of scope

- Virtual displays, forced single-screen mode
- A privileged helper (so elevated windows cannot be moved)
- Telemetry, accounts, licensing
- Linux support

## License

MIT
