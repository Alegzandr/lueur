//! A one-shot mDNS browse, enough to find Hue bridges and Nanoleaf controllers.
//!
//! The query goes out from an ephemeral port, which makes responders answer
//! by unicast straight back to it (RFC 6762 §6.7, "legacy unicast"). That
//! keeps the search to a single socket with no fixed port to bind, the same
//! shape as the Yeelight SSDP search.
//!
//! A multicast leaves from one interface only, the default route's unless
//! told otherwise, and on a computer with a VPN or a virtual machine's
//! adapter that is often not the one the bulbs are on. So the query goes
//! out on every LAN interface in turn; the answers, unicast, all come back
//! to the one socket.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const MDNS_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;

/// One service instance that answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Instance {
    pub ip: String,
    /// Service port from the SRV record (0 when none was sent).
    pub port: u16,
    /// Instance label, e.g. `Hue Bridge - 1A2B3C`.
    pub name: String,
    /// TXT `key=value` pairs, keys lower-cased.
    pub txt: HashMap<String, String>,
}

/// Browse `service` (e.g. `_hue._tcp.local`) for `timeout`.
pub fn browse(service: &str, timeout: Duration) -> io::Result<Vec<Instance>> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    let query = build_query(service);
    let interfaces = super::common::lan_interfaces();
    ask(&socket, &query, &interfaces)?;

    let mut found: HashMap<String, Instance> = HashMap::new();
    let deadline = Instant::now() + timeout;
    // Asked again every so often while there is time to hear back: the
    // first datagrams of a search are the likeliest to be lost (the
    // responder's ARP for us, a Wi-Fi radio waking up).
    let mut ask_again = Instant::now() + ASK_AGAIN;
    let mut buf = [0u8; 9000];
    while Instant::now() < deadline {
        if Instant::now() >= ask_again && deadline - Instant::now() > HEAR_BACK {
            ask_again += ASK_AGAIN;
            let _ = ask(&socket, &query, &interfaces);
        }
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let IpAddr::V4(src) = from.ip() else { continue };
                for inst in parse_response(&buf[..n], service, src) {
                    found.insert(inst.ip.clone(), inst);
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
            }
            Err(e) => return Err(e),
        }
    }
    Ok(found.into_values().collect())
}

/// How long a search waits before asking again.
const ASK_AGAIN: Duration = Duration::from_secs(1);
/// The least time left for an answer to be worth asking again.
const HEAR_BACK: Duration = Duration::from_millis(300);

/// Send `query` twice on every interface in `interfaces`, or on the default
/// route's when none would take it.
fn ask(socket: &UdpSocket, query: &[u8], interfaces: &[Ipv4Addr]) -> io::Result<()> {
    let target = SocketAddrV4::new(MDNS_ADDR, MDNS_PORT);
    let mut sent = false;
    for iface in interfaces {
        // An interface that will not take it (down, or gone since it was
        // listed) is skipped; the others still search.
        if socket2::SockRef::from(socket)
            .set_multicast_if_v4(iface)
            .is_ok()
        {
            for _ in 0..2 {
                sent |= socket.send_to(query, target).is_ok();
            }
        }
    }
    if !sent {
        socket2::SockRef::from(socket).set_multicast_if_v4(&Ipv4Addr::UNSPECIFIED)?;
        for _ in 0..2 {
            socket.send_to(query, target)?;
        }
    }
    Ok(())
}

/// A PTR question for `service`, with the unicast-response bit set.
pub fn build_query(service: &str) -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    write_name(&mut q, service);
    q.extend_from_slice(&TYPE_PTR.to_be_bytes());
    q.extend_from_slice(&0x8001u16.to_be_bytes()); // QU + class IN
    q
}

fn write_name(out: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.') {
        let bytes = &label.as_bytes()[..label.len().min(63)];
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    }
    out.push(0);
}

/// Read a (possibly compressed) name at `pos`; returns it and the position
/// just past it in the original record.
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut end = None;
    // Bound the pointer chase so a malicious loop cannot spin us.
    for _ in 0..64 {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            return Some((labels.join("."), end.unwrap_or(pos + 1)));
        }
        if len & 0xC0 == 0xC0 {
            let ptr = ((len & 0x3F) << 8) | *msg.get(pos + 1)? as usize;
            end.get_or_insert(pos + 2);
            pos = ptr;
            continue;
        }
        let label = msg.get(pos + 1..pos + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        pos += 1 + len;
    }
    None
}

