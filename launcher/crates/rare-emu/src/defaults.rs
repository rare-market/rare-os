//! First-run emulator settings for the SuiPlay0X1 (780M, 1920x1200 landscape session, 60 Hz, InputPlumber pad).
//!
//! Each writer creates the emulator's config in its native format and path, and only when the
//! emulator has no config yet. An existing file is never read for merging and never touched, so the
//! user's own settings (and EmuDeck's) always win.
//!
//! Controller bindings are not here: controls.rs owns them (one pad, every button, no hotkey on the
//! pad; the Rare menu sends the hotkeys as keyboard keys).

use std::fs;
use std::path::{Path, PathBuf};

/// Where an emulator's files live. Tests point `home` at a fixture directory.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    /// SDL joystick GUID of the pad InputPlumber exposes (for emulators that bind by GUID).
    pub pad_guid: Option<String>,
    /// Its evdev name, for RPCS3 ("Microsoft X-Box One Elite 2 pad").
    pub pad_name: Option<String>,
    /// A Flatpak's own config and data folders (~/.var/app/<id>/config and data), when set.
    pub xdg: Option<(PathBuf, PathBuf)>,
}

impl Env {
    pub fn real() -> Env {
        let home = dirs::home_dir().unwrap_or_default();
        let pad = std::fs::read_to_string("/proc/bus/input/devices").ok().and_then(|t| pad_info(&t));
        Env { home, pad_guid: pad.as_ref().map(|p| p.0.clone()), pad_name: pad.map(|p| p.1), xdg: None }
    }
    /// The same player and pad, with a Flatpak's folders.
    pub fn flatpak(&self, app: &str) -> Env {
        let base = self.home.join(".var/app").join(app);
        Env { xdg: Some((base.join("config"), base.join("data"))), ..self.clone() }
    }
    pub fn config(&self, rel: &str) -> PathBuf {
        match &self.xdg {
            Some((c, _)) => c.join(rel),
            None => self.home.join(".config").join(rel),
        }
    }
    pub fn data(&self, rel: &str) -> PathBuf {
        match &self.xdg {
            Some((_, d)) => d.join(rel),
            None => self.home.join(".local/share").join(rel),
        }
    }
}

/// What a writer did.
#[derive(Debug, Default, PartialEq, Eq, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Files created.
    pub written: Vec<String>,
    /// Files that already existed and were left alone.
    pub kept: Vec<String>,
    /// Things this emulator cannot be given, with the reason.
    pub notes: Vec<String>,
}

impl Report {
    fn put(&mut self, path: &Path, content: &str) -> std::io::Result<()> {
        if path.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            self.kept.push(path.to_string_lossy().into_owned());
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("rare-tmp");
        fs::write(&tmp, content)?;
        fs::rename(&tmp, path)?;
        self.written.push(path.to_string_lossy().into_owned());
        Ok(())
    }
}

/// SDL joystick GUID of the virtual Xbox-style pad (InputPlumber's), from /proc/bus/input/devices text.
/// Prefers a virtual device (sysfs under /devices/virtual) with a js handler, then any Xbox-style pad.
pub fn pad_guid(proc_devices: &str) -> Option<String> {
    pad_info(proc_devices).map(|p| p.0)
}

/// (SDL GUID, evdev name) of that pad.
pub fn pad_info(proc_devices: &str) -> Option<(String, String)> {
    struct Dev {
        bus: u16,
        vendor: u16,
        product: u16,
        version: u16,
        name: String,
        sysfs: String,
        js: bool,
    }
    let mut devs: Vec<Dev> = Vec::new();
    for block in proc_devices.split("\n\n") {
        let mut d = Dev { bus: 0, vendor: 0, product: 0, version: 0, name: String::new(), sysfs: String::new(), js: false };
        for line in block.lines() {
            if let Some(r) = line.strip_prefix("I: ") {
                for kv in r.split_whitespace() {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    let v = u16::from_str_radix(v, 16).unwrap_or(0);
                    match k {
                        "Bus" => d.bus = v,
                        "Vendor" => d.vendor = v,
                        "Product" => d.product = v,
                        "Version" => d.version = v,
                        _ => {}
                    }
                }
            } else if let Some(r) = line.strip_prefix("N: Name=") {
                d.name = r.trim_matches('"').to_string();
            } else if let Some(r) = line.strip_prefix("S: Sysfs=") {
                d.sysfs = r.to_string();
            } else if let Some(r) = line.strip_prefix("H: Handlers=") {
                d.js = r.split_whitespace().any(|h| h.starts_with("js"));
            }
        }
        let n = d.name.to_ascii_lowercase();
        if d.js && (n.contains("x-box") || n.contains("xbox")) {
            devs.push(d);
        }
    }
    let pick = devs.iter().find(|d| d.sysfs.starts_with("/devices/virtual")).or_else(|| devs.first())?;
    let le = |v: u16| format!("{:02x}{:02x}", v & 0xff, v >> 8);
    Some((format!("{}0000{}0000{}0000{}0000", le(pick.bus), le(pick.vendor), le(pick.product), le(pick.version)), pick.name.clone()))
}

