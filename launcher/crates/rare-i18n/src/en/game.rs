strings! {
    /// Launch-failure sheet: appended to a sentence when the game exited with a code; code is a number
    exit_code: " (exit code {code})",
    /// Launch-failure sheet title; name is the game
    fail_title: "{name} didn't start.",
    /// Launch-failure sheet text, Steam game that Steam did not open
    fail_steam: "Steam didn't open it. Open Steam to see why: an update, a sign-in or an error.",
    fail_steamui: "Steam didn't open. Try again, or restart the device if it keeps happening.",
    fail_never: "Heroic didn't start it. Open Heroic in Desktop mode to see why.",
    fail_stuck: "It's still running but nothing showed up in 45 seconds. Try again closes it and starts it again.",
    /// code is the exit-code suffix above, or empty
    fail_quick: "It closed before showing anything{code}. Try again, or check it in Desktop mode.",
    fail_closed: "It closed unexpectedly{code}.",
    /// Launch-failure sheet, Epic and GOG games: where the store tool's, umu-run's and Proton's output is; path is a file path
    fail_log: "Details: {path}",
    /// Button while a game's runtimes are installed (short: button)
    setting_up: "Setting up",
    /// Button: pause a download (short: button)
    pause: "Pause",
    /// Button: sync save files with the store's cloud (short: button)
    sync_saves: "Sync saves",
    /// Value while a size or status is being looked up
    checking: "Checking",
    /// Info card label on a Steam game's page (Steam's own feature name)
    steam_cloud: "Steam Cloud",
    /// Under the Steam Cloud card; when is a relative time like "2 days ago"
    last_sync: "Last sync {when}",
    cloud_synced: "Synced",
    cloud_not_synced: "Not synced yet",
    cloud_conflict: "Conflict",
    cloud_conflict_sub: "Steam asks which save to keep",
    cloud_not_used: "Not used",
    /// Playtime line, minutes under an hour; n is a number
    min_played: "{n} min played",
    /// Playtime line; n is whole hours
    h_played: "{n} h played",
    /// Under a game tile; source is a store name like "Steam"
    source_not_installed: "{source}, not installed",
    /// Playtime on the Playtime card, minutes under an hour; n is a number
    min: "{n} min",
    /// Playtime on the Playtime card; n is hours with one decimal, e.g. 49.2
    h: "{n} h",
    /// Toast when the player picks the "Play on" chip of the page they are on; label is the copy's name (e.g. "Steam")
    copy_toast: "This page is the {label} copy. Pick another to switch.",
    /// Status line above the game's logo while it runs
    now_playing: "Now playing",
    /// Label of the row of chips that switch between copies of one game (also a hint)
    play_on: "Play on",
    /// Button while the game runs: closes it (short: button)
    quit_game: "Quit game",
    /// Button: move the game to another drive (verb, short: button)
    move_: "Move",
    /// Hint of the Move button
    move_hint: "Move to another drive",
    /// Button on a game stored in another Steam library: adds that library to Steam
    add_library: "Add library to Steam",
    /// Button: opens the game inside the Steam app
    open_in_steam: "Open in Steam",
    /// Button: opens the game's Steam store page (short: button)
    store_page: "Store page",
    store_page_hint: "Open the Steam store page",
    /// Button on an Android game: pad buttons as screen touches
    touch_controls: "Touch controls",
    /// Same button when n touch points are mapped
    touch_controls_n: "Touch controls ({n})",
    touch_hint: "Buttons as screen touches",
    /// Info card labels and values on the game page
    last_played: "Last played",
    not_yet: "Not yet",
    playtime: "Playtime",
    none_yet: "None yet",
    size: "Size",
    install_size: "Install size",
    /// Size value of a game that is streamed and takes no disk space
    streamed: "Streamed",
    plays_web: "Plays from the web",
    plays_cloud: "Plays from the cloud",
    /// Info card label: which store the game is from
    store: "Store",
    /// Toasts after pressing X on a game
    fav_added: "Added to favourites",
    fav_removed: "Removed from favourites",
    /// Controller hint: X removes the game from favourites (verb)
    unfavourite: "Unfavourite",
    /// Proton install progress line
    phase_lookup: "Looking up the release",
    phase_verify: "Checking the checksum",
    phase_unpack: "Unpacking",
    /// Proton choice button: Valve's experimental build (short: button)
    proton_experimental: "Experimental",
    /// Toast; name is a browser name
    opening_in: "Opening in {name}",
    browser_failed: "Couldn't open the browser.",
    proton_wait: "Wait for the Proton change to finish.",
    proton_close_first: "Close the game first, then change its Proton.",
    /// Toast; name is a Proton version name
    proton_already: "{name} is already this game's Proton",
    proton_set: "Proton for this game: {name}",
    /// Toast; name is a Proton flavour such as GE-Proton
    proton_installed: "{name} installed and selected",
    proton_removed: "{name} removed",
    proton_failed: "Couldn't finish that.",
    /// Button: downloads a Proton flavour; name such as GE-Proton (short: button)
    proton_install: "Install {name}",
    /// Button: removes an out-of-date Proton; name is its label
    proton_remove_old: "Remove old {name}",
    /// Status text while the Proton change is applied
    restarting_steam: "Restarting Steam",
    saving: "Saving",
    /// Heading of the emulator systems list on a game page (ES-DE systems)
    systems: "Systems",
    /// Heading of the recent ROMs list
    played_last: "Played last",
    /// Button; name is a browser name
    open_in: "Open in {name}",
    /// Bar caption while a Steam game starts
    starting_steam: "Starting Steam",
    starting_steam_updating: "Starting Steam, it is updating",
    starting_game: "Starting the game",
    /// Saves dates: time is HH:MM; shown after cap(), e.g. "Today 21:40"
    today_at: "today {time}",
    yesterday_at: "yesterday {time}",
    /// day is a number, month a short name, time HH:MM: "12 Oct 09:15"
    date_at: "{day} {month} {time}",
    /// Saves card / sheet status
    saves_not_found: "Not found",
    saves_not_found_sub: "Rare can't find this game's saves yet",
    /// when is a date like "today 21:40"
    kept_here: "Kept here {when}",
    cloud_saves_off: "Cloud saves are off",
    changed_both: "Changed here and in the cloud",
    uploads_after: "Uploads after you play",
    synced_when: "Synced {when}",
    /// Info card label for Rare's own cloud saves
    cloud_saves: "Cloud saves",
    /// Why a saved version exists (second line of a row)
    reason_played: "After playing",
    reason_download: "Before a cloud download",
    reason_restore: "Before a restore",
    reason_conflict: "Before choosing a save",
    reason_cloud: "From the cloud",
    reason_restored: "Restored",
    reason_kept: "Kept copy",
    /// Where a saved version lives
    place_both: "This device and cloud",
    this_device: "This device",
    cloud: "Cloud",
    /// Sheet status sentence: a status, then a note, and the same with no note
    status_line_sub: "{value}. {sub}.",
    status_line: "{value}.",
    turn_on_body: "Turn on cloud saves to keep this game's saves on Walrus and bring them to your other devices. One wallet signature on this device, then Rare syncs before and after you play.",
    waiting_wallet: "Waiting for the wallet",
    /// Button: turns on cloud saves
    turn_on_cloud_saves: "Turn on cloud saves",
    /// Button: opens the sheet to pick between two saves
    choose_save: "Choose a save",
    no_funds: "No funds for uploads. Add WAL and SUI to keep syncing.",
    /// n is a number of uploads
    funds_about: "Funds for about {n} uploads",
    /// Button: add tokens for uploads
    add_funds: "Add funds",
    /// Heading of the list of saved versions
    versions: "Versions",
    looking_for_saves: "Looking for saves",
    no_copies: "No copies yet. Rare keeps one after each session.",
    /// Right side of a version row: this is the save in use
    current: "Current",
    press_again: "Press again to restore",
    /// Controller hint on a version row (verb)
    restore: "Restore",
    /// Saves sheet title; game is the game's name
    saves_title: "{game} saves",
    saves_sub: "Kept on this device after every session, and in the cloud when cloud saves are on",
    save_in_use: "This is the save in use. Pick an older one to go back to it.",
    wait_sync: "Wait for the current save sync to finish.",
    /// Hints on the two cards of the conflict sheet
    keep_this: "Keep this one",
    use_this: "Use this one",
    /// device and cloud are times like "today 21:40"
    conflict_line: "This device: {device} / Cloud: {cloud}",
    conflict_note: "The one you don't pick is kept in Saves, so you can switch back.",
    conflict_title: "Which save do you want to keep?",
    conflict_sub: "{game} changed on this device and in the cloud",
    /// Fallback name of a controller action (the Capture action)
    action_capture: "Capture",
    /// Row label on the game page (settings applied when the game starts)
    launch_options: "Launch options",
    /// Button while Steam's launch-option file is read
    reading_steam: "Reading Steam",
    no_steam: "Steam isn't set up on this device.",
    /// Button; value is the saved arguments (or "None")
    arguments_value: "Arguments: {value}",
    /// Button; value is the saved environment variables (or "None")
    environment_value: "Environment: {value}",
    /// Value of LC or RC when it follows Settings > Controls; action is the action's name
    action_global: "{action} (Controls)",
    /// Row label
    controls: "Controls",
    /// Buttons for the left and right controller clicks (LC, RC stay as the buttons' names); value is an action
    lc_value: "LC: {value}",
    rc_value: "RC: {value}",
    saved_next_start: "Saved. Used the next time the game starts.",
    /// Toast; bad is the text the player typed
    env_bad: "{bad} is not NAME=value. Nothing was saved.",
    close_running_first: "Close the running game first.",
    steam_opts_saved: "Launch options saved in Steam",
    /// Text-entry titles
    steam_opts_title: "Steam launch options",
    args_title: "Arguments for this game",
    env_title: "Environment for this game",
    /// Choice in the LC/RC list; action is the name of the action set in Settings > Controls
    same_as_controls: "Same as Controls ({action})",
    /// Sheet title; button is LC or RC, game is the game's name
    button_in_game: "{button} in {game}",
    used_while_running: "Used while this game runs",
    /// Performance sheet: upscaling sharpness levels
    sharp_soft: "Soft",
    sharp_medium: "Medium",
    sharp_sharp: "Sharp",
    /// Sharpening strength levels
    level_low: "Low",
    level_high: "High",
    /// Colour filter looks
    colour_natural: "Natural",
    colour_vivid: "Vivid",
    colour_bold: "Bold",
    /// Power chips; watts is a number with a W, e.g. "15 W"
    power_quiet_w: "Quiet {watts}",
    power_balanced_w: "Balanced {watts}",
    power_performance_w: "Performance {watts}",
    power_custom_w: "Custom {watts}",
    /// Row labels in the Performance sheet
    row_power: "Power",
    row_frame_limit: "Frame limit",
    row_upscaling: "Upscaling",
    row_sharpness: "Sharpness",
    row_render_size: "Render size",
    row_refresh_rate: "Refresh rate",
    row_frame_generation: "Frame generation",
    row_flow_scale: "Flow scale",
    row_sharpening: "Sharpening",
    row_colour: "Colour",
    /// Refresh rate chip: use the screen's own rate
    screen_default: "Screen default",
    /// Notes under frame generation in the Performance sheet
    fx_no_layer: "Frame generation comes with the next Rare OS update.",
    fx_no_dll: "Frame generation runs on Lossless Scaling, which you buy and install from Steam. Then pick its lsfg-vk beta in Steam: Properties, Betas.",
    fx_old_branch: "Lossless Scaling is installed. If frame generation does not start, pick its lsfg-vk beta in Steam: Properties, Betas.",
    fx_ready: "Frame generation works in Vulkan and Proton games. Turn it on before the game starts; 2x, 3x and Off then switch while you play.",
    fx_filters_next: "Sharpening, colour and ReShade apply the next time the game starts.",
    fx_reshade_dir: "For ReShade shaders, put .fx files in ~/.local/share/rare/reshade/Shaders.",
    /// Row label on the game page for Rename, Art, Check files...
    manage: "Manage",
    /// Button: opens the Performance sheet; also the sheet's title
    performance: "Performance",
    performance_hint: "This game's performance",
    /// Button and hint: verify the game's files
    check_files: "Check files",
    /// Button: rename the game (verb)
    rename: "Rename",
    /// Button: change the game's cover art (noun)
    art: "Art",
    art_hint: "Change art",
    /// Button after a file check found a difference: keep this copy as the reference
    keep_good_copy: "Keep as the good copy",
    hz_note: "Refresh rates other than the screen's own work on screens that support them, like a TV or monitor.",
    /// Section heading in the Performance sheet
    fx_head: "Frame generation and filters",
    /// Button: opens Lossless Scaling's page in the Steam store
    lossless_in_steam: "Lossless Scaling in Steam",
    /// Button: clears this game's own performance settings
    use_rare_settings: "Use Rare's settings",
    /// Sheet subtitle; game is the game's name
    perf_sub: "Used each time {game} starts. Rare's own settings come back when it closes.",
    steam_open_failed: "Couldn't open Steam. Try again.",
    /// Toast when a chosen chip is already on; label is a row name, value the chip
    already_set: "{label} is already {value}",
    why_sharpness: "Pick FSR or NIS under Upscaling first.",
    why_flow: "Turn on frame generation first.",
    why_framegen: "Frame generation isn't ready on this device yet.",
    why_update: "This needs the next Rare OS update.",
    close_first: "Close the game first.",
    /// Text-entry title and placeholder for renaming
    rename_title: "Name for this game",
    rename_hint: "Leave empty for the original name",
    save_failed: "Couldn't save that setting.",
    name_back: "The original name is back",
    /// Toast; name is the new name
    renamed_to: "Renamed to {name}",
    rename_failed: "Couldn't rename it.",
    /// Runs on SuiPlay: how a player rates a game (noun/adjective, short)
    verdict_great: "Great",
    verdict_playable: "Playable",
    verdict_broken: "Broken",
    /// Sheet rows: what each rating means
    rating_great: "Runs well, nothing to fix",
    rating_playable: "Runs, with a tweak or a few issues",
    rating_broken: "Does not start, or is not playable",
    /// HowLongToBeat card columns: hours for the main story, with extras, everything
    hltb_main: "Main story",
    hltb_extras: "With extras",
    hltb_all: "Everything",
    /// Card title (HowLongToBeat data)
    how_long: "How long to beat",
    /// m is a Metacritic score number
    metacritic: "{m} Metacritic",
    /// p is a percentage number
    positive_steam: "{p}% positive on Steam",
    critics_score: "Critics' score",
    positive: "{p}% positive",
    /// label is Steam's rating words, e.g. "Very Positive"
    label_on_steam: "{label} on Steam",
    steam_reviews: "Steam reviews",
    /// Info card label for review scores
    reviews: "Reviews",
    /// Section heading under the fold
    about_game: "About this game",
    runs_on_suiplay: "Runs on SuiPlay",
    /// n is a formatted number of reports
    reports_n: "{n} report|{n} reports",
    no_reports: "No reports yet",
    nobody_said: "Nobody has said how this game runs on a SuiPlay yet. Play it, then tell others: Great, Playable or Broken, with the settings you used.",
    /// n is a number of reports, verdict is Great, Playable or Broken
    best_from: "Best settings, from {n} {verdict} report|Best settings, from {n} {verdict} reports",
    /// Button: opens the report sheet (the player already reported)
    change_report: "Change your report",
    /// Button: opens the report sheet
    report_how: "Report how it runs",
    /// Button: applies the community's best settings to this game
    use_settings: "Use these settings",
    use_settings_hint: "Use these settings for this game",
    /// verdict is Great, Playable or Broken
    you_said: "You said {verdict}",
    sending: "Sending",
    your_report: "Your report",
    reading_settings: "Reading this game's settings",
    /// settings is a short list of settings, e.g. "Balanced, 40 fps"
    sent_with: "Sent with your settings for this game: {settings}.",
    /// Report sheet title and subtitle
    how_runs: "How does it run?",
    on_this_suiplay: "{game} on this SuiPlay",
    opening_hltb: "Opening HowLongToBeat",
    using_settings: "Using these settings each time it starts",
    settings_save_failed: "Couldn't save the settings. Try again.",
    report_thanks: "Thanks. Your report is in.",
    /// Legend under the bar: verdict and its count
    legend_item: "{verdict} {n}",
    opening_manual: "Opening the manual",
    dlc_close_first: "Close the game first to change its DLC.",
    /// Toasts; name is the DLC's name
    dlc_on: "{name} is on",
    dlc_off: "{name} is off",
    shot_deleted: "Screenshot deleted",
    shot_delete_failed: "Couldn't delete that screenshot.",
    /// Controller hint in the screenshot viewer: previous picture
    previous: "Previous",
    press_again_delete: "Press again to delete",
    /// Controller hint and button: open the news post in the web browser
    open_in_browser: "Open in browser",
    /// Section headings under the game
    screenshots: "Screenshots",
    /// The game page's rail of the store page's screenshots
    store_shots: "From the store",
    /// Launch failure: an Epic game could not reach Epic to sign in
    store_fail_offline: "Epic can't be reached: no connection. Check Wi-Fi and try again.",
    /// Launch failure: the Epic sign-in expired
    store_fail_sign_in: "Epic needs you to sign in again: Settings > Accounts.",
    /// Launch failure: legendary does not list the game as installed
    store_fail_not_installed: "legendary doesn't have it as installed. Check files in the game's Manage row, or install it again.",
    /// Launch failure: the game needs an update first
    store_fail_out_of_date: "It needs an update first. Update it from its page.",
    updates: "Updates",
    news: "News",
    dlc: "DLC",
    manual: "Manual",
    /// DLC rail note; owned and n are numbers
    dlc_owned: "{owned} of {n} owned",
    dlc_in_store: "{n} in the store",
    game_manual: "Game manual",
    opens_in_browser: "Opens in the browser",
    /// Controller hint on a news card or the manual card (verb)
    read: "Read",
    /// DLC card status lines
    dlc_changing: "Changing, Steam restarts",
    dlc_turned_off: "Turned off",
    dlc_owned_one: "Owned",
    dlc_store_one: "In the Steam store",
    /// Hint on a DLC card
    view_in_steam: "View in Steam",
    /// Caption in the screenshot viewer; i is the picture's number, n the total
    viewer_pos: "{game} \u{b7} {i} of {n}",
    press_x_delete: "Press X again to delete",
    post_no_text: "This post has no text here. Y opens it in the browser.",
    /// News card tags made by Rare
    feed_patch_notes: "Patch notes",
    feed_announcement: "Announcement",
    feed_update: "Update",
    /// Price of a free DLC
    dlc_free: "Free",
    /// Relative time without a verb, e.g. under "Last played" and "Last sync"
    ago_just_now: "just now",
    ago_hours: "{n} h ago|{n} h ago",
    ago_yesterday: "yesterday",
    ago_days: "{n} day ago|{n} days ago",
}
