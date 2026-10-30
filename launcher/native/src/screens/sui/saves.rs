//! Cloud saves (launcher/src/sui/ui/CloudSaves.tsx): emulator and web game saves backed up to Walrus,
//! encrypted with a key from the player's wallet. Every upload and restore starts from a press on
//! that save set; nothing runs in the background and nothing is paid without Rare Wallet's review.
//! The work itself is rare-sui's `saves` (WS 9); this is the screen over its state.

use crate::screens::wallet::fmt::{date_md, num_locale, to_precision};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, dot, empty, head_row, load_bar, panel, section_head, segmented, BSize, ButtonSpec, BVariant, Tone};

pub const KEEP_OPTIONS: [u32; 4] = [30, 90, 180, 365];

pub fn keep_label(d: u32) -> &'static str {
    match d {
        30 => "30 days",
        90 => "90 days",
        180 => "6 months",
        _ => "1 year",
    }
}

/// `fmtWal`: FROST as WAL, three significant digits.
pub fn fmt_wal(frost: Option<u128>) -> String {
    let Some(f) = frost else { return String::new() };
    let n = f as f64 / 1e9;
    if n == 0.0 {
        return "0 WAL".into();
    }
    if n < 0.0001 {
        return "<0.0001 WAL".into();
    }
    let p: f64 = to_precision(n, 3).parse().unwrap_or(n);
    format!("{} WAL", num_locale(p, 0, 4))
}

/// `fmtSuiMist`: two significant digits.
pub fn fmt_sui_mist(mist: u128) -> String {
    let n = mist as f64 / 1e9;
    let p: f64 = if n == 0.0 { 0.0 } else { to_precision(n, 2).parse().unwrap_or(n) };
    format!("{} SUI", num_locale(p, 0, 4))
}

/// `fmtBytes`.
pub fn fmt_bytes(b: u64) -> String {
    let f = b as f64;
    if f >= 1e9 {
        format!("{} GB", crate::screens::wallet::fmt::to_fixed(f / 1e9, 1))
    } else if f >= 1e6 {
        format!("{} MB", crate::screens::wallet::fmt::to_fixed(f / 1e6, 1))
    } else {
        format!("{} KB", (f / 1e3).round().max(1.0))
    }
}

/// `ago`: "just now", "3 h ago", "yesterday", "12 days ago", "Mar 4".
pub fn ago(t: Option<i64>, now: i64, tz_min: i32) -> String {
    let Some(t) = t.filter(|t| *t != 0) else { return String::new() };
    let s = (now - t) as f64 / 1000.0;
    if s < 3600.0 {
        "just now".into()
    } else if s < 86400.0 {
        format!("{} h ago", (s / 3600.0).round())
    } else if s < 86400.0 * 2.0 {
        "yesterday".into()
    } else if s < 86400.0 * 30.0 {
        format!("{} days ago", (s / 86400.0).round())
    } else {
        date_md(t, tz_min, false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Pack,
    Encode,
    Register,
    Upload,
    Certify,
    Done,
    Download,
    Unpack,
}

pub fn step_label(s: Step) -> &'static str {
    match s {
        Step::Pack => "Packing",
        Step::Encode => "Encrypting",
        Step::Register => "Approve in Rare Wallet",
        Step::Upload => "Uploading",
        Step::Certify => "Approve to confirm",
        Step::Done => "Done",
        Step::Download => "Downloading",
        Step::Unpack => "Restoring",
    }
}

/// sui/native.ts `SaveSet`.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveSet {
    pub id: String,
    pub emulator: String,
    pub label: String,
    pub files: u64,
    pub bytes: u64,
    /// newest file, unix ms
    pub modified: Option<i64>,
}

/// A backup on Walrus (`Backup` with its sealed meta opened).
#[derive(Clone, Debug, PartialEq)]
pub struct Backup {
    pub blob_object_id: String,
    pub set_id: String,
    pub emulator: String,
    pub label: String,
    pub bytes: u64,
    pub created: i64,
    /// `daysLeft` when the epoch is known
    pub days_left: Option<u64>,
}

