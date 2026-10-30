//! Sui's per-transaction limits and a planner that splits a long list into the fewest
//! transactions that each stay inside them (`tx/nft/limits.ts`). Pure: no network, never signs.
//! The size estimate is the TS one exactly, so both launchers batch a selection the same way.

use rare_sui::ptb::{Command, Input, Tx};
use serde::{Deserialize, Serialize};

use super::{build_err, NftSendError};

/// Protocol limits (mainnet protocol config), with room left for gas data and signatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// max_programmable_tx_commands
    pub commands: usize,
    /// max_arguments: a TransferObjects / SplitCoins / MoveCall takes at most this many
    pub args: usize,
    /// max_input_objects
    pub objects: usize,
    /// max_tx_size_bytes is 128 KiB; the rest is kept for gas coins, expiration and signatures
    pub bytes: usize,
}

pub const SUI_LIMITS: Limits = Limits { commands: 1024, args: 512, objects: 2048, bytes: 100 * 1024 };

/// Items per TransferObjects / SplitCoins command, under `SUI_LIMITS.args`.
pub const ARGS_PER_COMMAND: usize = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxSize {
    pub commands: usize,
    /// distinct object inputs
    pub objects: usize,
    /// estimated BCS size of inputs + commands
    pub bytes: usize,
}

fn uleb(n: usize) -> usize {
    if n < 0x80 {
        1
    } else if n < 0x4000 {
        2
    } else {
        3
    }
}

/// JS `s.length` (UTF-16 code units).
fn jslen(s: &str) -> usize {
    s.encode_utf16().count()
}

fn str_bytes(s: &str) -> usize {
    uleb(jslen(s)) + jslen(s)
}

/// JS `String.prototype.trim`: Unicode white space, line terminators and the BOM.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// A type argument as `parseStructTag` leaves it: a string (primitive or vector) or a struct.
enum Param {
    Str(String),
    Struct(Tag),
}

struct Tag {
    address: String,
    module: String,
    name: String,
    params: Vec<Param>,
}

/// `splitGenericParameters` from @mysten/bcs.
fn split_generic(s: &str) -> Vec<String> {
    let mut tok = Vec::new();
    let mut word = String::new();
    let mut depth: i32 = 0;
    for c in s.chars() {
        if c == '<' {
            depth += 1;
        }
        if c == '>' {
            depth -= 1;
        }
        if depth == 0 && c == ',' {
            tok.push(js_trim(&word).to_string());
            word.clear();
            continue;
        }
        word.push(c);
    }
    tok.push(js_trim(&word).to_string());
    tok
}

/// `parseStructTag` from @mysten/sui 2.38 (address normalisation does not change the size).
fn parse_struct_tag(t: &str) -> Option<Tag> {
    let parts: Vec<&str> = t.split("::").collect();
    if parts.len() < 3 || parts[0].is_empty() || parts[1].is_empty() {
        return None;
    }
    let (address, module) = (parts[0], parts[1]);
    let rest = &t[address.len() + module.len() + 4..];
    let lt = rest.find('<');
    let name = match lt {
        Some(i) => &rest[..i],
        None => rest,
    };
    if name.is_empty() || (lt.is_some() && !rest.ends_with('>')) {
        return None;
    }
    let params = match lt {
        Some(i) => {
            let inner = &rest[i + 1..rest.rfind('>').unwrap_or(rest.len())];
            split_generic(inner).iter().map(|p| parse_type_tag(js_trim(p))).collect::<Option<Vec<_>>>()?
        }
        None => vec![],
    };
    Some(Tag { address: address.to_string(), module: module.to_string(), name: name.to_string(), params })
}

/// `parseTypeTag`: vectors stay strings (`vector<normalized inner>`), structs parse.
fn parse_type_tag(t: &str) -> Option<Param> {
    if let Some(rest) = t.strip_prefix("vector<") {
        if !t.ends_with('>') {
            return None;
        }
        let inner = &rest[..rest.len().saturating_sub(1)];
        if inner.is_empty() {
            return None;
        }
        return Some(Param::Str(match parse_type_tag(inner)? {
            Param::Str(s) => format!("vector<{s}>"),
            Param::Struct(tag) => format!("vector<{}>", normalize(&tag)),
        }));
    }
    if !t.contains("::") {
        return Some(Param::Str(t.to_string()));
    }
    parse_struct_tag(t).map(Param::Struct)
}

/// `normalizeStructTag` of a parsed tag (only its shape matters for the size).
fn normalize(t: &Tag) -> String {
    let mut s = format!("{}::{}::{}", t.address, t.module, t.name);
    if !t.params.is_empty() {
        let p: Vec<String> = t
            .params
            .iter()
            .map(|p| match p {
                Param::Str(s) => s.clone(),
                Param::Struct(t) => normalize(t),
            })
            .collect();
        s.push('<');
        s.push_str(&p.join(","));
        s.push('>');
    }
    s
}

