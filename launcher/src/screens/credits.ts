// Upstream projects and their licences. Shown only on the Credits and licences screen;
// full licence texts and the upstream project names ship in the image under
// /usr/share/rare/licenses/. Owner rule: some upstream names never appear in the UI, so
// those entries are described by what they are here.

export const CREDITS: Array<{ name: string; licence: string; use: string }> = [
  { name: "Bazzite and Universal Blue", licence: "Apache-2.0", use: "Base system image, handheld support and updates" },
  { name: "Fedora Linux", licence: "Various free licences", use: "Packages underneath the base image" },
  { name: "Linux kernel", licence: "GPL-2.0", use: "Kernel" },
  { name: "gamescope", licence: "BSD-2-Clause", use: "Game mode compositor" },
  { name: "gamescope-session-plus", licence: "MIT", use: "Game mode session" },
  { name: "InputPlumber", licence: "GPL-3.0", use: "Controller and button mapping (separate service)" },
  { name: "PowerStation", licence: "GPL-3.0", use: "Power limit control over D-Bus (separate service)" },
  { name: "SuiPlay0X1 hardware quirks", licence: "MIT", use: "Button and serial-port rules from the SuiPlay0X1's original OS" },
  { name: "Tauri", licence: "MIT / Apache-2.0", use: "Launcher app framework" },
  { name: "WebKitGTK", licence: "LGPL-2.1 / BSD", use: "Launcher rendering on Linux" },
  { name: "React", licence: "MIT", use: "Launcher interface" },
  { name: "Tailwind CSS", licence: "MIT", use: "Launcher styles" },
  { name: "gilrs", licence: "MIT / Apache-2.0", use: "Controller input" },
  { name: "Mysten Labs Sui TypeScript SDK", licence: "Apache-2.0", use: "Rare Wallet" },
  { name: "noble / scure cryptography", licence: "MIT", use: "Rare Wallet keys" },
  { name: "legendary", licence: "GPL-3.0", use: "Epic Games launching (separate program)" },
  { name: "heroic-gogdl / Heroic data", licence: "GPL-3.0", use: "GOG installs (read from disk)" },
  { name: "umu-launcher", licence: "GPL-3.0", use: "Running Windows games with Proton (separate program)" },
  { name: "Moonlight", licence: "GPL-3.0", use: "Streaming games from your PC (separate program, installed from Flathub)" },
  { name: "mdns-sd", licence: "MIT / Apache-2.0", use: "Finding gaming PCs on your network" },
  { name: "AreWeAntiCheatYet", licence: "MIT", use: "Anti-cheat status of Steam games on Linux" },
  { name: "Inter", licence: "SIL OFL 1.1", use: "Interface font" },
  { name: "Fira Code", licence: "SIL OFL 1.1", use: "Number font" },
];