/// `CostEstimate` (FROST and MIST).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cost {
    pub wal_frost: u128,
    pub tip_mist: u128,
    pub gas_mist: u128,
}

/// `useCloud` state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cloud {
    /// None while detecting
    pub sets: Option<Vec<SaveSet>>,
    pub backups: Option<Vec<Backup>>,
    /// the save key is made (unlocked on this device)
    pub key: bool,
    pub days: u32,
    pub testnet: bool,
    pub busy: Option<(String, Step)>,
    pub error: Option<String>,
    pub estimates: Vec<(String, Cost)>,
    /// Automatic cloud saves (features/cloudsync.rs)
    pub auto: Auto,
}

/// The automatic section: on, waiting for the wallet, and the writer's funds (WAL, SUI).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Auto {
    pub on: bool,
    pub unlocking: bool,
    pub funds: Option<(u64, u64)>,
    pub uploads_left: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Unlock,
    BackUp(String),
    /// the first press asks again ("Press again"), the second restores
    Restore(String),
    /// X: the next keep option
    Keep,
    AutoOn,
    AutoOff,
    AddFunds,
}

const COLS: &str = "grid-cols-[minmax(0,1fr)_6rem_7.5rem_9rem_7.5rem_15rem]";

/// `latestBySet`: the newest backup per save set.
pub fn latest_by_set(list: &[Backup]) -> Vec<(&str, &Backup)> {
    let mut out: Vec<(&str, &Backup)> = Vec::new();
    for b in list {
        match out.iter_mut().find(|(id, _)| *id == b.set_id) {
            Some(e) => {
                if b.created > e.1.created {
                    e.1 = b;
                }
            }
            None => out.push((&b.set_id, b)),
        }
    }
    out
}

