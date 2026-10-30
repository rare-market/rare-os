//! Ledger APDU framing, ported from @ledgerhq/devices (hid-framing makeBlocks/reduceResponse and
//! ble/sendAPDU + ble/receiveAPDU). Pure functions; the test vectors in testdata/ were produced by
//! running Ledger's own JS (see the test module).

/// USB HID report size.
pub const HID_PACKET: usize = 64;
/// HID channel Ledger uses for APDUs.
pub const HID_CHANNEL: u16 = 0x0101;
/// Frame tag for APDU data (HID and BLE).
pub const TAG_APDU: u8 = 0x05;
/// BLE MTU query, answered with a 0x08 frame whose byte 5 is the usable frame size.
pub const MTU_REQUEST: [u8; 5] = [0x08, 0x00, 0x00, 0x00, 0x00];
/// Frame size before (or without) an MTU answer: 23-byte ATT MTU minus the 3-byte ATT header.
pub const BLE_DEFAULT_FRAME: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    Short,
    Channel,
    Tag,
    Sequence,
    TooMuchData,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FrameError::Short => "frame too short",
            FrameError::Channel => "invalid channel",
            FrameError::Tag => "invalid tag",
            FrameError::Sequence => "invalid sequence",
            FrameError::TooMuchData => "too much data",
        };
        f.write_str(s)
    }
}

/// Splits an APDU into 64-byte HID frames: channel(2) tag(1) seq(2), the first frame then carries
/// the APDU length (2), the rest is APDU bytes, zero padded.
pub fn hid_frames(apdu: &[u8]) -> Vec<Vec<u8>> {
    let block = HID_PACKET - 5;
    let mut data = Vec::with_capacity(apdu.len() + 2);
    data.extend_from_slice(&(apdu.len() as u16).to_be_bytes());
    data.extend_from_slice(apdu);
    let n = data.len().div_ceil(block);
    data.resize(n * block, 0);
    data.chunks(block)
        .enumerate()
        .map(|(i, chunk)| {
            let mut f = Vec::with_capacity(HID_PACKET);
            f.extend_from_slice(&HID_CHANNEL.to_be_bytes());
            f.push(TAG_APDU);
            f.extend_from_slice(&(i as u16).to_be_bytes());
            f.extend_from_slice(chunk);
            f
        })
        .collect()
}

/// Splits an APDU into BLE frames of at most `frame` bytes: tag(1) seq(2), the first frame then
/// carries the APDU length (2). No padding.
pub fn ble_frames(apdu: &[u8], frame: usize) -> Vec<Vec<u8>> {
    let frame = frame.max(6);
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut seq: u16 = 0;
    loop {
        let head = if seq == 0 { 5 } else { 3 };
        let end = (i + frame - head).min(apdu.len());
        let mut f = Vec::with_capacity(end - i + head);
        f.push(TAG_APDU);
        f.extend_from_slice(&seq.to_be_bytes());
        if seq == 0 {
            f.extend_from_slice(&(apdu.len() as u16).to_be_bytes());
        }
        f.extend_from_slice(&apdu[i..end]);
        out.push(f);
        i = end;
        seq += 1;
        if i >= apdu.len() {
            return out;
        }
    }
}

/// The usable BLE frame size from an MTU answer, as @ledgerhq/hw-transport-web-ble infers it.
pub fn parse_mtu_reply(frame: &[u8]) -> Option<usize> {
    if frame.first() != Some(&0x08) || frame.len() < 6 {
        return None;
    }
    let size = frame[5] as usize;
    Some(if size > BLE_DEFAULT_FRAME { size } else { BLE_DEFAULT_FRAME })
}

/// Reassembles one response from HID or BLE frames.
#[derive(Debug)]
pub struct Reassembler {
    hid: bool,
    data: Vec<u8>,
    len: usize,
    seq: u16,
}

impl Reassembler {
    pub fn hid() -> Self {
        Self { hid: true, data: Vec::new(), len: 0, seq: 0 }
    }

    pub fn ble() -> Self {
        Self { hid: false, data: Vec::new(), len: 0, seq: 0 }
    }

    /// True before the first frame of a response arrived.
    pub fn is_empty(&self) -> bool {
        self.seq == 0
    }

    /// Feeds one frame; returns the whole response once complete.
    pub fn push(&mut self, frame: &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
        let first = self.seq == 0;
        let (head, tag_at) = if self.hid { (5, 2) } else { (3, 0) };
        let need = head + if first { 2 } else { 0 };
        if frame.len() < need {
            return Err(FrameError::Short);
        }
        if self.hid && u16::from_be_bytes([frame[0], frame[1]]) != HID_CHANNEL {
            return Err(FrameError::Channel);
        }
        if frame[tag_at] != TAG_APDU {
            return Err(FrameError::Tag);
        }
        if u16::from_be_bytes([frame[head - 2], frame[head - 1]]) != self.seq {
            return Err(FrameError::Sequence);
        }
        if first {
            self.len = u16::from_be_bytes([frame[head], frame[head + 1]]) as usize;
        }
        self.seq = self.seq.wrapping_add(1);
        self.data.extend_from_slice(&frame[need..]);
        if self.data.len() > self.len {
            if self.hid {
                // HID frames are zero padded.
                self.data.truncate(self.len);
            } else {
                return Err(FrameError::TooMuchData);
            }
        }
        if self.data.len() == self.len {
            return Ok(Some(std::mem::take(&mut self.data)));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Produced by Ledger's JS: hid-framing.js (channel 0x0101, 64-byte packets) and
    /// ble/sendAPDU.js at frame sizes 20, 23, 128 and 153, each round-tripped through Ledger's
    /// reduceResponse / receiveAPDU before being written.
    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!("testdata/ledger-vectors.json")).unwrap()
    }