fn u16_at(msg: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*msg.get(pos)?, *msg.get(pos + 1)?]))
}

/// Pull the instances of `service` out of one response packet from `src`.
pub fn parse_response(msg: &[u8], service: &str, src: Ipv4Addr) -> Vec<Instance> {
    parse_records(msg, service, src).unwrap_or_default()
}

fn parse_records(msg: &[u8], service: &str, src: Ipv4Addr) -> Option<Vec<Instance>> {
    let flags = u16_at(msg, 2)?;
    if flags & 0x8000 == 0 {
        return None; // a query, not an answer
    }
    let qd = u16_at(msg, 4)? as usize;
    let records = u16_at(msg, 6)? as usize + u16_at(msg, 8)? as usize + u16_at(msg, 10)? as usize;
    let mut pos = 12;
    for _ in 0..qd {
        let (_, next) = read_name(msg, pos)?;
        pos = next + 4;
    }

    let service = service.trim_end_matches('.').to_ascii_lowercase();
    // Lower-cased for matching, original case kept for display.
    let mut instances: Vec<(String, String)> = Vec::new();
    let mut srv: HashMap<String, (u16, String)> = HashMap::new();
    let mut txt: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut addrs: HashMap<String, Ipv4Addr> = HashMap::new();

    for _ in 0..records {
        let (name, next) = read_name(msg, pos)?;
        let rtype = u16_at(msg, next)?;
        let rdlen = u16_at(msg, next + 8)? as usize;
        let rdata = next + 10;
        let rend = rdata + rdlen;
        if rend > msg.len() {
            return None;
        }
        let key = name.to_ascii_lowercase();
        match rtype {
            TYPE_PTR if key == service => {
                if let Some((target, _)) = read_name(msg, rdata) {
                    let k = target.to_ascii_lowercase();
                    if !instances.iter().any(|(x, _)| *x == k) {
                        instances.push((k, target));
                    }
                }
            }
            TYPE_SRV => {
                let port = u16_at(msg, rdata + 4)?;
                let (host, _) = read_name(msg, rdata + 6)?;
                srv.insert(key.clone(), (port, host.to_ascii_lowercase()));
                if key.ends_with(&service) && !instances.iter().any(|(k, _)| *k == key) {
                    instances.push((key, name));
                }
            }
            TYPE_TXT => {
                let mut pairs = HashMap::new();
                let mut p = rdata;
                while p < rend {
                    let len = msg[p] as usize;
                    let Some(s) = msg.get(p + 1..p + 1 + len) else {
                        break;
                    };
                    let s = String::from_utf8_lossy(s);
                    match s.split_once('=') {
                        Some((k, v)) => pairs.insert(k.to_ascii_lowercase(), v.to_string()),
                        None => pairs.insert(s.to_ascii_lowercase(), String::new()),
                    };
                    p += 1 + len;
                }
                if key.ends_with(&service) && !instances.iter().any(|(k, _)| *k == key) {
                    instances.push((key.clone(), name));
                }
                txt.insert(key, pairs);
            }
            TYPE_A if rdlen == 4 => {
                let a = Ipv4Addr::new(msg[rdata], msg[rdata + 1], msg[rdata + 2], msg[rdata + 3]);
                addrs.insert(key, a);
            }
            _ => {}
        }
        pos = rend;
    }

    Some(
        instances
            .into_iter()
            .map(|(inst, original)| {
                let (port, host) = srv.get(&inst).cloned().unwrap_or_default();
                let ip = addrs.get(&host).copied().unwrap_or(src);
                // The instance label is the part before the service name.
                let label_len = original.len().saturating_sub(service.len() + 1);
                let name = original.get(..label_len).unwrap_or(&original).to_string();
                Instance {
                    ip: ip.to_string(),
                    port,
                    name,
                    txt: txt.get(&inst).cloned().unwrap_or_default(),
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rr(out: &mut Vec<u8>, name: &str, rtype: u16, rdata: &[u8]) {
        write_name(out, name);
        out.extend_from_slice(&rtype.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&120u32.to_be_bytes());
        out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        out.extend_from_slice(rdata);
    }

    fn name_bytes(name: &str) -> Vec<u8> {
        let mut v = Vec::new();
        write_name(&mut v, name);
        v
    }

    #[test]
    fn query_asks_for_ptr_with_unicast_reply() {
        let q = build_query("_hue._tcp.local");
        assert_eq!(&q[4..6], &[0, 1], "one question");
        assert_eq!(&q[12..17], b"\x04_hue");
        assert_eq!(&q[q.len() - 4..], &[0, 12, 0x80, 1]);
    }

    #[test]
    fn parses_a_hue_answer() {
        let service = "_hue._tcp.local";
        let inst = "Hue Bridge - 1A2B3C._hue._tcp.local";
        let mut msg = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 3];
        rr(&mut msg, service, TYPE_PTR, &name_bytes(inst));
        let mut srv = vec![0, 0, 0, 0, 0x01, 0xBB];
        srv.extend(name_bytes("hue-bridge.local"));
        rr(&mut msg, inst, TYPE_SRV, &srv);
        let mut txt = Vec::new();
        for s in ["bridgeid=001788fffe1a2b3c", "modelid=BSB002"] {
            txt.push(s.len() as u8);
            txt.extend_from_slice(s.as_bytes());
        }
        rr(&mut msg, inst, TYPE_TXT, &txt);
        rr(&mut msg, "hue-bridge.local", TYPE_A, &[192, 168, 1, 30]);

        let found = parse_response(&msg, service, Ipv4Addr::new(10, 0, 0, 9));
        assert_eq!(found.len(), 1);
        let hit = &found[0];
        assert_eq!(hit.ip, "192.168.1.30", "the A record wins over the source");
        assert_eq!(hit.port, 443);
        assert_eq!(hit.name, "Hue Bridge - 1A2B3C");
        assert_eq!(hit.txt["bridgeid"], "001788fffe1a2b3c");
    }

    #[test]
    fn follows_compression_pointers() {
        // Header, then "_hue._tcp.local" at offset 12, then a pointer to it.
        let mut msg = vec![0u8; 12];
        msg.extend(name_bytes("_hue._tcp.local"));
        let ptr_at = msg.len();
        msg.extend([0xC0, 12]);
        let (name, next) = read_name(&msg, ptr_at).unwrap();
        assert_eq!(name, "_hue._tcp.local");
        assert_eq!(next, ptr_at + 2);
    }

    #[test]
    fn a_pointer_loop_is_rejected() {
        let mut msg = vec![0u8; 12];
        msg.extend([0xC0, 12]);
        assert!(read_name(&msg, 12).is_none());
    }

    /// Against the real network: what answers to the services in
    /// `LUEUR_MDNS_SERVICES` (comma-separated), or to a few common ones.
    #[test]
    #[ignore = "searches the real network"]
    fn browses_the_real_network() {
        let services = std::env::var("LUEUR_MDNS_SERVICES").unwrap_or_else(|_| {
            "_hue._tcp.local,_nanoleafapi._tcp.local,_googlecast._tcp.local,\
             _airplay._tcp.local,_spotify-connect._tcp.local,_ipp._tcp.local,\
             _printer._tcp.local,_hap._tcp.local,_sonos._tcp.local"
                .into()
        });
        println!("interfaces: {:?}", super::super::common::lan_interfaces());
        let mut any = false;
        for service in services.split(',') {
            let found = browse(service, Duration::from_millis(1500)).unwrap();
            for inst in &found {
                println!(
                    "{service}: {} at {}:{} {:?}",
                    inst.name,
                    inst.ip,
                    inst.port,
                    inst.txt.keys()
                );
            }
            any |= !found.is_empty();
        }
        assert!(any, "nothing answered");
    }

    #[test]
    fn queries_and_truncated_packets_yield_nothing() {
        let q = build_query("_hue._tcp.local");
        assert!(parse_response(&q, "_hue._tcp.local", Ipv4Addr::LOCALHOST).is_empty());
        assert!(parse_response(&[0, 0, 0x84], "_hue._tcp.local", Ipv4Addr::LOCALHOST).is_empty());
    }
}