// ---------------------------------------------------------------------------------------------
// DuckStation  ~/.local/share/duckstation/settings.ini
// ---------------------------------------------------------------------------------------------

/// PS1 is trivial for a 780M: 4x internal (about 1280x960), PGXP on, threaded presentation.
pub fn duckstation(env: &Env, app_dir: Option<&Path>) -> std::io::Result<Report> {
    let mut r = Report::default();
    // Portable mode: a settings.ini or portable.txt next to the AppImage is its config.
    if let Some(d) = app_dir {
        for f in ["settings.ini", "portable.txt"] {
            if d.join(f).exists() {
                r.kept.push(d.join(f).to_string_lossy().into_owned());
                return Ok(r);
            }
        }
    }
    let ini = "\
[Main]
SettingsVersion = 3
EmulationSpeed = 1.000000
FastForwardSpeed = 0.000000
SyncToHostRefreshRate = false
StartFullscreen = true
PauseOnFocusLoss = false
ConfirmPowerOff = false
SetupWizardIncomplete = false
NoDesktopFile = true

[GPU]
Renderer = Vulkan
ResolutionScale = 4
UseThread = true
ThreadedPresentation = true
TrueColor = true
PGXPEnable = true
PGXPTextureCorrection = true
PGXPCulling = true

[InputSources]
SDL = true
SDLControllerEnhancedMode = true
";
    r.put(&env.data("duckstation/settings.ini"), ini)?;
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// PCSX2  ~/.config/PCSX2/inis/PCSX2.ini
// ---------------------------------------------------------------------------------------------

/// 2x native (about 1280x960), Vulkan (Renderer 14), MTVU on, frame limiter at the console's 60.
pub fn pcsx2(env: &Env) -> std::io::Result<Report> {
    let mut r = Report::default();
    let ini = "\
[UI]
SettingsVersion = 1
StartFullscreen = true
ConfirmShutdown = false
PauseOnFocusLoss = false
CheckForUpdates = false

[EmuCore]
EnableCheats = false

[EmuCore/Speedhacks]
vuThread = true

[EmuCore/GS]
Renderer = 14
upscale_multiplier = 2
VsyncEnable = false
FrameLimitEnable = true
SyncToHostRefreshRate = false
texture_preloading = 2
AspectRatio = Auto 4:3/3:2

[Framerate]
NominalScalar = 1
TurboScalar = 2
SlomoScalar = 0.5

[InputSources]
Keyboard = true
Mouse = true
SDL = true
SDLControllerEnhancedMode = true
";
    r.put(&env.config("PCSX2/inis/PCSX2.ini"), ini)?;
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// RPCS3  ~/.config/rpcs3/config.yml + input_configs/global/Default.yml
// ---------------------------------------------------------------------------------------------

/// 100% resolution scale at 720p output, async shader recompiler, LLVM recompilers, SPU block size Mega.
pub fn rpcs3(env: &Env) -> std::io::Result<Report> {
    let mut r = Report::default();
    let cfg = "\
Core:
  PPU Decoder: Recompiler (LLVM)
  SPU Decoder: Recompiler (LLVM)
  SPU loop detection: true
  SPU Block Size: Mega
  LLVM Precompilation: true
Video:
  Renderer: Vulkan
  Resolution: 1280x720
  Frame limit: Auto
  Shader Mode: Async Shader Recompiler
  Write Color Buffers: false
  Write Depth Buffer: false
  Read Color Buffers: false
  Read Depth Buffer: false
  VSync: false
  Multithreaded RSX: true
  Resolution Scale: 100
Miscellaneous:
  Exit RPCS3 when process finishes: true
  Start games in fullscreen mode: true
";
    // RPCS3's welcome box would sit over the first game Rare starts.
    r.put(&env.config("rpcs3/GuiConfigs/CurrentSettings.ini"), "[main_window]\ninfoBoxEnabledWelcome=false\n")?;
    r.put(&env.config("rpcs3/config.yml"), cfg)?;
    r.notes.push("RPCS3 has no mappable controller hotkeys for save state or fast-forward, so none are set.".into());
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// Cemu  ~/.config/Cemu/settings.xml + controllerProfiles/controller0.xml
// ---------------------------------------------------------------------------------------------

/// Vulkan, async shader compile, accurate barriers, GX2DrawDone sync off; pad 1 as the Wii U GamePad.
pub fn cemu(env: &Env) -> std::io::Result<Report> {
    let mut r = Report::default();
    let xml = "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<content>
  <check_update>false</check_update>
  <fullscreen>true</fullscreen>
  <disable_screensaver>true</disable_screensaver>
  <Graphic>
    <api>1</api>
    <VSync>1</VSync>
    <GX2DrawdoneSync>false</GX2DrawdoneSync>
    <AsyncCompile>true</AsyncCompile>
    <vkAccurateBarriers>true</vkAccurateBarriers>
  </Graphic>
</content>
";
    r.put(&env.config("Cemu/settings.xml"), xml)?;
    r.notes.push("Cemu has no save states and no controller hotkeys.".into());
    Ok(r)
}

fn cemu_pad(guid: &str) -> String {
    // Wii U button id -> SDL id, the layout Cemu's own Steam Deck profile uses for an Xbox-style pad.
    const MAP: &[(u32, u32)] = &[
        (1, 1), (2, 0), (3, 3), (4, 2), (5, 9), (6, 10), (7, 42), (8, 43), (9, 6), (10, 4), (11, 11), (12, 12), (13, 13), (14, 14),
        (15, 7), (16, 8), (17, 45), (18, 39), (19, 44), (20, 38), (21, 47), (22, 41), (23, 46), (24, 40), (25, 8),
    ];
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<emulated_controller>\n\t<type>Wii U GamePad</type>\n\t<controller>\n");
    s.push_str(&format!("\t\t<api>SDLController</api>\n\t\t<uuid>0_{guid}</uuid>\n\t\t<display_name>Rare pad</display_name>\n\t\t<rumble>1</rumble>\n"));
    for t in ["axis", "rotation", "trigger"] {
        s.push_str(&format!("\t\t<{t}>\n\t\t\t<deadzone>0.25</deadzone>\n\t\t\t<range>1</range>\n\t\t</{t}>\n"));
    }
    s.push_str("\t\t<mappings>\n");
    for (m, b) in MAP {
        s.push_str(&format!("\t\t\t<entry>\n\t\t\t\t<mapping>{m}</mapping>\n\t\t\t\t<button>{b}</button>\n\t\t\t</entry>\n"));
    }
    s.push_str("\t\t</mappings>\n\t</controller>\n</emulated_controller>\n");
    s
}

// ---------------------------------------------------------------------------------------------
// Azahar  ~/.config/azahar-emu/qt-config.ini
// ---------------------------------------------------------------------------------------------

fn q(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key}\\default=false\n{key}={value}\n"));
}

/// Vulkan, 3x (about 1200x720 per top screen), async shader compile, disk shader cache, limiter 100%.
pub fn azahar(env: &Env) -> std::io::Result<Report> {
    let mut r = Report::default();
    let mut s = String::from("[Renderer]\n");
    q(&mut s, "graphics_api", "2");
    q(&mut s, "resolution_factor", "3");
    q(&mut s, "async_shader_compilation", "true");
    q(&mut s, "async_presentation", "true");
    q(&mut s, "use_disk_shader_cache", "true");
    q(&mut s, "use_hw_shader", "true");
    q(&mut s, "use_frame_limit", "true");
    q(&mut s, "frame_limit", "100");
    q(&mut s, "texture_filter", "0");
    s.push_str("\n[UI]\n");
    q(&mut s, "fullscreen", "true");
    q(&mut s, "confirmClose", "false");
    q(&mut s, "check_for_update_on_start", "false");
    r.put(&env.config("azahar-emu/qt-config.ini"), &s)?;
    r.notes.push("Azahar's hotkeys are keyboard-only, so none are set.".into());
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// Citron  ~/.config/citron/qt-config.ini
// ---------------------------------------------------------------------------------------------

/// Handheld mode at 1x (1280x720), Vulkan, FIFO vsync (60), async shaders and GPU emulation, aggressive VRAM.
pub fn citron(env: &Env) -> std::io::Result<Report> {
    let mut r = Report::default();
    let mut s = String::from("[Core]\n");
    q(&mut s, "use_multi_core", "true");
    s.push_str("\n[Renderer]\n");
    q(&mut s, "backend", "1");
    q(&mut s, "resolution_setup", "2");
    q(&mut s, "use_vsync", "2");
    q(&mut s, "use_asynchronous_shaders", "true");
    q(&mut s, "use_asynchronous_gpu_emulation", "true");
    q(&mut s, "use_disk_shader_cache", "true");
    q(&mut s, "vram_usage_mode", "1");
    q(&mut s, "gpu_accuracy", "0");
    s.push_str("\n[System]\n");
    q(&mut s, "use_docked_mode", "0");
    s.push_str("\n[UI]\n");
    q(&mut s, "fullscreen", "true");
    q(&mut s, "confirmStop", "2");
    // A dialog over a game started from Rare has no pad focus: no update prompt at start.
    q(&mut s, "check_for_updates_on_start", "false");
    r.put(&env.config("citron/qt-config.ini"), &s)?;
    r.notes.push("The Switch has no save states; Citron's controller shortcuts are left at their defaults.".into());
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// Rare's earlier controller sections (before controls.rs), so they are recognised as Rare's and
// updated without asking. Never written any more.
// ---------------------------------------------------------------------------------------------

pub const LEGACY_DUCKSTATION_PAD: &str = "\
Type = AnalogController
Up = SDL-0/DPadUp
Right = SDL-0/DPadRight
Down = SDL-0/DPadDown
Left = SDL-0/DPadLeft
Triangle = SDL-0/Y
Circle = SDL-0/B
Cross = SDL-0/A
Square = SDL-0/X
Select = SDL-0/Back
Start = SDL-0/Start
L1 = SDL-0/LeftShoulder
R1 = SDL-0/RightShoulder
L2 = SDL-0/+LeftTrigger
R2 = SDL-0/+RightTrigger
L3 = SDL-0/LeftStick
R3 = SDL-0/RightStick
LLeft = SDL-0/-LeftX
LRight = SDL-0/+LeftX
LDown = SDL-0/+LeftY
LUp = SDL-0/-LeftY
RLeft = SDL-0/-RightX
RRight = SDL-0/+RightX
RDown = SDL-0/+RightY
RUp = SDL-0/-RightY
SmallMotor = SDL-0/SmallMotor
LargeMotor = SDL-0/LargeMotor

";
pub const LEGACY_DUCKSTATION_HOTKEYS: &str = "\
LoadSelectedSaveState = SDL-0/Back & SDL-0/LeftShoulder
SaveSelectedSaveState = SDL-0/Back & SDL-0/RightShoulder
FastForward = SDL-0/Back & SDL-0/+RightTrigger
SelectPreviousSaveStateSlot = SDL-0/Start & SDL-0/LeftShoulder
SelectNextSaveStateSlot = SDL-0/Start & SDL-0/RightShoulder
";
pub const LEGACY_PCSX2_PAD: &str = "\
Type = DualShock2
Deadzone = 0.000000
AxisScale = 1.330000
Up = SDL-0/DPadUp
Right = SDL-0/DPadRight
Down = SDL-0/DPadDown
Left = SDL-0/DPadLeft
Select = SDL-0/Back
Start = SDL-0/Start
L1 = SDL-0/LeftShoulder
L2 = SDL-0/+LeftTrigger
R1 = SDL-0/RightShoulder
R2 = SDL-0/+RightTrigger
L3 = SDL-0/LeftStick
R3 = SDL-0/RightStick
LUp = SDL-0/-LeftY
LRight = SDL-0/+LeftX
LDown = SDL-0/+LeftY
LLeft = SDL-0/-LeftX
RUp = SDL-0/-RightY
RRight = SDL-0/+RightX
RDown = SDL-0/+RightY
RLeft = SDL-0/-RightX
SmallMotor = SDL-0/SmallMotor
LargeMotor = SDL-0/LargeMotor
Triangle = SDL-0/FaceNorth
Circle = SDL-0/FaceEast
Cross = SDL-0/FaceSouth
Square = SDL-0/FaceWest
";
pub const LEGACY_PCSX2_HOTKEYS: &str = "\
LoadStateFromSlot = SDL-0/Back & SDL-0/LeftShoulder
SaveStateToSlot = SDL-0/Back & SDL-0/RightShoulder
HoldTurbo = SDL-0/Back & SDL-0/+RightTrigger
NextSaveStateSlot = SDL-0/Start & SDL-0/RightShoulder
PreviousSaveStateSlot = SDL-0/Start & SDL-0/LeftShoulder

";

/// Rare's earlier Cemu controller file.
pub fn legacy_cemu_pad(guid: &str) -> String {
    cemu_pad(guid)
}

/// Rare's earlier RPCS3 pad file.
pub fn legacy_rpcs3_pad(name: &str) -> String {
    format!(
        "\
Player 1 Input:
  Handler: Evdev
  Device: {name} 0
  Config:
    Left Stick Left: LX-
    Left Stick Down: LY+
    Left Stick Right: LX+
    Left Stick Up: LY-
    Right Stick Left: RX-
    Right Stick Down: RY+
    Right Stick Right: RX+
    Right Stick Up: RY-
    Start: Start
    Select: Select
    PS Button: Mode
    Square: X
    Cross: A
    Circle: B
    Triangle: Y
    Left: Hat0 X-
    Down: Hat0 Y+
    Right: Hat0 X+
    Up: Hat0 Y-
    R1: TR
    R2: RZ+
    R3: Thumb R
    L1: TL
    L2: LZ+
    L3: Thumb L
    Left Stick Deadzone: 30
    Right Stick Deadzone: 30
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC: &str = "\
I: Bus=0003 Vendor=045e Product=028e Version=0200
N: Name=\"Microsoft X-Box 360 pad\"
S: Sysfs=/devices/pci0000:00/usb1/1-4/input/input6
H: Handlers=event5 js0 

I: Bus=0003 Vendor=045e Product=0b00 Version=0001
N: Name=\"Microsoft X-Box One Elite 2 pad\"
S: Sysfs=/devices/virtual/input/input18
H: Handlers=kbd event10 js1 

I: Bus=0003 Vendor=1234 Product=5678 Version=0111
N: Name=\"InputPlumber Keyboard\"
S: Sysfs=/devices/virtual/input/input19
H: Handlers=sysrq kbd event11 
";

    fn fixture() -> (tempfile::TempDir, Env) {
        let t = tempfile::tempdir().unwrap();
        let env = Env { home: t.path().to_path_buf(), pad_guid: pad_guid(PROC), pad_name: pad_info(PROC).map(|p| p.1), xdg: None };
        (t, env)
    }

    fn read(env: &Env, rel: &str) -> String {
        std::fs::read_to_string(env.home.join(rel)).unwrap()
    }

    #[test]
    fn guid_prefers_the_virtual_pad() {
        assert_eq!(pad_guid(PROC).as_deref(), Some("030000005e040000000b000001000000"));
        // Only the physical pad: its own GUID, same shape as the device's citron config.
        let only = PROC.split("\n\n").next().unwrap();
        assert_eq!(pad_guid(only).as_deref(), Some("030000005e0400008e02000000020000"));
        assert_eq!(pad_guid("I: Bus=0003 Vendor=1 Product=2 Version=3\nN: Name=\"Mouse\"\nH: Handlers=mouse0\n"), None);
    }

    #[test]
    fn duckstation_writes_vulkan_and_hotkeys_once() {
        let (_t, env) = fixture();
        let r = duckstation(&env, None).unwrap();
        assert_eq!(r.written.len(), 1);
        let ini = read(&env, ".local/share/duckstation/settings.ini");
        assert!(ini.contains("Renderer = Vulkan") && ini.contains("ResolutionScale = 4") && ini.contains("PGXPEnable = true"));
        // The pad and its hotkeys are controls.rs's.
        assert!(!ini.contains("[Pad1]") && !ini.contains("[Hotkeys]"));
        // A second run keeps the file, and the user's edits survive.
        std::fs::write(env.home.join(".local/share/duckstation/settings.ini"), "[GPU]\nRenderer = OpenGL\n").unwrap();
        let r = duckstation(&env, None).unwrap();
        assert!(r.written.is_empty() && r.kept.len() == 1);
        assert!(read(&env, ".local/share/duckstation/settings.ini").contains("OpenGL"));
    }

    #[test]
    fn duckstation_portable_is_left_alone() {
        let (t, env) = fixture();
        let apps = t.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("portable.txt"), "").unwrap();
        let r = duckstation(&env, Some(&apps)).unwrap();
        assert!(r.written.is_empty());
        assert!(!env.home.join(".local/share/duckstation").exists());
    }

    #[test]
    fn pcsx2_renderer_and_scale() {
        let (_t, env) = fixture();
        pcsx2(&env).unwrap();
        let ini = read(&env, ".config/PCSX2/inis/PCSX2.ini");
        assert!(ini.contains("Renderer = 14") && ini.contains("upscale_multiplier = 2") && ini.contains("vuThread = true"));
        assert!(!ini.contains("[Pad1]") && !ini.contains("SDL-0"));
    }

    #[test]
    fn existing_pcsx2_ini_is_never_overwritten() {
        let (_t, env) = fixture();
        let p = env.home.join(".config/PCSX2/inis");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("PCSX2.ini"), "[UI]\nSettingsVersion = 1\n").unwrap();
        let r = pcsx2(&env).unwrap();
        assert!(r.written.is_empty());
        assert_eq!(std::fs::read_to_string(p.join("PCSX2.ini")).unwrap(), "[UI]\nSettingsVersion = 1\n");
    }

    #[test]
    fn rpcs3_writes_config_and_pad() {
        let (_t, env) = fixture();
        let r = rpcs3(&env).unwrap();
        assert_eq!(r.written.len(), 2);
        assert!(read(&env, ".config/rpcs3/GuiConfigs/CurrentSettings.ini").contains("infoBoxEnabledWelcome=false"));
        let c = read(&env, ".config/rpcs3/config.yml");
        assert!(c.contains("Renderer: Vulkan") && c.contains("Shader Mode: Async Shader Recompiler") && c.contains("Resolution Scale: 100"));
        assert!(c.contains("SPU Block Size: Mega"));
        // Valid YAML shape: every non-empty line is a key, indented by an even number of spaces.
        for l in c.lines() {
            assert!(l.trim_end().ends_with(':') || l.contains(": "), "{l}");
            assert_eq!((l.len() - l.trim_start().len()) % 2, 0);
        }
    }

    #[test]
    fn cemu_settings() {
        let (_t, env) = fixture();
        assert_eq!(cemu(&env).unwrap().written.len(), 1);
        let x = read(&env, ".config/Cemu/settings.xml");
        assert!(x.contains("<api>1</api>") && x.contains("<AsyncCompile>true</AsyncCompile>") && x.contains("<GX2DrawdoneSync>false"));
        assert_eq!(legacy_cemu_pad("g").matches("<entry>").count(), 25);
    }

    #[test]
    fn azahar_and_citron_qt_format() {
        let (_t, env) = fixture();
        azahar(&env).unwrap();
        let a = read(&env, ".config/azahar-emu/qt-config.ini");
        assert!(a.contains("graphics_api\\default=false\ngraphics_api=2\n"));
        assert!(a.contains("resolution_factor=3") && a.contains("async_shader_compilation=true"));
        citron(&env).unwrap();
        let c = read(&env, ".config/citron/qt-config.ini");
        assert!(c.contains("backend=1") && c.contains("resolution_setup=2") && c.contains("use_asynchronous_shaders=true"));
        // Re-running keeps both files.
        assert!(azahar(&env).unwrap().written.is_empty());
        assert!(citron(&env).unwrap().written.is_empty());
    }
}
