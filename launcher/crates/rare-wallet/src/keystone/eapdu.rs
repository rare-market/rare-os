//! Keystone 3 EAPDU framing, ported from @keystonehq/hw-transport-usb (frame.js) and checked
//! against the firmware (src/webusb_protocol/general/eapdu_protocol_parser.{c,h}).
//!
//! Every packet: cla(1)=0, ins(2)=command, p1(2)=total packets, p2(2)=index, lc(2)=request id,
//! then data. Requests carry up to 55 data bytes per packet; responses end with a 2-byte status.

use std::collections::BTreeMap;

pub const HEADER: usize = 9;
pub const MAX_DATA: usize = 55;
/// @keystonehq MAXUSBPackets: the firmware will not take more.
pub const MAX_PACKETS: usize = 200;

/// Firmware StatusEnum values (eapdu_protocol_parser.h), listed for reference.
#[allow(dead_code)]
pub mod status {
    pub const SUCCESS: u16 = 0;
    pub const FAILURE: u16 = 1;
    pub const PARSING_REJECTED: u16 = 4;
    pub const PARSING_ERROR: u16 = 5;
    pub const PARSING_DISALLOWED: u16 = 6;
    pub const MISMATCHED_WALLET: u16 = 8;
    pub const VERIFY_PASSWORD_ERROR: u16 = 9;
    pub const EXPORT_UNSUPPORTED_CHAIN: u16 = 10;
    pub const EXPORT_DISALLOWED: u16 = 13;
    pub const EXPORT_REJECTED: u16 = 14;
    pub const EXPORT_BUSY: u16 = 15;
}

/// Request packets. Empty data still makes one header-only packet.
pub fn encode(command: u16, request_id: u16, data: &[u8]) -> Vec<Vec<u8>> {
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[]] } else { data.chunks(MAX_DATA).collect() };
    let total = chunks.len() as u16;
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let mut p = Vec::with_capacity(HEADER + c.len());
            p.push(0);
            p.extend_from_slice(&command.to_be_bytes());
            p.extend_from_slice(&total.to_be_bytes());
            p.extend_from_slice(&(i as u16).to_be_bytes());
            p.extend_from_slice(&request_id.to_be_bytes());
            p.extend_from_slice(c);
            p
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub command: u16,
    pub total: u16,
    pub index: u16,
    pub request_id: u16,
    pub data: Vec<u8>,
    pub status: u16,
}

/// One response packet (header, data, status). None if too short.
pub fn parse(p: &[u8]) -> Option<Packet> {
    if p.len() < HEADER + 2 {
        return None;
    }
    let u16_at = |i: usize| u16::from_be_bytes([p[i], p[i + 1]]);
    let end = p.len() - 2;
    Some(Packet { command: u16_at(1), total: u16_at(3), index: u16_at(5), request_id: u16_at(7), data: p[HEADER..end].to_vec(), status: u16_at(end) })
}

/// Collects the packets answering one request; others (stale requests) are ignored.
pub struct Collector {
    command: u16,
    request_id: u16,
    total: u16,
    parts: BTreeMap<u16, Vec<u8>>,
    status: u16,
}

impl Collector {
    pub fn new(command: u16, request_id: u16) -> Self {
        Self { command, request_id, total: 0, parts: BTreeMap::new(), status: 0 }
    }

    /// Returns (status, data) once every packet arrived.
    pub fn push(&mut self, raw: &[u8]) -> Option<(u16, Vec<u8>)> {
        let p = parse(raw)?;
        if p.command != self.command || p.request_id != self.request_id || p.total == 0 || p.index >= p.total {
            return None;
        }
        self.total = p.total;
        self.status = p.status;
        self.parts.insert(p.index, p.data);
        if self.parts.len() < self.total as usize {
            return None;
        }
        let data = std::mem::take(&mut self.parts).into_values().flatten().collect();
        Some((self.status, data))
    }
}

/// Maps a failed response to a "code|message" error. `payload` is the JSON "payload" text.
pub fn status_error(status: u16, payload: Option<&str>) -> String {
    use crate::err;
    let msg = payload.unwrap_or("").trim();
    let lower = msg.to_lowercase();
    match status {
        status::PARSING_REJECTED | status::EXPORT_REJECTED => err("rejected", "Rejected on your Keystone"),
        status::VERIFY_PASSWORD_ERROR => err("rejected", "Wrong password on your Keystone"),
        status::PARSING_DISALLOWED | status::EXPORT_DISALLOWED if lower.contains("locked") => err("locked", "Unlock your Keystone"),
        status::PARSING_DISALLOWED | status::EXPORT_DISALLOWED if lower.contains("previous request") || lower.contains("waiting for user") => {
            err("busy", "Finish or reject the request on your Keystone first")
        }
        status::PARSING_DISALLOWED | status::EXPORT_DISALLOWED => {
            err("usb-off", "Open the Keystone home screen and turn on USB in Settings > Connection")
        }
        status::EXPORT_BUSY => err("busy", "Finish or reject the request on your Keystone first"),
        status::MISMATCHED_WALLET => err("mismatch", "This Keystone holds a different wallet"),
        status::EXPORT_UNSUPPORTED_CHAIN => err("unsupported", "Your Keystone does not support this yet. Update its firmware"),
        _ if !msg.is_empty() => err("error", format!("Keystone: {msg}")),
        _ => err("error", format!("Keystone error {status}")),
    }
}

