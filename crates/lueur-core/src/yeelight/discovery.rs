//! SSDP discovery of Yeelight bulbs on the local network.
//!
//! We send an `M-SEARCH` to the Yeelight multicast group and collect the unicast
//! replies for a short window. This only ever touches the LAN multicast address;
//! there is no outbound internet traffic, which keeps the app's network
//! footprint (and attack surface) minimal.

use super::protocol::{self, DiscoveredDevice, SSDP_ADDR, SSDP_PORT};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// Discover bulbs, listening for replies for `timeout`.
///
/// Deduplicates by device id, so a bulb answering twice appears once. Returns an
/// error only if the socket itself cannot be created/bound; individual malformed
/// replies are simply skipped.
pub fn discover(timeout: Duration) -> std::io::Result<Vec<DiscoveredDevice>> {
    // Bind to an ephemeral port on all interfaces.
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    // Allow us to also see multicast NOTIFYs if any arrive.
    socket.set_multicast_loop_v4(true).ok();

    let msg = protocol::ssdp_search_message();
    let target = SocketAddrV4::new(SSDP_ADDR.parse().unwrap(), SSDP_PORT);

    // Send the search a few times; UDP is lossy and bulbs can be slow to wake.
    for _ in 0..3 {
        let _ = socket.send_to(msg.as_bytes(), target);
    }

    let mut found: HashMap<String, DiscoveredDevice> = HashMap::new();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, _addr)) => {
                if let Ok(text) = std::str::from_utf8(&buf[..n])
                    && let Some(dev) = protocol::parse_discovery(text)
                {
                    found.insert(dev.id.clone(), dev);
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // No datagram this interval; keep polling until the deadline.
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    let mut devices: Vec<_> = found.into_values().collect();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(devices)
}
