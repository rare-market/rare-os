//! How WebKitGTK draws under gamescope (Rare OS game mode, or the launcher as a Steam shortcut).
//! Chosen once in `main`, before GTK or any thread starts, by `RARE_WEBKIT`:
//!
//! - `gpu`: X11 client of gamescope's Xwayland, DMA-BUF renderer on with shared-memory
//!   transport (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`). The web process rasterises and composites
//!   on the GPU (Skia GL, real WebGL), reads the frame back once and hands it over as SHM.
//!   On GTK3 + X11 WebKit's own EGL display is not shared with GDK (GLX), so without FORCE_SHM it
//!   takes the X11-only "mapping" path instead: linear GBM buffers, `gbm_bo_map` in the UI process
//!   and a flipped cairo paint (AcceleratedBackingStore.cpp, `BufferGBM`). That is the path that
//!   flashed, and it still copies on the CPU, so it buys nothing over SHM.
//!   It also sets `WEBKIT_DISABLE_DMABUF_ATLAS=1`. With GPU tile painting, WebKit 2.54 packs the
//!   raster images a tile draws into an atlas texture: a linear DMA-BUF from the texture pool that
//!   an `AtlasUpload` worker fills through a CPU mapping while the tile is replayed on a GPU painting
//!   thread (SkiaPaintingEngine::createAtlas, SkiaGPUAtlas::uploadImages). On the SuiPlay every
//!   image on the screen vanished for exactly one composited frame while the rest of the page stayed
//!   (2026-10-27 capture, tools/journeys/flash), the flash the owner saw. The variable keeps the
//!   atlas on the GL path (glTexSubImage plus a GL fence, uploaded before the tile paints); images
//!   are still drawn by the GPU.
//! - `gpuatlas`: `gpu` with WebKit's DMA-BUF image atlas, for A/B tests only.
//! - `cpu` (default): the proven no-flash path, `WEBKIT_DISABLE_DMABUF_RENDERER=1` (no GPU in the web process).
//! - `dmabuf`: X11 with WebKit's defaults (the flashing mapping path), for A/B tests only.
//! - `wayland`: native Wayland client of gamescope (`GDK_BACKEND=wayland`, `WAYLAND_DISPLAY` =
//!   `GAMESCOPE_WAYLAND_DISPLAY`). Zero copy: GTK3 on Wayland shares WebKit's EGL display, so the
//!   web process's dmabuf becomes a GL texture in GTK and GTK's EGL buffer goes to gamescope as a
//!   `wl_buffer` dmabuf, with no Xwayland upload. gamescope 3.17 (checked in its source):
//!   - serves `xdg_shell` on `gamescope-0` whatever its flags; `--expose-wayland` only exports
//!     `WAYLAND_DISPLAY`/`XDG_SESSION_TYPE=wayland` to every child (Steam, Proton, Chrome), which we
//!     do not want, so the session never passes it and only the launcher gets the socket here.
//!   - gives an xdg window an app id only from its client's cgroup (`GetAppIdFromCgroup`:
//!     `app-steam-app<id>-<n>.scope`). rare-session-client starts the launcher in
//!     `app-steam-app1380012613-<n>.scope`; without one (an older session client) the launcher
//!     re-executes itself in such a scope (`plan`, `prepare`). Steam and everything Rare starts get
//!     scopes of their own (launch.rs), so nothing else shares Rare's id.
//!   - matches `GAMESCOPECTRL_BASELAYER_WINDOW` against Xwayland windows only, so the overlay and
//!     sheets hold the screen through `GAMESCOPECTRL_BASELAYER_APPID` order
//!     (`overlay::gamescope::hold_front`), and orders two xdg windows of one app newest first, so
//!     Rare's other windows (web game, in-app browser) are hidden while the main window holds it.
//!   - never sizes xdg toplevels (no configure size, fullscreen requests are ignored), so the
//!     main window sizes itself to the output (`fit_output`).
//!   Not the default: on the SuiPlay (2026-10-27) it still flashed 9-22 times per 14 tab switches
//!   (the flashes are WebKit's accelerated compositing, not the X11 path) and used more CPU than
//!   `cpu` on a still screen.
//!
//! Variables set or already set by the caller win: an explicit `WEBKIT_DISABLE_DMABUF_RENDERER=1`
//! (an old device wrapper) keeps the CPU path. Every variable set here is listed in `SET_VAR` so
//! `launch::clean_env` keeps them away from Steam and games.

/// Rare's gamescope app id (overlay::gamescope::RARE_APPID, which this mirrors: `prepare` runs
/// first thing in `main`, before any host hook exists).
const RARE_APPID: u32 = 0x5241_5245;

