//! Sound outputs and the microphone, through PipeWire. `pw-dump` gives the graph (sinks, sources,
//! the cards' active routes, the "default" metadata); `wpctl` changes the default and the mic.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(3);

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Speaker,
    Headphones,
    Bluetooth,
    Hdmi,
    Usb,
}

/// One sink (output) or source (input).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: u32,
    /// PipeWire's node.name.
    pub name: String,
    /// A plain name for people: "Speaker", "WH-1000XM4", "TV (HDMI)".
    pub label: String,
    pub kind: Kind,
    pub default: bool,
}

impl Device {
    /// A chip label: Speaker, Headphones, the Bluetooth name cut to 12 characters, HDMI, USB.
    pub fn short(&self) -> String {
        match self.kind {
            Kind::Speaker => "Speaker".into(),
            Kind::Headphones => "Headphones".into(),
            Kind::Hdmi => "HDMI".into(),
            Kind::Usb => "USB".into(),
            Kind::Bluetooth => {
                let n: Vec<char> = self.label.chars().collect();
                if n.len() > 12 {
                    format!("{}…", n[..11].iter().collect::<String>().trim_end())
                } else {
                    self.label.clone()
                }
            }
        }
    }
}

/// Everything Quick settings and Settings show.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Audio {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    /// None when there is no mic.
    pub mic_muted: Option<bool>,
    /// 0..=100.
    pub mic_level: Option<u8>,
    /// "Louder speakers": None without the SuiPlay0X1's speaker chain.
    pub louder: Option<bool>,
}

impl Audio {
    pub fn default_output(&self) -> Option<&Device> {
        self.outputs.iter().find(|d| d.default)
    }
    pub fn default_input(&self) -> Option<&Device> {
        self.inputs.iter().find(|d| d.default)
    }
}

fn props(o: &Value) -> Option<&serde_json::Map<String, Value>> {
    o.get("info")?.get("props")?.as_object()
}

fn s<'a>(p: &'a serde_json::Map<String, Value>, k: &str) -> Option<&'a str> {
    p.get(k)?.as_str().filter(|x| !x.is_empty())
}

/// A metadata value: {"name": "..."} or (older pw-dump) the same JSON as a string.
fn meta_name(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => o.get("name")?.as_str().map(String::from),
        Value::String(t) => serde_json::from_str::<Value>(t).ok().and_then(|j| j.get("name")?.as_str().map(String::from)).or_else(|| Some(t.clone())),
        _ => None,
    }
}

/// "Rembrandt Radeon High Definition Audio Controller Digital Stereo (HDMI)" -> the card name only.
fn tidy(d: &str) -> String {
    let mut t = d.trim().to_string();
    for suf in [" (HDMI)", " Analog Stereo", " Digital Stereo", " Analog Surround 5.1", " Analog Mono", " Mono", " Stereo", " Multichannel"] {
        if let Some(x) = t.strip_suffix(suf) {
            t = x.trim().to_string();
        }
    }
    t
}

