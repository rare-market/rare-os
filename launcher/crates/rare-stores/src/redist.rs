//! GOG redistributables: the runtimes a GOG game lists as dependencies (Visual C++, DirectX,
//! .NET, OpenAL, PhysX...), installed into the game's prefix once, after its install and again
//! before its first start when that didn't happen (a game from Heroic, a new prefix).
//!
//! The list comes from `gogdl info` (kept per game in `~/.config/rare/stores/gog-deps.json`). The
//! installers come from GOG's public dependency store through `gogdl redist`, shared by every game
//! in `~/.local/share/rare/gog-redist`; each runs silently through umu-run in the game's prefix with
//! the game's Proton. What a prefix has is kept in `<prefix>/.rare-redist.json`, so a new prefix gets
//! them again. GOG's script interpreter (ISI) and entries with no installer are skipped.

use crate::tools::{self, GOGDL, UMU};
use crate::{read_json, rare_dir, write_json};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

fn deps_path() -> PathBuf {
    rare_dir().join("gog-deps.json")
}

pub fn save_deps(id: &str, deps: Vec<String>) {
    let mut v = read_json(&deps_path()).unwrap_or_else(|| json!({}));
    if let Some(o) = v.as_object_mut() {
        o.insert(id.to_string(), json!(deps));
    }
    write_json(&deps_path(), &v);
}

/// The game's dependencies, from the saved list or (once) from `gogdl info`.
pub fn deps_of(id: &str) -> Vec<String> {
    let saved = read_json(&deps_path()).and_then(|v| v.get(id).cloned());
    if let Some(Value::Array(a)) = saved {
        return a.iter().filter_map(|d| d.as_str().map(String::from)).collect();
    }
    if crate::gog::account().is_some() && crate::gog::info(id).is_ok() {
        return deps_of_saved(id);
    }
    Vec::new()
}

fn deps_of_saved(id: &str) -> Vec<String> {
    read_json(&deps_path()).and_then(|v| v.get(id).and_then(Value::as_array).cloned()).unwrap_or_default().iter().filter_map(|d| d.as_str().map(String::from)).collect()
}

/// Where the installers are kept (all games share them).
pub fn store() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share")).join("rare/gog-redist")
}

fn marks_file(game: &str) -> PathBuf {
    rare_library::umu::prefix(&format!("gog:{game}")).join(".rare-redist.json")
}

/// The dependencies already in the game's prefix.
pub fn installed(game: &str) -> Vec<String> {
    read_json(&marks_file(game)).and_then(|v| v.as_array().cloned()).unwrap_or_default().iter().filter_map(|d| d.as_str().map(String::from)).collect()
}

/// What still has to go into the prefix (ISI never does).
pub fn needed(game: &str, deps: &[String]) -> Vec<String> {
    let have = installed(game);
    deps.iter().filter(|d| d.as_str() != "ISI" && !have.contains(d)).cloned().collect()
}

/// A Windows command line's arguments: split on spaces outside double quotes, quotes dropped.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The installer and its silent arguments for a dependency, from gogdl's redist manifest.
pub fn installer(manifest: &Value, dep: &str) -> Option<(String, String)> {
    let d = manifest.get("depots")?.as_array()?.iter().find(|d| d.get("dependencyId").and_then(Value::as_str) == Some(dep))?;
    let path = d.pointer("/executable/path")?.as_str()?.to_string();
    if path.is_empty() {
        return None;
    }
    Some((path, d.pointer("/executable/arguments").and_then(Value::as_str).unwrap_or("").to_string()))
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Puts what the game needs into its prefix. `deps`: None reads the game's list. `step(name)` is
/// told each installer as it starts. Returns the dependencies installed now.
pub fn ensure(game: &str, deps: Option<Vec<String>>, step: &mut dyn FnMut(&str, usize, usize)) -> Result<Vec<String>, String> {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let deps = deps.unwrap_or_else(|| deps_of(game));
    let todo = needed(game, &deps);
    if todo.is_empty() {
        return Ok(vec![]);
    }
    if !tools::have(UMU) {
        return Err("umu-run is missing. Update Rare and try again.".into());
    }
    let dir = store();
    let _ = std::fs::create_dir_all(&dir);
    let ids = todo.join(",");
    tools::run(GOGDL, &["--auth-config-path", &crate::gog::auth_path().to_string_lossy(), "redist", "--ids", &ids, "--path", &dir.to_string_lossy()], &[], Duration::from_secs(1800))?;
    let manifest = read_json(&dir.join(".gogdl-redist-manifest")).ok_or("GOG's runtimes didn't download. Try again.")?;
    let id = format!("gog:{game}");
    let prefix = rare_library::umu::prefix(&id);
    let _ = std::fs::create_dir_all(&prefix);
    let env = rare_library::umu::env(&id);
    let mut done = installed(game);
    let mut now = Vec::new();
    for (i, dep) in todo.iter().enumerate() {
        let Some((rel, args)) = installer(&manifest, dep) else {
            // Nothing to run (a DOSBox or ScummVM build the game ships itself).
            done.push(dep.clone());
            continue;
        };
        let exe = dir.join(&rel);
        if !exe.is_file() {
            log::warn!("redist {dep}: {} is missing", exe.display());
            continue;
        }
        step(dep, i, todo.len());
        let mut a: Vec<String> = Vec::new();
        if rel.to_ascii_lowercase().ends_with(".msi") {
            a.extend(["msiexec".into(), "/i".into()]);
        }
        a.push(exe.to_string_lossy().into_owned());
        a.extend(split_args(&args));
        let out = tools::run_with(UMU, &a, &env, Some(Duration::from_secs(1200)), &mut |_| {}, &mut |_| {})?;
        if !out.ok {
            // Installers often end with "restart needed" or "already newer"; the game decides.
            log::warn!("redist {dep} for {game}: {}", out.error());
        }
        done.push(dep.clone());
        now.push(dep.clone());
        write_json(&marks_file(game), &json!(done));
    }
    write_json(&marks_file(game), &json!(done));
    Ok(now)
}

/// True when the game's prefix still lacks something it lists (without asking GOG).
pub fn pending(game: &str) -> bool {
    !needed(game, &deps_of_saved(game)).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_and_installers() {
        assert_eq!(split_args("/install /quiet /norestart"), vec!["/install", "/quiet", "/norestart"]);
        assert_eq!(split_args(r#"/q:a /c:"msiexec /i x.msi""#), vec!["/q:a", "/c:msiexec /i x.msi"]);
        assert!(split_args("").is_empty());
        let m = json!({"depots":[{"dependencyId":"openAL","executable":{"path":"__redist/openAL/oalinst.exe","arguments":"/S"}},{"dependencyId":"DOSBox074","executable":{"path":"","arguments":""}}]});
        assert_eq!(installer(&m, "openAL"), Some(("__redist/openAL/oalinst.exe".into(), "/S".into())));
        assert_eq!(installer(&m, "DOSBox074"), None);
        assert_eq!(installer(&m, "nope"), None);
    }
}
