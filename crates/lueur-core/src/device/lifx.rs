//! LIFX LAN protocol: small little-endian binary frames over UDP 56700.
//!
//! Open by design — no pairing, no key. Colours are HSBK (hue, saturation,
//! brightness, kelvin), with a transition time per command, so the bulb does
//! its own interpolation between frames.

use super::common::{LastSent, Throttle, broadcast_targets, rgb_to_hsv, udp_to};
use super::{DeviceKind, DiscoveredDevice, LightStream, REST_BRIGHT, REST_KELVIN};
use crate::color::BulbOutput;
use crate::yeelight::Effect;
use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

pub const PORT: u16 = 56700;

const GET_SERVICE: u16 = 2;
const STATE_SERVICE: u16 = 3;
const GET_LABEL: u16 = 23;
const STATE_LABEL: u16 = 25;
const SET_COLOR: u16 = 102;
const SET_LIGHT_POWER: u16 = 117;

const HEADER_LEN: usize = 36;
/// Our `source` id; LIFX echoes it so replies can be told apart.
const SOURCE: u32 = 0x4C55_4555; // "LUEU"

const MIN_KELVIN: u16 = 2500;
const MAX_KELVIN: u16 = 9000;

/// LIFX asks clients to stay under 20 messages a second per bulb.
const FRAME_INTERVAL: Duration = Duration::from_millis(50);

/// Build a frame. `target` is the bulb's MAC (all zero with `tagged` for a
/// broadcast to every bulb).
pub fn frame(
    msg_type: u16,
    target: [u8; 6],
    tagged: bool,
    sequence: u8,
    payload: &[u8],
) -> Vec<u8> {
    let size = (HEADER_LEN + payload.len()) as u16;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_le_bytes());
    // protocol 1024, addressable, tagged when broadcasting.
    let flags: u16 = 1024 | 0x1000 | if tagged { 0x2000 } else { 0 };
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&SOURCE.to_le_bytes());
    out.extend_from_slice(&target);
    out.extend_from_slice(&[0, 0]); // target padding
    out.extend_from_slice(&[0; 6]); // reserved
    out.push(0); // no ack, no response
    out.push(sequence);
    out.extend_from_slice(&[0; 8]); // reserved
    out.extend_from_slice(&msg_type.to_le_bytes());
    out.extend_from_slice(&[0; 2]); // reserved
    out.extend_from_slice(payload);
    out
}

/// A received frame's type, sender MAC and payload.
pub fn parse_frame(raw: &[u8]) -> Option<(u16, [u8; 6], &[u8])> {
    if raw.len() < HEADER_LEN {
        return None;
    }
    let size = u16::from_le_bytes([raw[0], raw[1]]) as usize;
    if size < HEADER_LEN || size > raw.len() {
        return None;
    }
    let protocol = u16::from_le_bytes([raw[2], raw[3]]) & 0x0FFF;
    if protocol != 1024 {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&raw[8..14]);
    let msg_type = u16::from_le_bytes([raw[32], raw[33]]);
    Some((msg_type, mac, &raw[HEADER_LEN..size]))
}

/// HSBK and transition for one output.
pub fn set_color_payload(output: BulbOutput, duration_ms: u32) -> Vec<u8> {
    let (hue, sat, bri, kelvin) = match output {
        BulbOutput::Color { rgb, bright } => {
            let (h, s, _) = rgb_to_hsv(rgb);
            (
                (h / 360.0 * 65535.0).round() as u16,
                (s * 65535.0).round() as u16,
                bri16(bright),
                3500,
            )
        }
        BulbOutput::White { kelvin, bright } => {
            (0, 0, bri16(bright), kelvin.clamp(MIN_KELVIN, MAX_KELVIN))
        }
    };
    let mut p = vec![0u8]; // reserved
    for v in [hue, sat, bri, kelvin] {
        p.extend_from_slice(&v.to_le_bytes());
    }
    p.extend_from_slice(&duration_ms.to_le_bytes());
    p
}

fn bri16(bright: u8) -> u16 {
    (u32::from(bright.min(100)) * 65535 / 100) as u16
}

/// Parse a 12-hex-digit MAC (as stored in [`DeviceKind::Lifx`]).
pub fn parse_mac(text: &str) -> Option<[u8; 6]> {
    if text.len() != 12 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(mac)
}

fn mac_hex(mac: [u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02x}")).collect()
}

fn target_of(text: &str) -> io::Result<[u8; 6]> {
    parse_mac(text).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad LIFX target"))
}

pub fn set_power(ip: &str, port: u16, target: &str, on: bool) -> io::Result<()> {
    let socket = udp_to(ip, port)?;
    let mut payload = (if on { 65535u16 } else { 0 }).to_le_bytes().to_vec();
    payload.extend_from_slice(&300u32.to_le_bytes());
    let msg = frame(SET_LIGHT_POWER, target_of(target)?, false, 0, &payload);
    for _ in 0..2 {
        socket.send(&msg)?;
    }
    Ok(())
}

pub struct LifxStream {
    socket: UdpSocket,
    target: [u8; 6],
    sequence: u8,
    last: LastSent,
    pace: Throttle,
}

impl LifxStream {
    pub fn open(ip: &str, port: u16, target: &str) -> io::Result<Self> {
        Ok(LifxStream {
            socket: udp_to(ip, port)?,
            target: target_of(target)?,
            sequence: 0,
            last: LastSent::default(),
            pace: Throttle::new(FRAME_INTERVAL),
        })
    }

