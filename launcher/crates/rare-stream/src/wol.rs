//! Wake-on-LAN, the same packets Moonlight's "Wake PC" sends (NvComputer::wake): the magic
//! packet to the broadcast address and the PC's last addresses, on the standard port 9, the
//! Moonlight Internet Hosting Tool's 47009 and the ports GameStream forwards.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

const PORTS: [u16; 7] = [9, 47009, 47998, 47999, 48000, 48002, 48010];

/// "a8:a1:59:12:34:56" or "A8-A1-59-12-34-56".
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.trim().split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        if p.len() != 2 {
            return None;
        }
        out[i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(out)
}

/// Six 0xFF bytes, then the MAC sixteen times.
pub fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut p = [0xFFu8; 102];
    for i in 0..16 {
        p[6 + i * 6..12 + i * 6].copy_from_slice(&mac);
    }
    p
}

/// Where the packet goes: the limited broadcast, then each known IPv4 address and its /24
/// broadcast (a sleeping PC's ARP entry may be gone), on every port.
pub fn targets(addresses: &[String]) -> Vec<SocketAddr> {
    let mut ips: Vec<Ipv4Addr> = vec![Ipv4Addr::BROADCAST];
    for a in addresses {
        let host = a.rsplit_once(':').filter(|(h, _)| !h.contains(':')).map(|(h, _)| h).unwrap_or(a);
        if let Ok(ip) = host.parse::<Ipv4Addr>() {
            let o = ip.octets();
            for x in [ip, Ipv4Addr::new(o[0], o[1], o[2], 255)] {
                if !ips.contains(&x) {
                    ips.push(x);
                }
            }
        }
    }
    ips.iter().flat_map(|ip| PORTS.iter().map(move |p| SocketAddr::from((*ip, *p)))).collect()
}

/// Sends the packets. Ok when at least one went out.
pub fn wake(mac: [u8; 6], addresses: &[String]) -> Result<(), String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.set_broadcast(true).map_err(|e| e.to_string())?;
    let p = magic_packet(mac);
    let sent = targets(addresses).iter().filter(|t| sock.send_to(&p, t).is_ok()).count();
    if sent == 0 {
        return Err("Could not send the wake packet".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs() {
        assert_eq!(parse_mac("A8:A1:59:12:34:56"), Some([0xa8, 0xa1, 0x59, 0x12, 0x34, 0x56]));
        assert_eq!(parse_mac("a8-a1-59-12-34-56"), Some([0xa8, 0xa1, 0x59, 0x12, 0x34, 0x56]));
        assert_eq!(parse_mac("a8:a1:59:12:34"), None);
        assert_eq!(parse_mac("zz:a1:59:12:34:56"), None);
    }

    #[test]
    fn packet_and_targets() {
        let m = [1, 2, 3, 4, 5, 6];
        let p = magic_packet(m);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert_eq!(&p[6..12], &m);
        assert_eq!(&p[96..102], &m);
        let t = targets(&["192.168.0.20".into(), "192.168.0.20:48989".into(), "fe80::2".into()]);
        // broadcast, the host, its /24 broadcast; seven ports each
        assert_eq!(t.len(), 3 * 7);
        assert!(t.contains(&"255.255.255.255:9".parse().unwrap()));
        assert!(t.contains(&"192.168.0.20:47009".parse().unwrap()));
        assert!(t.contains(&"192.168.0.255:48010".parse().unwrap()));
    }
}