/// The "payload" string of an error JSON, if any.
pub fn payload_text(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    match &v["payload"] {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Firmware-style response packets (SendEApduResponse): data split in 53-byte chunks,
    /// every packet ends with the status.
    pub fn respond(command: u16, request_id: u16, status: u16, data: &[u8]) -> Vec<Vec<u8>> {
        let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[]] } else { data.chunks(64 - HEADER - 2).collect() };
        let total = chunks.len() as u16;
        chunks
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let mut p = encode(command, request_id, &[0]).remove(0);
                p.truncate(HEADER);
                p[3..5].copy_from_slice(&total.to_be_bytes());
                p[5..7].copy_from_slice(&(i as u16).to_be_bytes());
                p.extend_from_slice(c);
                p.extend_from_slice(&status.to_be_bytes());
                p
            })
            .collect()
    }

    /// Produced by @keystonehq/hw-transport-usb 0.2.0 frame.js `encode`.
    #[test]
    fn encode_matches_keystone_js() {
        let v: serde_json::Value = serde_json::from_str(include_str!("testdata/eapdu-vectors.json")).unwrap();
        let mut multi = false;
        for c in v.as_array().unwrap() {
            let cmd = c["command"].as_u64().unwrap() as u16;
            let rid = c["requestId"].as_u64().unwrap() as u16;
            let data = c["data"].as_str().unwrap();
            let want: Vec<Vec<u8>> = c["packets"].as_array().unwrap().iter().map(|p| unhex(p.as_str().unwrap())).collect();
            multi |= want.len() > 1;
            assert_eq!(encode(cmd, rid, data.as_bytes()), want, "command {cmd}");
        }
        assert!(multi);
    }

    #[test]
    fn encode_sizes() {
        let data = vec![b'a'; 55 * 3 + 1];
        let p = encode(2, 9, &data);
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|x| x.len() <= 64));
        assert_eq!(p[3].len(), HEADER + 1);
        assert_eq!(&p[2][1..9], &[0, 2, 0, 4, 0, 2, 0, 9]);
    }

    #[test]
    fn decode_multi_packet_out_of_order() {
        let body = format!("{{\"payload\":\"{}\"}}", "UR:SUI-SIGNATURE/".to_string() + &"X".repeat(150));
        let mut packets = respond(2, 0x4242, 0, body.as_bytes());
        assert!(packets.len() >= 3);
        packets.swap(0, 2);
        let mut c = Collector::new(2, 0x4242);
        // Stale answer to another request and a different command are ignored.
        assert!(c.push(&respond(2, 0x1111, 0, b"{}")[0]).is_none());
        assert!(c.push(&respond(3, 0x4242, 0, b"{}")[0]).is_none());
        assert!(c.push(&[0, 1]).is_none());
        let mut out = None;
        for p in &packets {
            out = c.push(p);
        }
        let (st, data) = out.unwrap();
        assert_eq!(st, 0);
        assert_eq!(String::from_utf8(data).unwrap(), body);
    }

    #[test]
    fn decode_status_and_empty_body() {
        let p = respond(5, 1, 6, b"");
        assert_eq!(p[0].len(), HEADER + 2);
        let mut c = Collector::new(5, 1);
        assert_eq!(c.push(&p[0]), Some((6, vec![])));
        let pk = parse(&respond(1, 2, 0x0102, b"hi")[0]).unwrap();
        assert_eq!(pk, Packet { command: 1, total: 1, index: 0, request_id: 2, data: b"hi".to_vec(), status: 0x0102 });
    }

    #[test]
    fn status_mapping() {
        let code = |s: u16, m: &str| status_error(s, Some(m)).split('|').next().unwrap().to_string();
        assert_eq!(code(4, "rejected"), "rejected");
        assert_eq!(code(14, ""), "rejected");
        assert_eq!(code(6, "Device is locked"), "locked");
        assert_eq!(code(6, "Export address is just allowed on specific pages"), "usb-off");
        assert_eq!(code(13, ""), "usb-off");
        assert_eq!(code(6, "Previous request is not finished"), "busy");
        assert_eq!(code(6, "Waiting for user approval"), "busy");
        assert_eq!(code(15, ""), "busy");
        assert_eq!(code(8, ""), "mismatch");
        assert_eq!(code(5, "Transaction check failed"), "error");
        assert!(status_error(5, Some("Transaction check failed")).ends_with("Transaction check failed"));
        assert_eq!(code(1, ""), "error");
    }

    #[test]
    fn payload_text_variants() {
        assert_eq!(payload_text(r#"{"payload":"Device is locked"}"#).as_deref(), Some("Device is locked"));
        assert_eq!(payload_text(r#"{"payload":true}"#).as_deref(), Some("true"));
        assert_eq!(payload_text("not json"), None);
    }
}
