//! Other computers running Lueur on the network, and the bulbs each drives.
//!
//! A thread of its own broadcasts this instance's claims to the LAN on
//! [`PORT`] (see `lueur_core::device::lease`): at once when they change,
//! then every second while it drives any bulb; an instance that drives
//! nothing keeps quiet. It hears the others' at the same port. What it
//! hears decides which bulbs the effects leave alone, and each time that
//! changes the window is told, so it shows who drives which light.
//!
//! Only announcements from a private or link-local address are read, at
//! most `lease::MAX_DATAGRAM` bytes of them, and nothing they say ever
//! reaches a bulb: the worst a forged one can do is make a light wait,
//! which the user's take overrides.

use crate::error::AppError;
use crate::state::AppState;
use crate::tray;
use lueur_core::device::common::{broadcast_targets, is_lan};
use lueur_core::device::lease::{self, Announce};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The port every instance listens and broadcasts on.
pub const PORT: u16 = 47_474;
/// How long the thread waits for an announcement before looking at its own.
const POLL: Duration = Duration::from_millis(100);
/// How often the broadcast addresses are worked out again (the PC may
/// change networks).
const RETARGET: Duration = Duration::from_secs(30);
/// The interface hears that another computer took or let go of a bulb at
/// most this often, however many announcements arrive.
const NOTIFY_GAP: Duration = Duration::from_millis(250);

/// Talk with the other instances for as long as the app runs.
pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("lueur-peers".into())
        .spawn(move || run(&app));
}

/// The socket announcements go out of and come in on. Another program on
/// the port only costs hearing the others: this instance still tells them.
fn bind() -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT)).or_else(|e| {
        log::warn!("cannot listen for other computers running Lueur: {e}");
        UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
    })?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(POLL))?;
    Ok(socket)
}

fn send(socket: &UdpSocket, targets: &[Ipv4Addr], a: &Announce) {
    let bytes = a.encode();
    for ip in targets {
        if let Err(e) = socket.send_to(&bytes, SocketAddrV4::new(*ip, PORT)) {
            log::debug!("cannot tell the other computers running Lueur: {e}");
        }
    }
}

fn run(app: &AppHandle) {
    let leases = Arc::clone(&app.state::<AppState>().leases);
    let socket = match bind() {
        Ok(s) => s,
        Err(e) => {
            log::error!("cannot talk with other computers running Lueur: {e}");
            return;
        }
    };
    let mut buf = vec![0u8; lease::MAX_DATAGRAM + 1];
    let mut targets: Vec<Ipv4Addr> = Vec::new();
    let mut targeted: Option<Instant> = None;
    let mut seen = leases.version();
    let mut notified: Option<Instant> = None;
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, SocketAddr::V4(from))) if n <= lease::MAX_DATAGRAM && is_lan(*from.ip()) => {
                if let Some(a) = Announce::decode(&buf[..n]) {
                    leases.hear(a);
                }
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            // Windows reports this way an earlier send nobody received.
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
            Err(e) => {
                log::debug!("listening for other computers running Lueur: {e}");
                std::thread::sleep(POLL);
            }
        }
        leases.tick();
        if let Some(a) = leases.outgoing() {
            if targeted.is_none_or(|t| t.elapsed() >= RETARGET) {
                targets = broadcast_targets();
                targeted = Some(Instant::now());
            }
            send(&socket, &targets, &a);
        }
        let version = leases.version();
        // A change held back by the gap goes out on a later pass.
        if version != seen && notified.is_none_or(|t| t.elapsed() >= NOTIFY_GAP) {
            seen = version;
            notified = Some(Instant::now());
            let _ = app.emit(tray::EFFECT_CHANGED, None::<AppError>);
        }
    }
}

/// Tell the others at once that this instance lets go of every bulb, as it
/// quits, rather than leave them waiting for its claims to run out.
pub fn leave(state: &AppState) {
    state.leases.leave();
    let Some(a) = state.leases.outgoing() else {
        return;
    };
    if let Ok(socket) = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        && socket.set_broadcast(true).is_ok()
    {
        send(&socket, &broadcast_targets(), &a);
    }
}

/// This computer's name, as the others show it.
pub fn host_name() -> String {
    let name = system_name().unwrap_or_default();
    // A Mac's is `Name.local`, a Linux box's may be fully qualified.
    let short = name.split('.').next().unwrap_or_default();
    lease::clean_host(short)
}

#[cfg(windows)]
fn system_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(unix)]
fn system_name() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length, which is what the
    // call is told; it writes a NUL-terminated name into it, or fails.
    let r = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if r != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_found_and_kept_short() {
        let name = host_name();
        assert!(!name.contains('.'), "{name}");
        assert!(name.chars().count() <= 48);
    }
}