    fn send(&mut self, output: BulbOutput, duration_ms: u32) -> io::Result<()> {
        self.sequence = self.sequence.wrapping_add(1);
        let msg = frame(
            SET_COLOR,
            self.target,
            false,
            self.sequence,
            &set_color_payload(output, duration_ms),
        );
        self.socket.send(&msg).map(|_| ())
    }
}

impl LightStream for LifxStream {
    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        let ms = if effect == Effect::Smooth {
            duration_ms
        } else {
            0
        };
        self.send(output, ms)?;
        self.last.mark(output);
        Ok(())
    }

    fn rest(&mut self) {
        let rest = BulbOutput::White {
            kelvin: REST_KELVIN.max(MIN_KELVIN),
            bright: REST_BRIGHT,
        };
        let _ = self.send(rest, 400);
    }
}

/// Broadcast `GetService`, then ask each bulb that answers for its label.
pub fn discover(timeout: Duration) -> io::Result<Vec<DiscoveredDevice>> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(150)))?;
    let probe = frame(GET_SERVICE, [0; 6], true, 0, &[]);
    for target in broadcast_targets() {
        for _ in 0..2 {
            let _ = socket.send_to(&probe, SocketAddrV4::new(target, PORT));
        }
    }

    let mut found: HashMap<[u8; 6], DiscoveredDevice> = HashMap::new();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 1024];
    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, SocketAddr::V4(from))) => {
                let Some((msg_type, mac, payload)) = parse_frame(&buf[..n]) else {
                    continue;
                };
                match msg_type {
                    // Service 1 is UDP; the payload carries the port to use.
                    STATE_SERVICE if payload.len() >= 5 && payload[0] == 1 => {
                        let port =
                            u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                        let port = u16::try_from(port).unwrap_or(PORT);
                        if let std::collections::hash_map::Entry::Vacant(slot) = found.entry(mac) {
                            let _ = socket.send_to(&frame(GET_LABEL, mac, false, 1, &[]), from);
                            slot.insert(DiscoveredDevice {
                                id: format!("lifx:{}", mac_hex(mac)),
                                ip: from.ip().to_string(),
                                port,
                                model: String::new(),
                                name: String::new(),
                                support: Vec::new(),
                                fw_ver: None,
                                kind: DeviceKind::Lifx {
                                    target: mac_hex(mac),
                                },
                                paired: true,
                            });
                        }
                    }
                    STATE_LABEL => {
                        if let Some(d) = found.get_mut(&mac) {
                            d.name = label(payload);
                        }
                    }
                    _ => {}
                }
            }
            Ok(_) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
            }
            Err(e) => return Err(e),
        }
    }
    Ok(found.into_values().collect())
}

fn label(payload: &[u8]) -> String {
    let raw = &payload[..payload.len().min(32)];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    #[test]
    fn get_service_broadcast_matches_the_spec() {
        let f = frame(GET_SERVICE, [0; 6], true, 0, &[]);
        assert_eq!(f.len(), 36);
        assert_eq!(&f[0..2], &[36, 0]);
        // protocol 1024 | addressable | tagged = 0x3400
        assert_eq!(&f[2..4], &[0x00, 0x34]);
        assert_eq!(&f[32..34], &[2, 0]);
    }

    #[test]
    fn frames_round_trip() {
        let mac = [0xd0, 0x73, 0xd5, 1, 2, 3];
        let f = frame(SET_COLOR, mac, false, 7, &[1, 2, 3]);
        let (t, m, p) = parse_frame(&f).unwrap();
        assert_eq!((t, m, p), (SET_COLOR, mac, &[1u8, 2, 3][..]));
        assert_eq!(&f[2..4], &[0x00, 0x14], "unicast frames are not tagged");
    }

    #[test]
    fn rejects_short_or_foreign_frames() {
        assert!(parse_frame(&[0; 10]).is_none());
        let mut f = frame(GET_SERVICE, [0; 6], true, 0, &[]);
        f[2] = 0x01; // protocol 1025
        assert!(parse_frame(&f).is_none());
    }

    #[test]
    fn colour_becomes_hsbk() {
        let p = set_color_payload(
            BulbOutput::Color {
                rgb: Rgb::new(0, 0, 255),
                bright: 100,
            },
            80,
        );
        assert_eq!(p.len(), 13);
        let hue = u16::from_le_bytes([p[1], p[2]]);
        assert!((43_600..=43_700).contains(&hue), "blue is 240°, got {hue}");
        assert_eq!(u16::from_le_bytes([p[3], p[4]]), 65535);
        assert_eq!(u16::from_le_bytes([p[5], p[6]]), 65535);
        assert_eq!(u32::from_le_bytes([p[9], p[10], p[11], p[12]]), 80);
    }

    #[test]
    fn white_is_unsaturated_at_a_clamped_kelvin() {
        let p = set_color_payload(
            BulbOutput::White {
                kelvin: 1700,
                bright: 50,
            },
            0,
        );
        assert_eq!(u16::from_le_bytes([p[3], p[4]]), 0);
        assert_eq!(u16::from_le_bytes([p[7], p[8]]), MIN_KELVIN);
    }

    #[test]
    fn macs_parse_and_print() {
        let mac = parse_mac("d073d5010203").unwrap();
        assert_eq!(mac, [0xd0, 0x73, 0xd5, 1, 2, 3]);
        assert_eq!(mac_hex(mac), "d073d5010203");
        assert!(parse_mac("d073").is_none());
        assert!(parse_mac("zz73d5010203").is_none());
    }

    #[test]
    fn labels_stop_at_nul() {
        let mut p = b"Desk".to_vec();
        p.resize(32, 0);
        assert_eq!(label(&p), "Desk");
    }
}
