// PC streaming types, shaped like the native side's (src-tauri/src/stream).

export type Codec = "auto" | "av1" | "hevc" | "h264";

export interface StreamSettings {
  /** "1920x1200" (the SuiPlay's panel) or "1280x800". */
  resolution: string;
  fps: number;
  bitrateMbps: number;
  codec: Codec;
}

/** Hardware video decode on this device (VA-API). */
export interface Decode {
  av1: boolean;
  hevc: boolean;
  h264: boolean;
}

/** A gaming PC (native `HostView`). */
export interface Host {
  uuid: string;
  name: string;
  address: string | null;
  /** Moonlight holds this PC's certificate. */
  paired: boolean;
  /** Answered just now. */
  online: boolean;
  /** The app running on the PC. */
  currentGame: number | null;
  /** Wake-on-LAN possible (a MAC address is known). */
  canWake: boolean;
  /** "Sunshine" or "GeForce Experience". */
  software: string | null;
  /** ServerCodecModeSupport bits. */
  codecs: number | null;
}

/** A game or app on a PC. `art` is Rare's cached box art (a local file, or a URL in the browser build). */
export interface StreamApp {
  id: number;
  name: string;
  art: string | null;
  hdr: boolean;
}

export interface Offer {
  flatpak: string;
  name: string;
  command: string;
}

export interface Status {
  /** "flatpak" or "native"; null when Moonlight isn't installed. */
  moonlight: string | null;
  offer: Offer | null;
  decode: Decode | null;
  settings: StreamSettings;
  hosts: Host[];
  /** Host uuid -> its apps. */
  apps: Record<string, StreamApp[]>;
}

export interface PairView {
  uuid: string;
  name: string;
  address: string;
  pin: string;
  phase: "waiting" | "paired" | "failed";
  error: string | null;
  software: string;
}
