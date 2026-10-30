//! Journey probe for tools/journeys: the logic lives in `rare_overlay::journey`; these are the
//! Tauri commands the UI calls.

#[tauri::command]
pub fn journey_probe() -> bool {
    rare_overlay::journey::journey_probe()
}

#[tauri::command]
pub fn journey_log(lines: Vec<String>) {
    rare_overlay::journey::journey_log(lines)
}

#[tauri::command]
pub fn journey_cmd() -> Option<String> {
    rare_overlay::journey::journey_cmd()
}