/// `address`: the account the saves belong to; `confirm` is the backup waiting for its second press.
pub fn view(c: &Cloud, address: Option<&str>, confirm: Option<&str>, now: i64, tz_min: i32) -> El<Msg> {
    let backups = c.backups.as_deref().unwrap_or(&[]);
    let latest = latest_by_set(backups);
    let here: Vec<&str> = c.sets.as_deref().unwrap_or(&[]).iter().map(|s| s.id.as_str()).collect();
    let elsewhere: Vec<&Backup> = latest.iter().filter(|(id, _)| !here.contains(id)).map(|(_, b)| *b).collect();
    let total_wal: u128 = c.estimates.iter().map(|(_, e)| e.wal_frost).sum();
    let total_sui: u128 = c.estimates.iter().map(|(_, e)| e.tip_mist + e.gas_mist).sum();
    let keep = keep_label(c.days);
    let n = backups.len();
    let mut s = div("h-full overflow-y-auto px-[2.5rem] pb-[3rem] pt-[2rem]").attr("data-scroll", "").kid(
        div("flex items-end gap-[1rem]")
            .kid(
                div("min-w-0 flex-1").kid(h1("text-title font-semibold tracking-[-0.02em]").text("Cloud saves")).kid(p("mt-[0.25rem] max-w-[46rem] text-small text-muted").text(
                    "Emulator and web game saves, encrypted on this device and kept on Walrus. Your wallet pays for the storage and only your account can open them. Steam games use Steam Cloud.",
                )),
            )
            .kid(span("").kid(glyph("x")).attr("data-size", "sm"))
            .kid(segmented(&[("30", "30 days"), ("90", "90 days"), ("180", "6 months"), ("365", "1 year")], &c.days.to_string(), "")),
    );
    let status = match (address, c.key) {
        (None, _) => ("No account".to_string(), "Add a Rare Wallet account to back up saves.".to_string()),
        (Some(_), true) => (
            "Unlocked on this device".into(),
            format!(
                "{n} {} on Walrus{}. Backing up everything here for {keep} costs about {} plus {} in fees.",
                if n == 1 { "backup" } else { "backups" },
                if c.testnet { " testnet" } else { "" },
                fmt_wal(Some(total_wal)),
                fmt_sui_mist(total_sui)
            ),
        ),
        (Some(_), false) => ("Locked".into(), "Approve one signature in Rare Wallet to make your save key. It costs nothing and the key never leaves this device.".into()),
    };
    let mut pnl = vec![
        span("flex h-[2.5rem] w-[2.5rem] shrink-0 items-center justify-center rounded-full bg-l3 text-icon-2").kid(if c.key { icon("cloud", "").attr("size", "1.2rem") } else { icon("lock", "").attr("size", "1.15rem") }),
        div("min-w-0 flex-1").kid(div("text-body font-semibold").text(status.0)).kid(div("text-small text-muted").text(status.1)),
    ];
    if address.is_some() && !c.key {
        pnl.push(button_el(ButtonSpec::new("Unlock").v(BVariant::Primary).def(true).hint("Unlock"), Some(Msg::Unlock)));
    }
    s = s.kid(panel("mt-[1.5rem] flex items-center gap-[1rem] px-[1.25rem] py-[1rem]", pnl));
    s = s.kid(auto_panel(&c.auto, address.is_some()));
    if let Some(e) = &c.error {
        s = s.kid(div("mt-[0.9rem] flex items-center gap-[0.5rem] text-small text-fg").kid(dot(Tone::Negative, false, "")).kid(text(e)));
    }
    s = s.kid(section_head(text("On this device"), c.sets.as_ref().map(|x| text(format!("{} found", x.len()))), None, "mb-[0.5rem] mt-[2rem]"));
    match &c.sets {
        Some(sets) if sets.is_empty() => {
            s = s.kid(empty(
                "No emulator saves found",
                Some(text("Rare looks in RetroArch, RPCS3, PCSX2, Cemu, DuckStation, Dolphin, PPSSPP and other emulators, EmuDeck's Emulation folder and your web games.")),
                vec![],
                "",
            ));
        }
        Some(sets) => {
            s = s.kid(head_row(
                COLS,
                "",
                vec![span("").text("Saves"), span("text-right").text("Size"), span("text-right").text("Changed"), span("text-right").text("Last backup"), span("text-right").text(keep), span("")],
            ));
            s = s.kid(div("").attr("data-nav-group", "saves-here").kids(sets.iter().map(|set| {
                let last = latest.iter().find(|(id, _)| *id == set.id).map(|(_, b)| *b);
                let est = c.estimates.iter().find(|(id, _)| *id == set.id).map(|(_, e)| e.wal_frost);
                set_row(set, last, est, c, confirm, now, tz_min)
            })));
        }
        None => {}
    }
    if !elsewhere.is_empty() {
        s = s.kid(section_head(text("From your other devices"), Some(text("not on this device yet")), None, "mb-[0.5rem] mt-[2rem]"));
        s = s.kid(div("").attr("data-nav-group", "saves-elsewhere").kids(elsewhere.iter().map(|b| {
            let mine = c.busy.as_ref().filter(|(id, _)| *id == b.set_id).map(|(_, st)| step_label(*st));
            let again = confirm == Some(b.blob_object_id.as_str());
            let label = mine.unwrap_or(if again { "Press again" } else { "Restore here" });
            div(cn(&["grid h-[3.25rem] items-center gap-x-[0.75rem] px-[0.75rem] text-small", COLS]))
                .key(&b.blob_object_id)
                .kid(span("min-w-0 truncate").kid(span("font-medium text-fg").text(&b.emulator)).kid(text(" ")).kid(span("text-muted").text(&b.label)))
                .kid(span("text-right tabular-nums text-muted").text(fmt_bytes(b.bytes)))
                .kid(span(""))
                .kid(span("text-right text-muted").text(ago(Some(b.created), now, tz_min)))
                .kid(span(""))
                .kid(span("flex justify-end").kid(button_el(ButtonSpec::new(label).size(BSize::Sm).hint(if again { "Restore" } else { "Restore here" }).disabled(c.busy.is_some()), Some(Msg::Restore(b.blob_object_id.clone())))))
        })));
    }
    div("rise fixed inset-0 z-30 bg-bg").attr("data-nav-scope", "").on_pad("x", Msg::Keep).kid(load_bar(c.busy.is_some() || c.sets.is_none(), "")).kid(s)
}

