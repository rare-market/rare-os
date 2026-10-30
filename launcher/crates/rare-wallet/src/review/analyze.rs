//! Classify a simulation from the signer's point of view (ports `review/analyze.ts`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::packages::{humanize, is_coin_object, is_framework_prefixed, known_package, norm_id, object_label, short_id, Label};
use super::types::{IdOperation, Owner, OutputKind, SimFailure, SimSource, SimStatus, Simulation};

/// `0x2::sui::SUI` normalized.
pub const SUI_TYPE: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";

// ---------- normalizeStructTag (lenient, like @mysten/sui) ----------

enum TypeTagP {
    Plain(String),
    Struct(StructP),
}

struct StructP {
    address: String,
    module: String,
    name: String,
    params: Vec<TypeTagP>,
}

fn split_generic(s: &str) -> Vec<String> {
    let mut tok = vec![];
    let mut word = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        if c == '<' {
            depth += 1;
        }
        if c == '>' {
            depth -= 1;
        }
        if depth == 0 && c == ',' {
            tok.push(word.trim().to_string());
            word.clear();
            continue;
        }
        word.push(c);
    }
    tok.push(word.trim().to_string());
    tok
}

/// MVR names (`@org/app`, `org.sui/app/1`) keep their spelling.
fn is_named_package(a: &str) -> bool {
    let parts: Vec<&str> = a.split('/').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    if parts.get(2).is_some_and(|v| v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let org = parts[0];
    let app = parts[1];
    let org_ok = !org.is_empty() && (org.contains('@') || org.to_ascii_lowercase().ends_with(".sui"));
    let app_ok = app.len() < 64 && !app.is_empty() && app.split('-').all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
    org_ok && app_ok
}

fn parse_type_tag(t: &str) -> Option<TypeTagP> {
    if let Some(inner) = t.strip_prefix("vector<") {
        let inner = inner.strip_suffix('>')?;
        if inner.is_empty() {
            return None;
        }
        return Some(TypeTagP::Plain(match parse_type_tag(inner)? {
            TypeTagP::Plain(s) => format!("vector<{s}>"),
            TypeTagP::Struct(s) => format!("vector<{}>", render(&s)),
        }));
    }
    if !t.contains("::") {
        return Some(TypeTagP::Plain(t.to_string()));
    }
    parse_struct(t).map(TypeTagP::Struct)
}

fn parse_struct(t: &str) -> Option<StructP> {
    let parts: Vec<&str> = t.split("::").collect();
    if parts.len() < 3 {
        return None;
    }
    let (address, module) = (parts[0], parts[1]);
    if address.is_empty() || module.is_empty() {
        return None;
    }
    let rest = &t[address.len() + module.len() + 4..];
    let lt = rest.find('<');
    let name = lt.map_or(rest, |i| &rest[..i]);
    if name.is_empty() || (lt.is_some() && !rest.ends_with('>')) {
        return None;
    }
    let params = match lt {
        Some(i) => split_generic(&rest[i + 1..rest.len() - 1]).iter().map(|p| parse_type_tag(p.trim())).collect::<Option<Vec<_>>>()?,
        None => vec![],
    };
    Some(StructP { address: if is_named_package(address) { address.to_string() } else { norm_id(address) }, module: module.into(), name: name.into(), params })
}

fn render(s: &StructP) -> String {
    let mut out = format!("{}::{}::{}", s.address, s.module, s.name);
    if !s.params.is_empty() {
        out.push('<');
        for (i, p) in s.params.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            match p {
                TypeTagP::Plain(x) => out.push_str(x),
                TypeTagP::Struct(x) => out.push_str(&render(x)),
            }
        }
        out.push('>');
    }
    out
}

/// `normalizeStructTag`, or the input unchanged when it does not parse (`normType`).
pub fn norm_type(t: &str) -> String {
    if t.starts_with("vector<") {
        return t.to_string();
    }
    parse_struct(t).map(|s| render(&s)).unwrap_or_else(|| t.to_string())
}

// ---------- BigInt ----------

/// `BigInt(s ?? '0')`, 0 when it would throw.
pub fn parse_big(s: Option<&str>) -> i128 {
    let t = s.unwrap_or("0").trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0;
    }
    for (p, r) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if t.len() > 2 && t.get(..2).is_some_and(|x| x.eq_ignore_ascii_case(p)) {
            return i128::from_str_radix(&t[2..], r).ok().filter(|_| t[2..].chars().all(|c| c.is_digit(r))).unwrap_or(0);
        }
    }
    let body = t.strip_prefix(['-', '+']).unwrap_or(t);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    t.parse().unwrap_or(0)
}