/// BCS size of a type tag written as a string (`0x2::sui::SUI`, `u64`, `vector<u8>`).
pub fn type_tag_bytes(t: &str) -> usize {
    let s = js_trim(t);
    if let Some(rest) = s.strip_prefix("vector<") {
        // s.slice(7, -1)
        let inner = rest.char_indices().last().map(|(i, _)| &rest[..i]).unwrap_or("");
        return 1 + type_tag_bytes(inner);
    }
    if !s.contains("::") {
        return 1;
    }
    match parse_struct_tag(s) {
        Some(tag) => {
            1 + 32
                + str_bytes(&tag.module)
                + str_bytes(&tag.name)
                + uleb(tag.params.len())
                + tag
                    .params
                    .iter()
                    .map(|p| match p {
                        Param::Str(s) => type_tag_bytes(s),
                        Param::Struct(t) => type_tag_bytes(&format!("{}::{}::{}", t.address, t.module, t.name)),
                    })
                    .sum::<usize>()
        }
        None => 1 + 32 + jslen(s),
    }
}

const ARG: usize = 3;
/// a coinWithBalance intent turns into a few commands at build time (merge + split, or a withdrawal)
const INTENT_COMMANDS: usize = 4;

/// Commands, object inputs and an estimated size for an unresolved transaction.
pub fn measure(tx: &Tx) -> TxSize {
    let mut bytes = 0;
    let mut objects = 0;
    for i in &tx.inputs {
        match i {
            Input::Pure(b) => bytes += 1 + 3 + b.len(),
            Input::Object(_) => {
                objects += 1;
                // owned object ref (id, version, digest); shared ones are smaller
                bytes += 1 + 1 + 32 + 8 + 33;
            }
            // reservation, type tag, source
            Input::Withdrawal { coin_type, .. } => bytes += 1 + 9 + 1 + type_tag_bytes(coin_type) + 1,
        }
    }
    let mut commands = 0;
    for c in &tx.commands {
        match c {
            Command::MoveCall(m) => {
                commands += 1;
                bytes += 1 + 32 + str_bytes(&m.module) + str_bytes(&m.function) + uleb(m.type_args.len()) + m.type_args.iter().map(|t| type_tag_bytes(t)).sum::<usize>() + uleb(m.args.len()) + m.args.len() * ARG;
            }
            Command::TransferObjects { objects, .. } => {
                commands += 1;
                bytes += 1 + uleb(objects.len()) + objects.len() * ARG + ARG;
            }
            Command::SplitCoins { amounts, .. } => {
                commands += 1;
                bytes += 1 + ARG + uleb(amounts.len()) + amounts.len() * ARG;
            }
            Command::MergeCoins { sources, .. } => {
                commands += 1;
                bytes += 1 + ARG + uleb(sources.len()) + sources.len() * ARG;
            }
            Command::CoinWithBalance { .. } => {
                commands += INTENT_COMMANDS;
                bytes += INTENT_COMMANDS * 120;
            }
            Command::MakeMoveVec { .. } => {
                commands += 1;
                bytes += 64;
            }
        }
    }
    TxSize { commands, objects, bytes }
}

pub fn fits(s: &TxSize, limits: &Limits) -> bool {
    s.commands <= limits.commands && s.objects <= limits.objects && s.bytes <= limits.bytes
}

/// Split `items` into the fewest consecutive batches whose transactions fit. `build` makes the
/// transaction for a slice; its size has to grow with the slice (true for every builder here).
/// Each batch is the longest prefix that fits, found by binary search, which gives the minimum
/// count for this order. Fails when a single item does not fit on its own.
pub fn plan_batches<T: Clone>(items: &[T], build: impl Fn(&[T]) -> Result<Tx, NftSendError>, limits: &Limits) -> Result<Vec<Vec<T>>, NftSendError> {
    let mut out = Vec::new();
    let ok = |from: usize, n: usize| -> Result<bool, NftSendError> { Ok(fits(&measure(&build(&items[from..(from + n).min(items.len())])?), limits)) };
    let mut at = 0;
    while at < items.len() {
        let left = items.len() - at;
        if ok(at, left)? {
            out.push(items[at..].to_vec());
            break;
        }
        if !ok(at, 1)? {
            return Err(build_err("One item is too big for a single transaction"));
        }
        // largest n in [1, left) that fits
        let (mut lo, mut hi) = (1, left - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if ok(at, mid)? {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        out.push(items[at..at + lo].to_vec());
        at += lo;
    }
    Ok(out)
}

/// Split a slice into chunks of at most `n`.
pub fn chunk<T: Clone>(a: &[T], n: usize) -> Vec<Vec<T>> {
    a.chunks(n).map(<[T]>::to_vec).collect()
}
