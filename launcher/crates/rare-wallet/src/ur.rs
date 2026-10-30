//! Uniform Resources (BCR-2020-005) for the Keystone: the small CBOR subset the registry types
//! use, Bytewords (minimal style, CRC-32), single-part URs, and multi-part URs that fit in one
//! fragment. Byte-identical to @ngraveio/bc-ur 1.1.13 and @keystonehq/bc-ur-registry's encoder
//! (map keys in ascending order, shortest integer forms).

// ---------------------------------------------------------------- CBOR

#[derive(Debug, Clone, PartialEq)]
pub enum Cbor {
    Uint(u64),
    /// -1 - n
    Neg(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Cbor>),
    /// integer-keyed maps, kept in the order given (the encoder sorts ascending)
    Map(Vec<(u64, Cbor)>),
    Tag(u64, Box<Cbor>),
    Bool(bool),
    Null,
}

impl Cbor {
    pub fn tag(tag: u64, v: Cbor) -> Cbor {
        Cbor::Tag(tag, Box::new(v))
    }
    /// The value inside any tags.
    pub fn untag(&self) -> &Cbor {
        match self {
            Cbor::Tag(_, v) => v.untag(),
            v => v,
        }
    }
    pub fn get(&self, key: u64) -> Option<&Cbor> {
        match self.untag() {
            Cbor::Map(m) => m.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self.untag() {
            Cbor::Uint(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self.untag() {
            Cbor::Bytes(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_text(&self) -> Option<&str> {
        match self.untag() {
            Cbor::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Cbor]> {
        match self.untag() {
            Cbor::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self.untag() {
            Cbor::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn tag_number(&self) -> Option<u64> {
        match self {
            Cbor::Tag(t, _) => Some(*t),
            _ => None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    fn head(out: &mut Vec<u8>, major: u8, n: u64) {
        let m = major << 5;
        if n < 24 {
            out.push(m | n as u8);
        } else if n <= 0xff {
            out.extend_from_slice(&[m | 24, n as u8]);
        } else if n <= 0xffff {
            out.push(m | 25);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        } else if n <= 0xffff_ffff {
            out.push(m | 26);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        } else {
            out.push(m | 27);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Cbor::Uint(n) => Self::head(out, 0, *n),
            Cbor::Neg(n) => Self::head(out, 1, *n),
            Cbor::Bytes(b) => {
                Self::head(out, 2, b.len() as u64);
                out.extend_from_slice(b);
            }
            Cbor::Text(t) => {
                Self::head(out, 3, t.len() as u64);
                out.extend_from_slice(t.as_bytes());
            }
            Cbor::Array(a) => {
                Self::head(out, 4, a.len() as u64);
                a.iter().for_each(|v| v.write(out));
            }
            Cbor::Map(m) => {
                let mut sorted: Vec<&(u64, Cbor)> = m.iter().collect();
                sorted.sort_by_key(|(k, _)| *k);
                Self::head(out, 5, sorted.len() as u64);
                for (k, v) in sorted {
                    Self::head(out, 0, *k);
                    v.write(out);
                }
            }
            Cbor::Tag(t, v) => {
                Self::head(out, 6, *t);
                v.write(out);
            }
            Cbor::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
            Cbor::Null => out.push(0xf6),
        }
    }

    pub fn decode(b: &[u8]) -> Option<Cbor> {
        let mut i = 0;
        let v = Self::read(b, &mut i, 0)?;
        (i == b.len()).then_some(v)
    }

    fn read(b: &[u8], i: &mut usize, depth: u32) -> Option<Cbor> {
        if depth > 32 {
            return None;
        }
        let first = *b.get(*i)?;
        *i += 1;
        let (major, info) = (first >> 5, first & 0x1f);
        let n = match info {
            0..=23 => info as u64,
            24 => {
                *i += 1;
                *b.get(*i - 1)? as u64
            }
            25 => {
                *i += 2;
                u16::from_be_bytes(b.get(*i - 2..*i)?.try_into().ok()?) as u64
            }
            26 => {
                *i += 4;
                u32::from_be_bytes(b.get(*i - 4..*i)?.try_into().ok()?) as u64
            }
            27 => {
                *i += 8;
                u64::from_be_bytes(b.get(*i - 8..*i)?.try_into().ok()?)
            }
            _ => return None,
        };
        let take = |i: &mut usize, n: u64| -> Option<Vec<u8>> {
            let n = usize::try_from(n).ok()?;
            let s = b.get(*i..i.checked_add(n)?)?.to_vec();
            *i += n;
            Some(s)
        };
        Some(match major {
            0 => Cbor::Uint(n),
            1 => Cbor::Neg(n),
            2 => Cbor::Bytes(take(i, n)?),
            3 => Cbor::Text(String::from_utf8(take(i, n)?).ok()?),
            4 => {
                let mut a = Vec::new();
                for _ in 0..n.min(4096) {
                    a.push(Self::read(b, i, depth + 1)?);
                }
                if n > 4096 {
                    return None;
                }
                Cbor::Array(a)
            }
            5 => {
                if n > 4096 {
                    return None;
                }
                let mut m = Vec::new();
                for _ in 0..n {
                    let k = match Self::read(b, i, depth + 1)? {
                        Cbor::Uint(k) => k,
                        _ => return None,
                    };
                    m.push((k, Self::read(b, i, depth + 1)?));
                }
                Cbor::Map(m)
            }
            6 => Cbor::Tag(n, Box::new(Self::read(b, i, depth + 1)?)),
            7 => match info {
                20 => Cbor::Bool(false),
                21 => Cbor::Bool(true),
                22 => Cbor::Null,
                _ => return None,
            },
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------- Bytewords

const WORDS: &str = "ableacidalsoapexaquaarchatomauntawayaxisbackbaldbarnbeltbetabiasbluebodybragbrewbulbbuzzcalmcashcatschefcityclawcodecolacookcostcruxcurlcuspcyandarkdatadaysdelidicedietdoordowndrawdropdrumdulldutyeacheasyechoedgeepicevenexamexiteyesfactfairfernfigsfilmfishfizzflapflewfluxfoxyfreefrogfuelfundgalagamegeargemsgiftgirlglowgoodgraygrimgurugushgyrohalfhanghardhawkheathelphighhillholyhopehornhutsicedideaidleinchinkyintoirisironitemjadejazzjoinjoltjowljudojugsjumpjunkjurykeepkenokeptkeyskickkilnkingkitekiwiknoblamblavalazyleaflegsliarlimplionlistlogoloudloveluaulucklungmainmanymathmazememomenumeowmildmintmissmonknailnavyneednewsnextnoonnotenumbobeyoboeomitonyxopenovalowlspaidpartpeckplaypluspoempoolposepuffpumapurrquadquizraceramprealredorichroadrockroofrubyruinrunsrustsafesagascarsetssilkskewslotsoapsolosongstubsurfswantacotasktaxitenttiedtimetinytoiltombtoystriptunatwinuglyundouniturgeuservastveryvetovialvibeviewvisavoidvowswallwandwarmwaspwavewaxywebswhatwhenwhizwolfworkyankyawnyellyogayurtzapszerozestzinczonezoom";

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Minimal Bytewords (first and last letter of each word) of `data` || CRC-32.
pub fn bytewords_encode(data: &[u8]) -> String {
    let w = WORDS.as_bytes();
    let mut all = data.to_vec();
    all.extend_from_slice(&crc32(data).to_be_bytes());
    all.iter().flat_map(|&b| [w[b as usize * 4] as char, w[b as usize * 4 + 3] as char]).collect()
}

pub fn bytewords_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    if s.len() % 2 != 0 || s.len() < 10 {
        return None;
    }
    let w = WORDS.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.chunks(2) {
        let (a, z) = (pair[0].to_ascii_lowercase(), pair[1].to_ascii_lowercase());
        out.push((0..256).find(|&i| w[i * 4] == a && w[i * 4 + 3] == z)? as u8);
    }
    let (body, crc) = out.split_at(out.len() - 4);
    (crc32(body).to_be_bytes() == crc).then(|| body.to_vec())
}

// ---------------------------------------------------------------- URs

#[derive(Debug, Clone, PartialEq)]
pub struct Ur {
    pub kind: String,
    pub cbor: Vec<u8>,
}

/// "UR:TYPE/BYTEWORDS", uppercase like the TS encoder sends it.
pub fn encode_ur(kind: &str, cbor: &[u8]) -> String {
    format!("ur:{kind}/{}", bytewords_encode(cbor)).to_uppercase()
}

/// A single-part UR, or a multi-part one whose message fits in its one fragment.
pub fn decode_ur(text: &str) -> Option<Ur> {
    let s = text.trim().to_ascii_lowercase();
    let rest = s.strip_prefix("ur:")?;
    let parts: Vec<&str> = rest.split('/').collect();
    let kind = *parts.first()?;
    if kind.is_empty() || !kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return None;
    }
    match parts.len() {
        2 => Some(Ur { kind: kind.into(), cbor: bytewords_decode(parts[1])? }),
        3 => {
            let (n, len) = parts[1].split_once('-')?;
            let (n, len): (u64, u64) = (n.parse().ok()?, len.parse().ok()?);
            let part = Cbor::decode(&bytewords_decode(parts[2])?)?;
            let a = part.as_array()?;
            let (seq, seq_len, msg_len, checksum, fragment) = (a.first()?.as_u64()?, a.get(1)?.as_u64()?, a.get(2)?.as_u64()? as usize, a.get(3)?.as_u64()?, a.get(4)?.as_bytes()?);
            if seq != n || seq_len != len || seq_len != 1 || fragment.len() < msg_len {
                return None;
            }
            let msg = fragment[..msg_len].to_vec();
            (crc32(&msg) as u64 == checksum).then(|| Ur { kind: kind.into(), cbor: msg })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"Hello, world!"), 0xebe6c6e6);
        assert_eq!(crc32(b"Wolf"), 0x598c84dc);
    }

    #[test]
    fn bytewords_round_trip_and_checksum() {
        // BCR-2020-012 test vector
        let data = [0, 1, 2, 128, 255];
        assert_eq!(bytewords_encode(&data), "aeadaolazmjendeoti");
        assert_eq!(bytewords_decode("aeadaolazmjendeoti").unwrap(), data);
        assert_eq!(bytewords_decode("AEADAOLAZMJENDEOTI").unwrap(), data);
        assert!(bytewords_decode("aeadaolazmjendeotj").is_none());
        assert!(bytewords_decode("ae").is_none());
    }

    #[test]
    fn cbor_round_trip_with_sorted_maps_and_short_ints() {
        let v = Cbor::Map(vec![(3, Cbor::Text("x".into())), (1, Cbor::Array(vec![Cbor::Uint(44), Cbor::Bool(true), Cbor::Uint(784)])), (2, Cbor::tag(37, Cbor::Bytes(vec![1; 16])))]);
        let b = v.encode();
        assert_eq!(b[0], 0xa3);
        assert_eq!(b[1], 0x01); // key 1 first
        let back = Cbor::decode(&b).unwrap();
        assert_eq!(back.get(2).unwrap().tag_number(), Some(37));
        assert_eq!(back.get(3).unwrap().as_text(), Some("x"));
        assert_eq!(Cbor::Uint(0xffff_ffff).encode(), vec![0x1a, 0xff, 0xff, 0xff, 0xff]);
        assert!(Cbor::decode(&[0x5f]).is_none());
        assert!(Cbor::decode(&[0x41]).is_none());
    }

    #[test]
    fn urs_single_and_one_fragment_multi_part() {
        let cbor = Cbor::Bytes(vec![7; 20]).encode();
        let text = encode_ur("bytes", &cbor);
        assert!(text.starts_with("UR:BYTES/"));
        assert_eq!(decode_ur(&text).unwrap(), Ur { kind: "bytes".into(), cbor: cbor.clone() });
        let part = Cbor::Array(vec![Cbor::Uint(1), Cbor::Uint(1), Cbor::Uint(cbor.len() as u64), Cbor::Uint(crc32(&cbor) as u64), Cbor::Bytes(cbor.clone())]).encode();
        let multi = format!("ur:bytes/1-1/{}", bytewords_encode(&part));
        assert_eq!(decode_ur(&multi).unwrap().cbor, cbor);
        assert!(decode_ur(&format!("ur:bytes/1-3/{}", bytewords_encode(&part))).is_none());
        assert!(decode_ur("hello").is_none());
        assert!(decode_ur("ur:Bad Type/aeadaolazmjendeoti").is_none());
    }
}