/// Serde for i128 base units as decimal strings (the TS bigints in JSON).
pub mod big_str {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &i128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i128, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }

    pub mod opt {
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(v: &Option<i128>, s: S) -> Result<S::Ok, S::Error> {
            match v {
                Some(v) => s.serialize_str(&v.to_string()),
                None => s.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i128>, D::Error> {
            Option::<String>::deserialize(d)?.map(|s| s.parse().map_err(serde::de::Error::custom)).transpose()
        }
    }
}

// ---------- insights ----------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinFlow {
    pub coin_type: String,
    /// Absolute base units.
    #[serde(with = "big_str")]
    pub amount: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObjectFlowKind {
    /// Owner changes to another address.
    Sent,
    /// Becomes a child of another object (kiosk listing, vault deposit).
    IntoObject,
    /// Wrapped inside another object.
    Wrapped,
    Deleted,
    /// Made shared: anyone can use it.
    Shared,
    /// Frozen, no one can change it again.
    Frozen,
    Received,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectFlow {
    pub object_id: String,
    #[serde(rename = "type")]
    pub object_type: Option<String>,
    pub kind: ObjectFlowKind,
    /// Recipient address or parent object id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedTouch {
    pub object_id: String,
    #[serde(rename = "type")]
    pub object_type: Option<String>,
    pub label: String,
    pub sub: String,
    pub created: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MoveCallRef {
    pub package: String,
    pub module: String,
    pub function: String,
}

/// A positive balance change for another address (who else gets coins).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherReceive {
    pub address: String,
    pub coin_type: String,
    #[serde(with = "big_str")]
    pub amount: i128,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fee {
    #[serde(with = "big_str")]
    pub computation: i128,
    #[serde(with = "big_str")]
    pub storage: i128,
    #[serde(with = "big_str")]
    pub rebate: i128,
    #[serde(with = "big_str")]
    pub net: i128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub ok: bool,
    /// Where the simulation ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SimSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<SimFailure>,
    /// Plain words for the failure banner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub send: Vec<CoinFlow>,
    pub receive: Vec<CoinFlow>,
    /// Positive balance changes for other addresses (who else gets coins).
    pub others_receive: Vec<OtherReceive>,
    pub objects_out: Vec<ObjectFlow>,
    pub objects_in: Vec<ObjectFlow>,
    pub shared: Vec<SharedTouch>,
    pub publishes: bool,
    pub fee: Fee,
    pub fee_paid_by_signer: bool,
    pub calls: Vec<MoveCallRef>,
    /// Other addresses that receive coins or objects from this tx.
    pub recipients: Vec<String>,
}

impl Insights {
    /// Nothing moves, success: a base for tests and hand-made insights.
    pub fn empty() -> Self {
        Insights {
            ok: true,
            source: None,
            failure: None,
            reason: None,
            send: vec![],
            receive: vec![],
            others_receive: vec![],
            objects_out: vec![],
            objects_in: vec![],
            shared: vec![],
            publishes: false,
            fee: Fee::default(),
            fee_paid_by_signer: true,
            calls: vec![],
            recipients: vec![],
        }
    }
}

fn owner_addr(o: Option<&Owner>) -> Option<String> {
    match o {
        Some(Owner::Address { address }) => Some(norm_id(address)),
        _ => None,
    }
}

/// `0x2::dynamic_field::Field<..>`: the wrapper a kiosk keeps each item under.
fn is_dynamic_field(t: Option<&str>) -> bool {
    t.is_some_and(|t| is_framework_prefixed(t, "dynamic_field::Field<"))
}

/// `/^0x0*2::kiosk::Kiosk$/`
fn is_kiosk(t: Option<&str>) -> bool {
    t.and_then(|t| t.strip_prefix("0x")).is_some_and(|r| r.trim_start_matches('0') == "2::kiosk::Kiosk")
}

/// `/^0x[0-9a-f]+::(kiosk::KioskOwnerCap|personal_kiosk::PersonalKioskCap)$/`
fn is_kiosk_cap(t: Option<&str>) -> bool {
    let Some(r) = t.and_then(|t| t.strip_prefix("0x")) else { return false };
    let Some((hex, rest)) = r.split_once("::") else { return false };
    !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) && (rest == "kiosk::KioskOwnerCap" || rest == "personal_kiosk::PersonalKioskCap")
}

/// Classify a simulation from the signer's point of view. Coins are balance changes; Coin<T>
/// objects, the gas coin, address-balance accumulators and packages never show as objects.
/// `kiosks` are the signer's kiosk ids: items in them count as the signer's on either side.
pub fn analyze(sim: &Simulation, signer: &str, calls: &[MoveCallRef], kiosks: &[String]) -> Insights {
    let me = norm_id(signer);
    // Kiosk items are owned by a dynamic field whose owner is the kiosk, not by the address.
    let mut kiosks: HashSet<String> = kiosks.iter().map(|k| norm_id(k)).collect();
    let mut field_in: HashMap<String, String> = HashMap::new();
    let mut field_out: HashMap<String, String> = HashMap::new();
    for o in &sim.objects {
        if !is_dynamic_field(o.object_type.as_deref()) {
            continue;
        }
        if let Some(Owner::Object { id }) = &o.input_owner {
            field_in.insert(norm_id(&o.object_id), norm_id(id));
        }
        if let Some(Owner::Object { id }) = &o.output_owner {
            field_out.insert(norm_id(&o.object_id), norm_id(id));
        }
    }
    // the kiosk (or other parent) an object-owned object sits in
    let parent_of = |o: Option<&Owner>, side: &HashMap<String, String>| -> Option<String> {
        match o {
            Some(Owner::Object { id }) => Some(side.get(&norm_id(id)).cloned().unwrap_or_else(|| id.clone())),
            _ => None,
        }
    };
    // a kiosk made in this transaction whose cap goes to one other address belongs to that
    // address: items moved into it are a send to them (how locked NFTs are sent)
    let new_kiosks: Vec<&str> =
        sim.objects.iter().filter(|o| !o.input_exists && matches!(o.output_owner, Some(Owner::Shared)) && is_kiosk(o.object_type.as_deref())).map(|o| o.object_id.as_str()).collect();
    let mut cap_to: Vec<String> = vec![];
    for o in sim.objects.iter().filter(|o| !o.input_exists && is_kiosk_cap(o.object_type.as_deref())) {
        if let Some(a) = owner_addr(o.output_owner.as_ref()).filter(|a| !a.is_empty() && *a != me) {
            if !cap_to.contains(&a) {
                cap_to.push(a);
            }
        }
    }
    let kiosk_owner = (new_kiosks.len() == 1 && cap_to.len() == 1).then(|| (norm_id(new_kiosks[0]), cap_to[0].clone()));
    // a kiosk made here whose cap comes to the signer is the signer's: a first purchase creates the
    // buyer's kiosk, and the item bought into it is what they receive
    let cap_to_me = sim.objects.iter().any(|o| !o.input_exists && is_kiosk_cap(o.object_type.as_deref()) && owner_addr(o.output_owner.as_ref()).as_deref() == Some(me.as_str()));
    if cap_to_me && cap_to.is_empty() {
        for k in &new_kiosks {
            kiosks.insert(norm_id(k));
        }
    }
    let mine = |o: Option<&Owner>, side: &HashMap<String, String>| -> bool {
        if owner_addr(o).as_deref() == Some(me.as_str()) {
            return true;
        }
        let p = if kiosks.is_empty() { None } else { parent_of(o, side) };
        p.is_some_and(|p| kiosks.contains(&norm_id(&p)))
    };

    let g = &sim.gas;
    let mut fee = Fee { computation: parse_big(Some(&g.computation)), storage: parse_big(Some(&g.storage)), rebate: parse_big(Some(&g.rebate)), net: 0 };
    fee.net = fee.computation + fee.storage - fee.rebate;
    let fee_paid_by_signer = match sim.gas_owner.as_deref() {
        None => true,
        Some(g) => g.is_empty() || norm_id(g) == me,
    };

    // balance changes, summed per (address, coin), in first-seen order
    let mut sums: Vec<OtherReceive> = vec![];
    let mut at: HashMap<(String, String), usize> = HashMap::new();
    let mut add = |address: String, coin_type: String, amount: i128| {
        let k = (address.clone(), coin_type.clone());
        match at.get(&k) {
            Some(i) => sums[*i].amount += amount,
            None => {
                at.insert(k, sums.len());
                sums.push(OtherReceive { address, coin_type, amount });
            }
        }
    };
    for c in &sim.balance_changes {
        add(norm_id(&c.address), norm_type(&c.coin_type), parse_big(Some(&c.amount)));
    }
    // the fee is its own line: take it out of the signer's SUI change
    if fee_paid_by_signer {
        add(me.clone(), SUI_TYPE.to_string(), fee.net);
    }
    let mut send = vec![];
    let mut receive = vec![];
    let mut others_receive = vec![];
    for s in sums {
        if s.amount == 0 {
            continue;
        }
        if s.address == me {
            if s.amount < 0 {
                send.push(CoinFlow { coin_type: s.coin_type, amount: -s.amount });
            } else {
                receive.push(CoinFlow { coin_type: s.coin_type, amount: s.amount });
            }
        } else if s.amount > 0 {
            others_receive.push(s);
        }
    }
    // largest first, stable like Array.prototype.sort
    send.sort_by_key(|f| std::cmp::Reverse(f.amount));
    receive.sort_by_key(|f| std::cmp::Reverse(f.amount));

    let mut objects_out = vec![];
    let mut objects_in = vec![];
    let mut shared = vec![];
    let mut publishes = false;
    let gas_id = sim.gas_object_id.as_deref().filter(|s| !s.is_empty()).map(norm_id);
    for o in &sim.objects {
        if o.output_kind == Some(OutputKind::Package) {
            publishes = true;
            continue;
        }
        if o.output_kind == Some(OutputKind::Accumulator) {
            continue;
        }
        if gas_id.as_deref().is_some_and(|g| norm_id(&o.object_id) == g) {
            continue;
        }
        let ty = o.object_type.as_deref();
        let shared_in = matches!(o.input_owner, Some(Owner::Shared));
        let shared_out = matches!(o.output_owner, Some(Owner::Shared));
        if shared_in || (shared_out && !o.input_exists) {
            let Label { title, sub } = object_label(ty, sim.packages.as_ref());
            shared.push(SharedTouch { object_id: o.object_id.clone(), object_type: o.object_type.clone(), label: title, sub, created: !o.input_exists });
            continue;
        }
        if is_coin_object(ty) || is_dynamic_field(ty) {
            continue;
        }
        let flow = |kind: ObjectFlowKind, to: Option<String>| ObjectFlow { object_id: o.object_id.clone(), object_type: o.object_type.clone(), kind, to, created: None };
        let to = owner_addr(o.output_owner.as_ref());
        let mine_out = mine(o.output_owner.as_ref(), &field_out);
        if o.input_exists && mine(o.input_owner.as_ref(), &field_in) {
            if !o.output_exists {
                objects_out.push(flow(if o.id_operation == IdOperation::Deleted { ObjectFlowKind::Deleted } else { ObjectFlowKind::Wrapped }, None));
            } else if mine_out {
                continue;
            } else if to.is_some() {
                objects_out.push(flow(ObjectFlowKind::Sent, to));
            } else if matches!(o.output_owner, Some(Owner::Object { .. })) {
                let parent = parent_of(o.output_owner.as_ref(), &field_out).unwrap_or_default();
                match &kiosk_owner {
                    Some((id, owner)) if norm_id(&parent) == *id => objects_out.push(flow(ObjectFlowKind::Sent, Some(owner.clone()))),
                    _ => objects_out.push(flow(ObjectFlowKind::IntoObject, Some(parent))),
                }
            } else if shared_out {
                objects_out.push(flow(ObjectFlowKind::Shared, None));
            } else if matches!(o.output_owner, Some(Owner::Immutable)) {
                objects_out.push(flow(ObjectFlowKind::Frozen, None));
            }
        } else if o.output_exists && mine_out {
            objects_in.push(ObjectFlow {
                object_id: o.object_id.clone(),
                object_type: o.object_type.clone(),
                kind: ObjectFlowKind::Received,
                to: None,
                created: Some(!o.input_exists || o.id_operation == IdOperation::Created),
            });
        }
    }

    let mut recipients: Vec<String> = vec![];
    for a in others_receive.iter().map(|r: &OtherReceive| r.address.clone()).chain(objects_out.iter().filter(|o| o.kind == ObjectFlowKind::Sent).filter_map(|o| o.to.clone())) {
        if !recipients.contains(&a) {
            recipients.push(a);
        }
    }
    let ok = sim.status == SimStatus::Success;
    Insights {
        ok,
        source: sim.source,
        failure: sim.failure.clone(),
        reason: (!ok).then(|| friendly_failure(sim.failure.as_ref())),
        send,
        receive,
        others_receive,
        objects_out,
        objects_in,
        shared,
        publishes,
        fee,
        fee_paid_by_signer,
        calls: calls.to_vec(),
        recipients,
    }
}

fn contains_ci(hay: &str, needles: &[&str]) -> bool {
    let h = hay.to_lowercase();
    needles.iter().any(|n| h.contains(&n.to_lowercase()))
}

/// `m.replace(/0x[0-9a-f]{20,}/gi, shortId)`
fn shorten_ids(m: &str) -> String {
    let b = m.as_bytes();
    let mut out = String::with_capacity(m.len());
    let mut i = 0;
    let mut last = 0;
    while i + 1 < b.len() {
        if b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X') {
            let mut j = i + 2;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j - (i + 2) >= 20 {
                out.push_str(&m[last..i]);
                out.push_str(&short_id(&m[i..j]));
                last = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&m[last..]);
    out
}

/// JS `s.length` (UTF-16 units) and `s.slice(0, n)`.
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn utf16_prefix(s: &str, n: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        if used + c.len_utf16() > n {
            return &s[..i];
        }
        used += c.len_utf16();
    }
    s
}

/// Short plain-words reason for a failed simulation.
pub fn friendly_failure(f: Option<&SimFailure>) -> String {
    let Some(f) = f else { return "Unknown error.".into() };
    let m = f.message.as_str();
    if contains_ci(m, &["InsufficientCoinBalance"]) {
        return "Not enough balance.".into();
    }
    if contains_ci(m, &["InsufficientGas", "lower than the needed amount", "gas balance", "GasBalanceTooLow", "budget"]) {
        return "Not enough SUI to pay the network fee.".into();
    }
    if let Some(a) = &f.abort {
        let module = a.module.as_deref().unwrap_or("");
        let function = a.function.as_deref().unwrap_or("");
        let at = if !module.is_empty() && !function.is_empty() { format!("{module}::{function}") } else { module.to_string() };
        // the framework's own "not enough" aborts
        if a.package.as_deref().is_some_and(|p| !p.is_empty() && norm_id(p) == norm_id("0x2")) && module == "balance" && a.code == "2" {
            return "Not enough balance.".into();
        }
        if let Some(c) = a.constant.as_deref().filter(|c| !c.is_empty()) {
            let c = match c.strip_prefix('E') {
                Some(r) if r.starts_with(|ch: char| ch.is_ascii_uppercase()) => r,
                _ => c,
            };
            let words = humanize(c);
            let mut chars = words.chars();
            let first = chars.next().map(|ch| ch.to_uppercase().collect::<String>()).unwrap_or_default();
            let rest = chars.as_str().to_lowercase();
            return format!("{first}{rest}{}.", if at.is_empty() { String::new() } else { format!(" ({at})") });
        }
        let proto = a.package.as_deref().filter(|p| !p.is_empty()).and_then(|p| known_package(p, None));
        return format!("{} stopped it{}, code {}.", proto.unwrap_or("The app's contract"), if at.is_empty() { String::new() } else { format!(" in {at}") }, a.code);
    }
    if contains_ci(m, &["input objects", "ObjectVersionUnavailable", "not available for consumption", "ObjectNotFound", "deleted"]) {
        return "Something it uses changed or no longer exists. Ask the site to try again.".into();
    }
    let clean = shorten_ids(m);
    if utf16_len(&clean) > 140 {
        return format!("{}\u{2026}", utf16_prefix(&clean, 137));
    }
    if clean.is_empty() {
        "Unknown error.".into()
    } else {
        clean
    }
}

/// Type-level package of a Move call target, used by the calls list.
pub fn call_label(c: &MoveCallRef, originals: Option<&BTreeMap<String, String>>) -> Label {
    Label { title: format!("{}::{}", c.module, c.function), sub: known_package(&c.package, originals).map(str::to_string).unwrap_or_else(|| short_id(&c.package)) }
}