/// Sinks and sources from `pw-dump`'s JSON: (outputs, inputs). Monitors and virtual nodes are skipped.
pub fn parse_dump(json: &str) -> (Vec<Device>, Vec<Device>) {
    let Ok(Value::Array(objs)) = serde_json::from_str::<Value>(json) else { return (vec![], vec![]) };
    // Each card's active routes: device id -> route names ("analog-output-headphones").
    let mut routes: std::collections::HashMap<u64, Vec<String>> = Default::default();
    let mut dev_props: std::collections::HashMap<u64, serde_json::Map<String, Value>> = Default::default();
    let (mut sink_def, mut src_def) = ((None, None), (None, None));
    for o in &objs {
        let ty = o.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let id = o.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
        if ty.ends_with(":Device") {
            if let Some(p) = props(o) {
                dev_props.insert(id, p.clone());
            }
            let r: Vec<String> = o
                .pointer("/info/params/Route")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().filter_map(|x| x.get("name")?.as_str().map(String::from)).collect())
                .unwrap_or_default();
            routes.insert(id, r);
        } else if ty.ends_with(":Metadata") && o.pointer("/props/metadata.name").and_then(|n| n.as_str()) == Some("default") {
            for m in o.get("metadata").and_then(|m| m.as_array()).into_iter().flatten() {
                let v = m.get("value").and_then(meta_name);
                match m.get("key").and_then(|k| k.as_str()) {
                    Some("default.audio.sink") => sink_def.0 = v,
                    Some("default.configured.audio.sink") => sink_def.1 = v,
                    Some("default.audio.source") => src_def.0 = v,
                    Some("default.configured.audio.source") => src_def.1 = v,
                    _ => {}
                }
            }
        }
    }
    // A filter chain in front of a sink ("Rare Speakers" -> the ALSA sink): its virtual sink is
    // where sound should go, so the real sink's entry stands for the chain (picking "Speaker" picks
    // the chain, and it shows as the default). The chain's output stream names the real sink in
    // target.object and shares a node.link-group with the virtual sink.
    let group = |o: &Value| o.pointer("/info/props/node.link-group").and_then(|g| g.as_str()).map(String::from);
    let mut fronts: std::collections::HashMap<String, (u32, String)> = Default::default();
    for o in &objs {
        let Some(p) = props(o) else { continue };
        let (Some("Stream/Output/Audio"), Some(target), Some(g)) = (s(p, "media.class"), s(p, "target.object"), group(o)) else { continue };
        let front = objs.iter().find(|x| {
            props(x).is_some_and(|q| s(q, "media.class") == Some("Audio/Sink") && q.get("node.virtual").is_some_and(|v| v == true || v == "true")) && group(x).as_deref() == Some(g.as_str())
        });
        if let Some(f) = front {
            let fname = f.pointer("/info/props/node.name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let fid = f.get("id").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            if !fname.is_empty() {
                fronts.insert(target.to_string(), (fid, fname));
            }
        }
    }
    let (mut outs, mut ins) = (vec![], vec![]);
    for o in &objs {
        if !o.get("type").and_then(|t| t.as_str()).unwrap_or("").ends_with(":Node") {
            continue;
        }
        let Some(p) = props(o) else { continue };
        let class = s(p, "media.class").unwrap_or("");
        let output = match class {
            "Audio/Sink" => true,
            "Audio/Source" => false,
            _ => continue,
        };
        let Some(name) = s(p, "node.name") else { continue };
        // node.virtual is a bool in pw-dump (a string in older ones).
        if name.ends_with(".monitor") || name == "auto_null" || p.get("node.virtual").is_some_and(|v| v == true || v == "true") {
            continue;
        }
        let id = o.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
        let dev = p.get("device.id").and_then(|d| d.as_u64().or_else(|| d.as_str()?.parse().ok()));
        let dp = dev.and_then(|d| dev_props.get(&d));
        let get = |k: &str| s(p, k).or_else(|| dp.and_then(|d| s(d, k)));
        let desc = get("node.description").or_else(|| get("device.description")).unwrap_or(name).to_string();
        let nick = s(p, "node.nick").unwrap_or("").to_string();
        let low = format!("{name} {desc}").to_lowercase();
        let bus = get("device.bus").unwrap_or("").to_lowercase();
        let api = get("device.api").unwrap_or("");
        let route_hp = dev.and_then(|d| routes.get(&d)).is_some_and(|r| r.iter().any(|x| x.contains(if output { "output-headphones" } else { "input-headset" }) || x.contains(if output { "output-headset" } else { "input-headphone" })));
        let kind = if name.starts_with("bluez") || api == "bluez5" || bus == "bluetooth" {
            Kind::Bluetooth
        } else if low.contains("hdmi") || low.contains("displayport") {
            Kind::Hdmi
        } else if bus == "usb" || name.contains("usb") {
            Kind::Usb
        } else if route_hp || low.contains("headphone") || low.contains("headset") {
            Kind::Headphones
        } else {
            Kind::Speaker
        };
        let label = match kind {
            Kind::Speaker if output => "Speaker".to_string(),
            Kind::Speaker => "Built-in mic".to_string(),
            Kind::Headphones if output => "Headphones".to_string(),
            Kind::Headphones => "Headset mic".to_string(),
            Kind::Bluetooth => get("device.alias").map(String::from).unwrap_or_else(|| tidy(&desc)),
            Kind::Hdmi => {
                // The nick is the screen's name from its EDID ("LG TV"); a bare "HDMI 0" says nothing.
                let n = nick.trim();
                if !n.is_empty() && !n.to_lowercase().starts_with("hdmi") && !n.to_lowercase().contains("digital") {
                    format!("{n} (HDMI)")
                } else {
                    "TV (HDMI)".to_string()
                }
            }
            Kind::Usb => get("device.product.name").map(String::from).unwrap_or_else(|| tidy(&desc)),
        };
        let (id, name) = match fronts.get(name).filter(|_| output) {
            Some((fid, fname)) => (*fid as u64, fname.as_str()),
            None => (id, name),
        };
        let d = Device { id: id as u32, name: name.to_string(), label, kind, default: false };
        if output {
            outs.push(d);
        } else {
            ins.push(d);
        }
    }
    // The configured default when it is present, else the current one.
    for (list, (cur, conf)) in [(&mut outs, sink_def), (&mut ins, src_def)] {
        let pick = conf.filter(|c| list.iter().any(|d| &d.name == c)).or(cur);
        for d in list.iter_mut() {
            d.default = pick.as_deref() == Some(d.name.as_str());
        }
        list.sort_by_key(|d| (d.kind, d.id));
    }
    (outs, ins)
}