/// Automatic cloud saves: on or off, and the funds its uploads use.
fn auto_panel(a: &Auto, account: bool) -> El<Msg> {
    let (title, body) = if a.on {
        let funds = match (a.funds, a.uploads_left) {
            (Some((wal, sui)), Some(n)) => format!(" Funds: {} and {}, about {n} uploads.", fmt_wal(Some(wal as u128)), fmt_sui_mist(sui as u128)),
            _ => String::new(),
        };
        ("Automatic cloud saves are on", format!("Rare checks the cloud before you play and uploads after, for emulators, web games and PC games outside Steam, Epic and GOG. Uploads use a cloud saves address of your account, so nothing asks to approve.{funds}"))
    } else {
        ("Automatic cloud saves are off", "Turn on to sync saves before and after every session. One wallet signature on this device; then add a little WAL and SUI for the uploads.".to_string())
    };
    let mut kids = vec![
        span("flex h-[2.5rem] w-[2.5rem] shrink-0 items-center justify-center rounded-full bg-l3 text-icon-2").kid(icon(if a.on { "refresh-cw" } else { "cloud-off" }, "").attr("size", "1.15rem")),
        div("min-w-0 flex-1").kid(div("text-body font-semibold").text(title)).kid(div("text-small text-muted").text(body)),
    ];
    if a.on {
        kids.push(button_el(ButtonSpec::new("Add funds").hint("Add 1 WAL and 0.2 SUI"), Some(Msg::AddFunds)));
        kids.push(button_el(ButtonSpec::new("Turn off").hint("Turn off"), Some(Msg::AutoOff)));
    } else if account {
        kids.push(button_el(ButtonSpec::new(if a.unlocking { "Waiting for the wallet" } else { "Turn on" }).v(BVariant::Primary).hint("Turn on").disabled(a.unlocking), Some(Msg::AutoOn)));
    }
    panel("mt-[0.75rem] flex items-center gap-[1rem] px-[1.25rem] py-[1rem]", kids)
}

fn set_row(set: &SaveSet, last: Option<&Backup>, est: Option<u128>, c: &Cloud, confirm: Option<&str>, now: i64, tz_min: i32) -> El<Msg> {
    let mine = c.busy.as_ref().filter(|(id, _)| *id == set.id).map(|(_, s)| *s);
    let last_txt = if !c.key {
        "Unlock to see".to_string()
    } else if let Some(b) = last {
        format!("{}{}", ago(Some(b.created), now, tz_min), b.days_left.map(|d| format!(" · {d} d left")).unwrap_or_default())
    } else {
        "Never".into()
    };
    let actions = match mine {
        Some(st) => span("flex h-[2.25rem] items-center gap-[0.5rem] text-small text-fg").kid(dot(Tone::Brand, true, "")).kid(text(step_label(st))),
        None => {
            let mut a = span("flex justify-end gap-[0.5rem]");
            if let Some(b) = last {
                let again = confirm == Some(b.blob_object_id.as_str());
                a = a.kid(button_el(
                    ButtonSpec::new(if again { "Press again" } else { "Restore" }).size(BSize::Sm).hint(if again { "Restore" } else { "Restore this backup" }).disabled(c.busy.is_some()),
                    Some(Msg::Restore(b.blob_object_id.clone())),
                ));
            }
            a.kid(button_el(ButtonSpec::new("Back up").size(BSize::Sm).hint("Back up now").disabled(!c.key || c.busy.is_some()), Some(Msg::BackUp(set.id.clone()))))
        }
    };
    let actions = if mine.is_some() { span("flex justify-end gap-[0.5rem]").kid(actions) } else { actions };
    div(cn(&["grid h-[3.25rem] items-center gap-x-[0.75rem] rounded-card px-[0.75rem] text-small", COLS]))
        .key(&set.id)
        .attr("data-nav-group", format!("save-{}", set.id))
        .attr("data-nav-remember", "")
        .kid(
            span("min-w-0 truncate")
                .kid(span("font-medium text-fg").text(&set.emulator))
                .kid(text(" "))
                .kid(span("text-muted").text(&set.label))
                .kid(span("ml-[0.5rem] text-label text-muted").text(format!("{} files", num_locale(set.files as f64, 0, 0)))),
        )
        .kid(span("text-right tabular-nums text-fg").text(fmt_bytes(set.bytes)))
        .kid(span("text-right text-muted").text(ago(set.modified, now, tz_min)))
        .kid(span("text-right text-muted").text(last_txt))
        .kid(span("text-right tabular-nums text-fg").text(fmt_wal(est)))
        .kid(actions)
}

