//! Mute and deafen for Discord's voice while a game has the screen, from the Rare menu. Discord's
//! own buttons can't be reached from outside, so Rare does it where the sound goes through: mute
//! is Discord's microphone streams muted in PipeWire (the others hear silence), deafen is its
//! playback muted as well (as Discord's own deafen also mutes the microphone). New streams (a
//! call joined later) get the same state on the next tick. Through pipewire-pulse's `pactl`.

use serde_json::Value;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Which client the streams come from: the Discord app, or Rare's web window showing Discord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    App,
    Web,
}

/// A stream is Discord's: by the Flatpak app id PipeWire records, the program or the app name.
/// For Discord on the web, the streams of Rare's web window (it shows one page at a time).
pub fn is_discord(props: &Value, who: Who) -> bool {
    let p = |k: &str| props.get(k).and_then(|v| v.as_str()).unwrap_or("").to_ascii_lowercase();
    let app = p("pipewire.access.portal.app_id") == "com.discordapp.discord" || p("application.process.binary").contains("discord") || p("application.name").contains("discord");
    match who {
        Who::App => app,
        Who::Web => app || p("application.process.binary") == "rare-webgame-host" || p("application.name").contains("rare-webgame-host"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    pub index: u64,
    pub mute: bool,
}

/// The Discord streams in `pactl -f json list sink-inputs|source-outputs` output.
pub fn parse(json: &str, who: Who) -> Vec<Stream> {
    let Ok(Value::Array(a)) = serde_json::from_str::<Value>(json) else { return vec![] };
    a.iter()
        .filter(|s| is_discord(&s["properties"], who))
        // A monitor capture (Rare's own recorder, a level meter) is not the microphone.
        .filter(|s| !s["properties"]["stream.capture.sink"].as_str().is_some_and(|v| v == "true"))
        .filter_map(|s| Some(Stream { index: s["index"].as_u64()?, mute: s["mute"].as_bool().unwrap_or(false) }))
        .collect()
}

fn pactl(args: &[&str]) -> Option<String> {
    let mut c = Command::new("pactl");
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut ch = c.spawn().ok()?;
    let t = std::time::Instant::now();
    loop {
        match ch.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = ch.kill();
                let _ = ch.wait();
                return None;
            }
        }
    }
    let o = ch.wait_with_output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Discord's playback streams and microphone streams now.
pub fn streams(who: Who) -> (Vec<Stream>, Vec<Stream>) {
    let play = pactl(&["-f", "json", "list", "sink-inputs"]).map(|j| parse(&j, who)).unwrap_or_default();
    let mic = pactl(&["-f", "json", "list", "source-outputs"]).map(|j| parse(&j, who)).unwrap_or_default();
    (play, mic)
}

/// What each kind of stream should be: (playback muted, microphone muted).
pub fn want(muted: bool, deafened: bool) -> (bool, bool) {
    (deafened, muted || deafened)
}

/// Puts Discord's streams in the state the switches ask for. Returns how many it changed.
pub fn apply(who: Who, muted: bool, deafened: bool) -> usize {
    let (play, mic) = streams(who);
    let (want_play, want_mic) = want(muted, deafened);
    let mut n = 0;
    for s in play.iter().filter(|s| s.mute != want_play) {
        if pactl(&["set-sink-input-mute", &s.index.to_string(), if want_play { "1" } else { "0" }]).is_some() {
            n += 1;
        }
    }
    for s in mic.iter().filter(|s| s.mute != want_mic) {
        if pactl(&["set-source-output-mute", &s.index.to_string(), if want_mic { "1" } else { "0" }]).is_some() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINK_INPUTS: &str = r#"[
      {"index":45,"mute":false,"properties":{"application.name":"Discord","application.process.binary":"Discord","pipewire.access.portal.app_id":"com.discordapp.Discord"}},
      {"index":46,"mute":true,"properties":{"application.name":"WEBRTC VoiceEngine","pipewire.access.portal.app_id":"com.discordapp.Discord"}},
      {"index":47,"mute":false,"properties":{"application.name":"Hades II","application.process.binary":"wine64-preloader"}},
      {"index":48,"mute":false,"properties":{"application.name":"rare-webgame-host","application.process.binary":"rare-webgame-host"}}
    ]"#;

    #[test]
    fn finds_discord_streams() {
        assert_eq!(parse(SINK_INPUTS, Who::App), vec![Stream { index: 45, mute: false }, Stream { index: 46, mute: true }]);
        assert_eq!(parse(SINK_INPUTS, Who::Web).iter().map(|s| s.index).collect::<Vec<_>>(), vec![45, 46, 48]);
        assert!(parse("not json", Who::App).is_empty());
        let monitor = r#"[{"index":9,"mute":false,"properties":{"application.name":"Discord","stream.capture.sink":"true"}}]"#;
        assert!(parse(monitor, Who::App).is_empty());
    }

    #[test]
    fn deafen_mutes_both() {
        assert_eq!(want(false, false), (false, false));
        assert_eq!(want(true, false), (false, true));
        assert_eq!(want(false, true), (true, true));
        assert_eq!(want(true, true), (true, true));
    }
}