/// The app id gamescope derives from a /proc/<pid>/cgroup text (gamescope's GetAppIdFromCgroup:
/// the last path element is `app-steam-app<appid>-<pid>.scope`).
fn appid_from_cgroup(cgroup: &str) -> Option<u32> {
    cgroup.lines().find_map(|line| {
        let path = line.splitn(3, ':').nth(2)?;
        let scope = path.rsplit('/').next()?;
        let rest = scope.strip_prefix("app-steam-app")?.strip_suffix(".scope")?;
        let (id, pid) = rest.split_once('-')?;
        pid.parse::<i32>().ok()?;
        id.parse::<u32>().ok().filter(|v| *v != 0)
    })
}

/// The variables this module set, comma separated, for `launch::clean_env`.
pub const SET_VAR: &str = "RARE_WEBKIT_SET";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Gpu,
    GpuAtlas,
    Cpu,
    DmaBuf,
    Wayland,
}

impl Mode {
    pub fn parse(v: &str) -> Option<Mode> {
        match v.trim().to_ascii_lowercase().as_str() {
            "gpu" | "shm" => Some(Mode::Gpu),
            "cpu" | "off" | "0" | "" => Some(Mode::Cpu),
            "gpuatlas" => Some(Mode::GpuAtlas),
            "dmabuf" => Some(Mode::DmaBuf),
            "wayland" => Some(Mode::Wayland),
            _ => None,
        }
    }
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Gpu => "gpu",
            Mode::GpuAtlas => "gpuatlas",
            Mode::Cpu => "cpu",
            Mode::DmaBuf => "dmabuf",
            Mode::Wayland => "wayland",
        }
    }
}

/// The renderer this process runs with, for the page (`render_mode`): a `Mode` name under
/// gamescope, else "desktop" (WebKit's defaults, nothing set here).
pub fn mode_name(get: &dyn Fn(&str) -> Option<String>) -> &'static str {
    if get("GAMESCOPE_WAYLAND_DISPLAY").filter(|v| !v.is_empty()).is_none() {
        return "desktop";
    }
    get("RARE_WEBKIT").and_then(|v| Mode::parse(&v)).unwrap_or(Mode::Cpu).name()
}

/// What to do: variables to set, and whether to re-execute inside an app-id scope first.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub set: Vec<(String, String)>,
    pub scope: Option<String>,
}