/// X steps through the keep options; Restore asks twice.
pub fn keep_next(days: u32) -> u32 {
    let i = KEEP_OPTIONS.iter().position(|d| *d == days).unwrap_or(0);
    KEEP_OPTIONS[(i + 1) % KEEP_OPTIONS.len()]
}

/// The restore press: Some(id) restores now, None arms the second press (`confirm` becomes it).
pub fn restore_press(confirm: &mut Option<String>, id: &str) -> Option<String> {
    if confirm.as_deref() != Some(id) {
        *confirm = Some(id.to_string());
        return None;
    }
    *confirm = None;
    Some(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::{ME, NOW};

    #[test]
    fn formats() {
        assert_eq!(fmt_wal(Some(0)), "0 WAL");
        assert_eq!(fmt_wal(Some(50_000)), "<0.0001 WAL");
        assert_eq!(fmt_wal(Some(1_234_567_890)), "1.23 WAL");
        assert_eq!(fmt_wal(None), "");
        assert_eq!(fmt_sui_mist(12_345_678), "0.012 SUI");
        assert_eq!(fmt_bytes(2_500_000), "2.5 MB");
        assert_eq!(fmt_bytes(300), "1 KB");
        assert_eq!(ago(Some(NOW - 5 * 3_600_000), NOW, 0), "5 h ago");
        assert_eq!(ago(Some(NOW - 40 * 86_400_000), NOW, 0), "Sep 17");
        assert_eq!(keep_next(365), 30);
    }

    #[test]
    fn screen() {
        let c = Cloud {
            sets: Some(vec![SaveSet { id: "rpcs3".into(), emulator: "RPCS3".into(), label: "Saves".into(), files: 1204, bytes: 52_000_000, modified: Some(NOW - 7_200_000) }]),
            backups: Some(vec![Backup { blob_object_id: "0xb1".into(), set_id: "dolphin".into(), emulator: "Dolphin".into(), label: "Memory cards".into(), bytes: 8_000_000, created: NOW - 3 * 86_400_000, days_left: Some(27) }]),
            key: true,
            days: 90,
            testnet: false,
            busy: None,
            error: None,
            estimates: vec![("rpcs3".into(), Cost { wal_frost: 120_000_000, tip_mist: 2_000_000, gas_mist: 4_000_000 })],
            auto: Auto::default(),
        };
        let v = view(&c, Some(ME), None, NOW, 0);
        assert!(v.texts().contains(&"Automatic cloud saves are off".to_string()));
        let t = v.texts();
        assert!(t.contains(&"Unlocked on this device".to_string()));
        assert!(t.contains(&"1 backup on Walrus. Backing up everything here for 90 days costs about 0.12 WAL plus 0.006 SUI in fees.".to_string()));
        assert!(t.contains(&"1,204 files".to_string()));
        assert!(t.contains(&"From your other devices".to_string()));
        assert!(t.contains(&"Restore here".to_string()));
        assert_eq!(v.pad("x"), Some(&Msg::Keep));
        let mut confirm = None;
        assert_eq!(restore_press(&mut confirm, "0xb1"), None);
        assert!(view(&c, Some(ME), confirm.as_deref(), NOW, 0).contains_text("Press again"));
        assert_eq!(restore_press(&mut confirm, "0xb1"), Some("0xb1".into()));
        let locked = Cloud { key: false, ..c.clone() };
        let v = view(&locked, Some(ME), None, NOW, 0);
        assert_eq!(v.landing().as_deref(), Some("Unlock"));
        assert!(v.contains_text("Unlock to see"));
        let busy = Cloud { busy: Some(("rpcs3".into(), Step::Upload)), ..c };
        assert!(view(&busy, Some(ME), None, NOW, 0).contains_text("Uploading"));
    }
}
