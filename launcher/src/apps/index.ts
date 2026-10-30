// Apps: Telegram, X, Netflix, YouTube, Discord and Chrome as a console media row (Home) and a
// Library filter. Native side: src-tauri/src/apps. docs/APPS.md has how each one opens and how
// the controller drives apps that don't read it.

export { APPS, appById, appByRunId, runId } from "./catalog";
export { useApps } from "./state";
export { AppTile, AppsGrid, AppsRail } from "./tiles";
export { AppSheet, RunningSheet } from "./sheets";
