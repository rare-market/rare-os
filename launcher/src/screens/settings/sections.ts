// The sections of Settings, in the order of the left list.

export type SectionId = "system" | "display" | "sound" | "controls" | "power" | "network" | "accounts" | "storage" | "about" | "emulators" | "notifications" | "licences" | "developer";

export const SECTIONS: Array<{ id: SectionId; label: string }> = [
  { id: "system", label: "System" },
  { id: "display", label: "Display" },
  { id: "sound", label: "Sound" },
  { id: "controls", label: "Controls" },
  { id: "power", label: "Power" },
  { id: "network", label: "Network" },
  { id: "accounts", label: "Accounts" },
  { id: "storage", label: "Storage" },
  { id: "notifications", label: "Notifications" },
  { id: "emulators", label: "Emulators" },
  { id: "developer", label: "Developer" },
  { id: "about", label: "About" },
  { id: "licences", label: "Credits and licences" },
];

export function sectionLabel(id: SectionId): string {
  return SECTIONS.find((s) => s.id === id)?.label ?? id;
}
