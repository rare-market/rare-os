//! "Louder speakers" for the SuiPlay0X1's "Rare Speakers" chain (image/.../50-rare-speaker-eq.conf):
//! the chain's input trim and limiter makeup gain, set live on the running chain with `pw-cli`.
//! On by default. The choice is kept in ~/.config/rare/speakers.json, which the image's
//! speaker-eq-route also reads when the chain reloads. Keep the numbers in step with that script.

use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// The chain's sink node.name.
pub const CHAIN: &str = "rare_speaker_eq";
const LIMIT: Duration = Duration::from_secs(3);

/// (trim, makeup dB). On: about +7.5 dB on ordinary content, peaks still held below -0.5 dBFS by
/// the limiter and clamp (measured through a null sink, see the conf).
const LOUD: (f32, f32) = (1.0, 9.0);
const SOFT: (f32, f32) = (0.841, 2.5);

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?;
    Some(base.join("rare/speakers.json"))
}

/// The saved choice; true when nothing is saved.
pub fn louder() -> bool {
    path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| parse(&t)).unwrap_or(true)
}

fn parse(t: &str) -> bool {
    serde_json::from_str::<Value>(t).ok().and_then(|v| v.get("louder")?.as_bool()).unwrap_or(true)
}

/// The params `pw-cli s <id> Props` takes.
fn props(on: bool) -> String {
    let (trim, gain) = if on { LOUD } else { SOFT };
    // Decimals always: a bare "1" is an Int pod, which a float control does not take.
    format!("{{ params = [ \"trimL:Mult\" {trim:.3} \"trimR:Mult\" {trim:.3} \"lim:gain (dB)\" {gain:.1} ] }}")
}

/// The chain's node id in a `pw-dump`, None when there is no chain (any machine but a SuiPlay0X1).
pub fn chain_id(dump: &str) -> Option<u32> {
    let Ok(Value::Array(objs)) = serde_json::from_str::<Value>(dump) else { return None };
    objs.iter()
        .find(|o| o.get("type").and_then(|t| t.as_str()).is_some_and(|t| t.ends_with(":Node")) && o.pointer("/info/props/node.name").and_then(|n| n.as_str()) == Some(CHAIN))
        .and_then(|o| o.get("id")?.as_u64())
        .map(|i| i as u32)
}

fn set_on(id: u32, on: bool) -> Result<(), String> {
    crate::system::run_for("pw-cli", &["s", &id.to_string(), "Props", &props(on)], LIMIT).map(|_| ())
}

/// The chain id the saved choice was last put on (it changes when PipeWire reloads the chain).
static APPLIED: Mutex<Option<u32>> = Mutex::new(None);

/// Puts the saved choice on the chain once per chain instance. Called with every audio reading.
pub fn ensure(id: Option<u32>) {
    let Some(id) = id else { return };
    let Ok(mut last) = APPLIED.lock() else { return };
    if *last == Some(id) {
        return;
    }
    if set_on(id, louder()).is_ok() {
        *last = Some(id);
    }
}

/// Saves the choice and puts it on the running chain.
pub fn set_louder(on: bool) -> Result<(), String> {
    let p = path().ok_or("No home folder")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, format!("{{\"louder\": {on}}}\n")).map_err(|e| e.to_string())?;
    let dump = crate::system::run_for("pw-dump", &[], LIMIT)?;
    let id = chain_id(&dump).ok_or("The speaker chain is not running")?;
    set_on(id, on)?;
    if let Ok(mut last) = APPLIED.lock() {
        *last = Some(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_choice_and_finds_the_chain() {
        assert!(parse(""));
        assert!(parse("{\"louder\": true}"));
        assert!(!parse("{\"louder\": false}\n"));
        assert_eq!(props(true), "{ params = [ \"trimL:Mult\" 1.000 \"trimR:Mult\" 1.000 \"lim:gain (dB)\" 9.0 ] }");
        assert_eq!(props(false), "{ params = [ \"trimL:Mult\" 0.841 \"trimR:Mult\" 0.841 \"lim:gain (dB)\" 2.5 ] }");
        let d = r#"[{"id": 37, "type": "PipeWire:Interface:Node", "info": {"props": {"node.name": "rare_speaker_eq"}}},
                    {"id": 38, "type": "PipeWire:Interface:Node", "info": {"props": {"node.name": "rare_speaker_eq_out"}}}]"#;
        assert_eq!(chain_id(d), Some(37));
        assert_eq!(chain_id("[]"), None);
    }
}