    #[test]
    fn hid_frames_match_ledger_js() {
        let v = vectors();
        let cases = v["hid"].as_array().unwrap();
        assert!(cases.len() >= 4);
        for c in cases {
            let apdu = unhex(c["apdu"].as_str().unwrap());
            let want: Vec<Vec<u8>> = c["frames"].as_array().unwrap().iter().map(|f| unhex(f.as_str().unwrap())).collect();
            assert_eq!(hid_frames(&apdu), want, "apdu len {}", apdu.len());
        }
    }

    #[test]
    fn hid_reassembles_ledger_js_frames() {
        let v = vectors();
        for c in v["hid"].as_array().unwrap() {
            let apdu = unhex(c["apdu"].as_str().unwrap());
            let mut r = Reassembler::hid();
            let frames = c["frames"].as_array().unwrap();
            let mut got = None;
            for (i, f) in frames.iter().enumerate() {
                let out = r.push(&unhex(f.as_str().unwrap())).unwrap();
                if i + 1 < frames.len() {
                    assert!(out.is_none());
                }
                got = out;
            }
            assert_eq!(got.unwrap(), apdu);
        }
    }

    #[test]
    fn ble_frames_match_ledger_js() {
        let v = vectors();
        let cases = v["ble"].as_array().unwrap();
        assert!(cases.len() >= 16);
        let mut multi = 0;
        for c in cases {
            let mtu = c["mtu"].as_u64().unwrap() as usize;
            let apdu = unhex(c["apdu"].as_str().unwrap());
            let want: Vec<Vec<u8>> = c["frames"].as_array().unwrap().iter().map(|f| unhex(f.as_str().unwrap())).collect();
            if want.len() > 1 {
                multi += 1;
            }
            assert_eq!(ble_frames(&apdu, mtu), want, "mtu {mtu} apdu len {}", apdu.len());
            let mut r = Reassembler::ble();
            let mut got = None;
            for f in &want {
                got = r.push(f).unwrap();
            }
            assert_eq!(got.unwrap(), apdu);
        }
        assert!(multi >= 6);
    }

    #[test]
    fn hid_round_trip_all_sizes() {
        for n in [0usize, 1, 5, 57, 58, 59, 60, 116, 117, 118, 255, 260, 1000] {
            let apdu: Vec<u8> = (0..n).map(|i| (i * 31 + 7) as u8).collect();
            let frames = hid_frames(&apdu);
            assert!(frames.iter().all(|f| f.len() == HID_PACKET));
            let mut r = Reassembler::hid();
            let mut got = None;
            for f in &frames {
                assert!(got.is_none());
                got = r.push(f).unwrap();
            }
            assert_eq!(got.unwrap(), apdu, "n={n}");
        }
    }

    #[test]
    fn ble_round_trip_all_sizes() {
        for mtu in [20usize, 23, 64, 153, 244] {
            for n in [1usize, 2, 15, 16, 17, 100, 255, 600] {
                let apdu: Vec<u8> = (0..n).map(|i| (i * 13 + 1) as u8).collect();
                let frames = ble_frames(&apdu, mtu);
                assert!(frames.iter().all(|f| f.len() <= mtu));
                let mut r = Reassembler::ble();
                let mut got = None;
                for f in &frames {
                    got = r.push(f).unwrap();
                }
                assert_eq!(got.unwrap(), apdu, "mtu={mtu} n={n}");
            }
        }
    }

    #[test]
    fn rejects_bad_frames() {
        let mut f = hid_frames(&[0xe0, 1, 0, 0, 0]).remove(0);
        f[0] = 0x02;
        assert_eq!(Reassembler::hid().push(&f), Err(FrameError::Channel));
        let mut f = hid_frames(&[0xe0, 1, 0, 0, 0]).remove(0);
        f[2] = 0x02;
        assert_eq!(Reassembler::hid().push(&f), Err(FrameError::Tag));
        let mut f = hid_frames(&[0xe0, 1, 0, 0, 0]).remove(0);
        f[4] = 1;
        assert_eq!(Reassembler::hid().push(&f), Err(FrameError::Sequence));
        assert_eq!(Reassembler::hid().push(&[1, 1, 5]), Err(FrameError::Short));
        // BLE: declared 2 bytes, sent 3.
        assert_eq!(Reassembler::ble().push(&[5, 0, 0, 0, 2, 1, 2, 3]), Err(FrameError::TooMuchData));
        let frames = ble_frames(&[9u8; 40], 20);
        let mut r = Reassembler::ble();
        r.push(&frames[0]).unwrap();
        assert_eq!(r.push(&frames[2]), Err(FrameError::Sequence));
    }

    #[test]
    fn mtu_reply() {
        assert_eq!(parse_mtu_reply(&[0x08, 0, 0, 0, 1, 153]), Some(153));
        assert_eq!(parse_mtu_reply(&[0x08, 0, 0, 0, 1, 10]), Some(BLE_DEFAULT_FRAME));
        assert_eq!(parse_mtu_reply(&[0x05, 0, 0, 0, 1, 153]), None);
        assert_eq!(parse_mtu_reply(&[0x08, 0]), None);
    }
}