/// The plan for an environment (`get`) and this process's cgroup file and pid. Pure, for tests.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn plan(get: &dyn Fn(&str) -> Option<String>, cgroup: &str, pid: u32) -> Plan {
    let mut p = Plan::default();
    let Some(socket) = get("GAMESCOPE_WAYLAND_DISPLAY").filter(|v| !v.is_empty()) else { return p };
    let mode = get("RARE_WEBKIT").and_then(|v| Mode::parse(&v)).unwrap_or(Mode::Cpu);
    let mut set = |k: &str, v: &str| {
        if get(k).is_none() {
            p.set.push((k.to_string(), v.to_string()));
        }
    };
    match mode {
        Mode::Cpu => set("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
        Mode::Gpu => {
            set("WEBKIT_DISABLE_DMABUF_RENDERER", "0");
            set("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
            set("WEBKIT_DISABLE_DMABUF_ATLAS", "1");
        }
        Mode::GpuAtlas => {
            set("WEBKIT_DISABLE_DMABUF_RENDERER", "0");
            set("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
        }
        Mode::DmaBuf => set("WEBKIT_DISABLE_DMABUF_RENDERER", "0"),
        Mode::Wayland => {
            set("WEBKIT_DISABLE_DMABUF_RENDERER", "0");
            // GDK_BACKEND and WAYLAND_DISPLAY are forced: the session exports neither for us.
            p.set.push(("GDK_BACKEND".into(), "wayland".into()));
            p.set.push(("WAYLAND_DISPLAY".into(), socket.clone()));
            if get("RARE_SESSION_CLIENT").as_deref() == Some("1") &&appid_from_cgroup(cgroup).is_none() {
                p.scope = Some(format!("app-steam-app{}-{pid}", RARE_APPID));
            }
        }
    }
    let names: Vec<String> = p.set.iter().map(|(k, _)| k.clone()).collect();
    if !names.is_empty() {
        p.set.push((SET_VAR.into(), names.join(",")));
    }
    p
}

/// True when this process draws as a Wayland client of gamescope (`RARE_WEBKIT=wayland`).
pub fn wayland_client() -> bool {
    std::env::var("GDK_BACKEND").as_deref() == Ok("wayland") && std::env::var(SET_VAR).map(|v| v.split(',').any(|k| k == "GDK_BACKEND")).unwrap_or(false)
}

/// The size of gamescope's output as its Xwayland root has it (what an X11 fullscreen window
/// gets). For the Wayland main window, which gamescope never sizes.
#[cfg(target_os = "linux")]
pub fn output_size() -> Option<(u32, u32)> {
    use x11rb::connection::Connection;
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?;
    Some((root.width_in_pixels as u32, root.height_in_pixels as u32)).filter(|(w, h)| *w > 0 && *h > 0)
}

#[cfg(not(target_os = "linux"))]
pub fn output_size() -> Option<(u32, u32)> {
    None
}

/// Applies the plan for this process. Call first thing in `main`.
pub fn prepare() {
    #[cfg(target_os = "linux")]
    {
        let cgroup = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
        let get = |k: &str| std::env::var(k).ok();
        let p = plan(&get, &cgroup, std::process::id());
        for (k, v) in &p.set {
            std::env::set_var(k, v);
        }
        if let Some(unit) = p.scope {
            // Wayland windows get their app id from the cgroup. systemd-run --scope execs the
            // program in place (same pid, same AppImage mount). RARE_WEBKIT_SCOPED stops a loop.
            if std::env::var_os("RARE_WEBKIT_SCOPED").is_none() {
                use std::os::unix::process::CommandExt;
                if let Ok(exe) = std::env::current_exe() {
                    let err = std::process::Command::new("systemd-run")
                        .args(["--user", "--scope", "--collect", "--quiet"])
                        .arg(format!("--unit={unit}"))
                        .arg("--")
                        .arg(exe)
                        .args(std::env::args_os().skip(1))
                        .env("RARE_WEBKIT_SCOPED", "1")
                        .exec();
                    eprintln!("rare: could not enter scope {unit}: {err}; the launcher has no app id on Wayland");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(vars: &[(&str, &str)], cgroup: &str) -> Plan {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let get = move |k: &str| vars.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        plan(&get, cgroup, 42)
    }

    fn val<'a>(p: &'a Plan, k: &str) -> Option<&'a str> {
        p.set.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn nothing_outside_gamescope() {
        assert_eq!(run(&[("RARE_WEBKIT", "wayland")], ""), Plan::default());
    }

    #[test]
    fn cpu_is_the_default() {
        // Device proof 2026-10-27: X11 gpu flashed (12-15 frames per 14 tab switches) at ~100% of a
        // core; cpu had 0 flashes at 16.5%. cpu stays the default until the Wayland path is proven.
        let p = run(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0")], "");
        assert_eq!(val(&p, "WEBKIT_DISABLE_DMABUF_RENDERER"), Some("1"));
        assert_eq!(val(&p, "GDK_BACKEND"), None);
        assert_eq!(p.scope, None);
    }

    #[test]
    fn gpu_still_selectable() {
        let p = run(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "gpu")], "");
        assert_eq!(val(&p, "WEBKIT_DMABUF_RENDERER_FORCE_SHM"), Some("1"));
        assert_eq!(val(&p, "WEBKIT_DISABLE_DMABUF_ATLAS"), Some("1"));
    }

    #[test]
    fn gpuatlas_keeps_webkits_image_atlas() {
        let p = run(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "gpuatlas")], "");
        assert_eq!(val(&p, "WEBKIT_DMABUF_RENDERER_FORCE_SHM"), Some("1"));
        assert_eq!(val(&p, "WEBKIT_DISABLE_DMABUF_ATLAS"), None);
    }

    #[test]
    fn explicit_old_workaround_wins() {
        let p = run(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("WEBKIT_DISABLE_DMABUF_RENDERER", "1")], "");
        assert_eq!(val(&p, "WEBKIT_DISABLE_DMABUF_RENDERER"), None);
    }

    #[test]
    fn cpu_fallback() {
        let p = run(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "cpu")], "");
        assert_eq!(val(&p, "WEBKIT_DISABLE_DMABUF_RENDERER"), Some("1"));
        assert_eq!(val(&p, "WEBKIT_DMABUF_RENDERER_FORCE_SHM"), None);
    }

    #[test]
    fn wayland_enters_the_rare_scope_once() {
        let env = [("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "wayland"), ("RARE_SESSION_CLIENT", "1"), ("DISPLAY", ":0")];
        let p = run(&env, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/gamescope-session-plus@rare.service\n");
        assert_eq!(val(&p, "GDK_BACKEND"), Some("wayland"));
        assert_eq!(val(&p, "WAYLAND_DISPLAY"), Some("gamescope-0"));
        assert_eq!(p.scope.as_deref(), Some("app-steam-app1380012613-42"));
        let inside = run(&env, "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-steam-app1380012613-42.scope\n");
        assert_eq!(inside.scope, None);
    }

    #[test]
    fn wayland_as_a_steam_shortcut_keeps_steams_scope() {
        let p = run(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "wayland")], "0::/app.slice/app-steam-app3000-9.scope\n");
        assert_eq!(p.scope, None);
    }

    #[test]
    fn mode_names_for_the_page() {
        let name = |vars: &[(&str, &str)]| {
            let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            mode_name(&move |k: &str| vars.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()))
        };
        assert_eq!(name(&[]), "desktop");
        assert_eq!(name(&[("RARE_WEBKIT", "gpu")]), "desktop");
        assert_eq!(name(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0")]), "cpu");
        assert_eq!(name(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "shm")]), "gpu");
        assert_eq!(name(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"), ("RARE_WEBKIT", "nonsense")]), "cpu");
    }
}