fn dump() -> (Vec<Device>, Vec<Device>) {
    match crate::system::run_for("pw-dump", &[], LIMIT) {
        Ok(j) => parse_dump(&j),
        Err(_) => (vec![], vec![]),
    }
}

pub fn outputs() -> Vec<Device> {
    dump().0
}

pub fn inputs() -> Vec<Device> {
    dump().1
}

/// `wpctl get-volume` -> (level 0..=100, muted).
fn parse_volume(out: &str) -> Option<(u8, bool)> {
    let v: f64 = out.split_whitespace().nth(1)?.parse().ok()?;
    Some(((v * 100.0).round().clamp(0.0, 100.0) as u8, out.contains("MUTED")))
}

fn mic() -> Option<(u8, bool)> {
    parse_volume(&crate::system::run_for("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"], LIMIT).ok()?)
}

/// Outputs, inputs, the mic and "Louder speakers" in one go (one pw-dump, one wpctl). Also puts the
/// saved "Louder speakers" choice on the speaker chain when the chain is new (first reading, or
/// PipeWire reloaded it).
pub fn read() -> Audio {
    let j = crate::system::run_for("pw-dump", &[], LIMIT).unwrap_or_default();
    let (outputs, inputs) = parse_dump(&j);
    let chain = crate::speakers::chain_id(&j);
    crate::speakers::ensure(chain);
    let m = if inputs.is_empty() { None } else { mic() };
    Audio { outputs, inputs, mic_muted: m.map(|m| m.1), mic_level: m.map(|m| m.0), louder: chain.map(|_| crate::speakers::louder()) }
}

/// Makes an output the default (by PipeWire id or node.name). WirePlumber moves the playing
/// streams with it.
pub fn set_default_output(target: &str) -> Result<(), String> {
    let id = match target.parse::<u32>() {
        Ok(i) => i,
        Err(_) => outputs().into_iter().find(|d| d.name == target).map(|d| d.id).ok_or_else(|| "That output is gone".to_string())?,
    };
    crate::system::run_for("wpctl", &["set-default", &id.to_string()], LIMIT).map(|_| ())
}

pub fn mic_muted() -> Option<bool> {
    mic().map(|m| m.1)
}

pub fn set_mic_muted(muted: bool) -> Result<(), String> {
    crate::system::run_for("wpctl", &["set-mute", "@DEFAULT_AUDIO_SOURCE@", if muted { "1" } else { "0" }], LIMIT).map(|_| ())
}

pub fn mic_level() -> Option<u8> {
    mic().map(|m| m.0)
}

