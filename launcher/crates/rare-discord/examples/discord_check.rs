//! Device check for Discord presence: Rare's supervisor with Discord on the web "open" and a game
//! running starts arRPC (RARE_ARRPC_DIR) and reports the game to it; then a second client, as a
//! game with Discord's SDK would, sets its own activity. Watch arRPC's bridge (ws://127.0.0.1:1337)
//! to see what Discord on the web would get. Use a scratch XDG_RUNTIME_DIR.
//!
//!   RARE_ARRPC_DIR=~/rare-native/arrpc XDG_RUNTIME_DIR=/tmp/x cargo run --example discord_check

use rare_discord::{ipc, DiscordHost};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

struct Host;

impl DiscordHost for Host {
    fn presence_enabled(&self) -> bool {
        true
    }
    fn running(&self) -> Vec<(String, String)> {
        vec![("app:discord".into(), "Discord".into()), ("steam:504230".into(), "Celeste".into())]
    }
    fn env(&self, _cmd: &mut std::process::Command) {}
}

/// VOICE=1: mute and deafen on the Discord streams playing now (start a stream named Discord first).
fn voice() {
    use rare_discord::voice::{apply, streams, Who};
    let show = |w: &str| {
        let (p, m) = streams(Who::App);
        println!("{w}: playback {:?}, microphone {:?}", p.iter().map(|s| s.mute).collect::<Vec<_>>(), m.iter().map(|s| s.mute).collect::<Vec<_>>());
    };
    show("before");
    apply(Who::App, true, false);
    show("muted");
    apply(Who::App, false, true);
    show("deafened");
    apply(Who::App, false, false);
    show("back");
}

fn main() {
    if std::env::var_os("VOICE").is_some() {
        voice();
        return;
    }
    rare_discord::start(Arc::new(Host));
    for _ in 0..6 {
        std::thread::sleep(Duration::from_secs(2));
        println!("status: {}", serde_json::to_string(&rare_discord::status()).unwrap());
    }
    let sock = rare_discord::arrpc::socket().expect("arRPC socket");
    let mut game = ipc::Client::connect(&sock, rare_discord::ARRPC_APP_ID).expect("connect as a game");
    game.set_activity(4242, Some(&json!({ "name": "Test client game", "details": "Level 3", "timestamps": { "start": 1 } }))).expect("set activity");
    println!("test client: activity set on {}", sock.display());
    std::thread::sleep(Duration::from_secs(2));
    game.set_activity(4242, None).expect("clear");
    println!("test client: cleared");
    drop(game);
    std::thread::sleep(Duration::from_secs(1));
    rare_discord::shutdown();
    println!("done");
}