pub fn set_mic_level(level: u8) -> Result<(), String> {
    crate::system::run_for("wpctl", &["set-volume", "@DEFAULT_AUDIO_SOURCE@", &format!("{}%", level.min(100))], LIMIT).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id": 30, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
       "metadata": [
         {"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON", "value": {"name": "alsa_output.pci-0000_04_00.6.analog-stereo"}},
         {"subject": 0, "key": "default.configured.audio.sink", "type": "Spa:String:JSON", "value": {"name": "bluez_output.AA_BB_CC_DD_EE_FF.1"}},
         {"subject": 0, "key": "default.audio.source", "type": "Spa:String:JSON", "value": "{\"name\":\"alsa_input.pci-0000_04_00.6.analog-stereo\"}"},
         {"subject": 0, "key": "default.configured.audio.source", "type": "Spa:String:JSON", "value": {"name": "gone.source"}}
       ]},
      {"id": 40, "type": "PipeWire:Interface:Device", "info": {"props": {"device.api": "alsa", "device.bus": "pci", "device.description": "Family 17h HD Audio Controller"},
       "params": {"Route": [{"index": 3, "direction": "Output", "name": "analog-output-headphones", "description": "Headphones"}]}}},
      {"id": 41, "type": "PipeWire:Interface:Device", "info": {"props": {"device.api": "bluez5", "device.alias": "WH-1000XM4 Headphones"}}},
      {"id": 42, "type": "PipeWire:Interface:Device", "info": {"props": {"device.api": "alsa", "device.bus": "usb", "device.product.name": "USB Audio"}}},
      {"id": 50, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_04_00.6.analog-stereo", "node.description": "Family 17h HD Audio Controller Analog Stereo", "device.id": 40}}},
      {"id": 51, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_04_00.1.hdmi-stereo", "node.description": "Rembrandt Radeon High Definition Audio Controller Digital Stereo (HDMI)", "node.nick": "LG TV SSCR2"}}},
      {"id": 52, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "bluez_output.AA_BB_CC_DD_EE_FF.1", "node.description": "WH-1000XM4", "device.id": 41}}},
      {"id": 53, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.usb-Generic_USB_Audio-00.analog-stereo", "node.description": "USB Audio Analog Stereo", "device.id": 42}}},
      {"id": 54, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "auto_null", "node.description": "Dummy Output"}}},
      {"id": 55, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "node.name": "Firefox"}}},
      {"id": 60, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Source", "node.name": "alsa_input.pci-0000_04_00.6.analog-stereo", "node.description": "Family 17h HD Audio Controller Analog Stereo", "device.id": 40}}},
      {"id": 61, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Source", "node.name": "alsa_output.pci-0000_04_00.6.analog-stereo.monitor"}}},
      {"id": 62, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Source/Virtual", "node.name": "echo-cancel-source"}}}
    ]"#;

    #[test]
    fn parses_outputs_inputs_and_defaults() {
        let (outs, ins) = parse_dump(DUMP);
        let got: Vec<(u32, &str, Kind, bool)> = outs.iter().map(|d| (d.id, d.label.as_str(), d.kind, d.default)).collect();
        assert_eq!(
            got,
            vec![
                (50, "Headphones", Kind::Headphones, false),
                (52, "WH-1000XM4 Headphones", Kind::Bluetooth, true),
                (51, "LG TV SSCR2 (HDMI)", Kind::Hdmi, false),
                (53, "USB Audio", Kind::Usb, false),
            ]
        );
        assert_eq!(ins.len(), 1);
        assert_eq!(ins[0].label, "Built-in mic");
        // The configured source is gone, so the current one is the default (string-encoded value).
        assert!(ins[0].default);
        assert_eq!(outs[1].short(), "WH-1000XM4…");
        assert_eq!(outs[2].short(), "HDMI");
    }

    #[test]
    fn plain_speaker_and_bare_hdmi() {
        let j = r#"[
          {"id": 5, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_04_00.6.analog-stereo", "node.description": "Built-in Audio Analog Stereo"}}},
          {"id": 6, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_04_00.1.hdmi-stereo-extra1", "node.description": "HDMI / DisplayPort 2 Output", "node.nick": "HDMI 1"}}}
        ]"#;
        let (outs, ins) = parse_dump(j);
        assert_eq!(outs.iter().map(|d| d.label.as_str()).collect::<Vec<_>>(), vec!["Speaker", "TV (HDMI)"]);
        assert!(ins.is_empty());
        assert!(outs.iter().all(|d| !d.default));
        assert_eq!(parse_dump("not json"), (vec![], vec![]));
    }

    #[test]
    fn a_speaker_chain_stands_in_for_its_sink() {
        let j = r#"[
          {"id": 30, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
           "metadata": [{"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON", "value": {"name": "rare_speaker_eq"}}]},
          {"id": 37, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "rare_speaker_eq", "node.description": "Rare Speakers", "node.virtual": true, "node.link-group": "filter-chain-1-29"}}},
          {"id": 38, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "node.name": "rare_speaker_eq_out", "node.link-group": "filter-chain-1-29", "target.object": "alsa_output.pci-0000_c4_00.6.analog-stereo"}}},
          {"id": 62, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_c4_00.6.analog-stereo", "node.description": "Ryzen HD Audio Controller Analog Stereo"}}},
          {"id": 70, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "bluez_output.AA.1", "node.description": "Buds", "device.api": "bluez5"}}}
        ]"#;
        let (outs, _) = parse_dump(j);
        let got: Vec<(u32, &str, &str, bool)> = outs.iter().map(|d| (d.id, d.name.as_str(), d.label.as_str(), d.default)).collect();
        assert_eq!(got, vec![(37, "rare_speaker_eq", "Speaker", true), (70, "bluez_output.AA.1", "Buds", false)]);
        assert_eq!(crate::speakers::chain_id(j), Some(37));
    }

    #[test]
    fn reads_wpctl_volume() {
        assert_eq!(parse_volume("Volume: 0.40"), Some((40, false)));
        assert_eq!(parse_volume("Volume: 1.00 [MUTED]"), Some((100, true)));
        assert_eq!(parse_volume(""), None);
    }
}
