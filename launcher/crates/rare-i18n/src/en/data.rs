strings! {
    /// Library source label: games found on this device
    source_local: "Local",
    /// Library source label: browser games
    source_web: "Web",
    /// Library source label: the emulator apps
    source_emulators: "Emulators",
    /// Library source label: cloud-streamed games
    source_cloud: "Cloud",
    /// Library source label: emulated games (ROMs)
    source_emulation: "Emulation",
    /// Under a game
    never_played: "Never played",
    /// Under a game
    played_just_now: "Played just now",
    /// Under a game; h is hours
    played_hours_ago: "Played {n} h ago",
    /// Under a game
    played_yesterday: "Played yesterday",
    /// Under a game
    played_days_ago: "Played {n} days ago",
    /// Under a game; month is a short name like Jan
    played_on: "Played {month} {day}",
    /// Under a game; month is a short name like Jan
    played_on_year: "Played {month} {day}, {year}",
    /// Short month name
    month_jan: "Jan",
    /// Short month name
    month_feb: "Feb",
    /// Short month name
    month_mar: "Mar",
    /// Short month name
    month_apr: "Apr",
    /// Short month name
    month_may: "May",
    /// Short month name
    month_jun: "Jun",
    /// Short month name
    month_jul: "Jul",
    /// Short month name
    month_aug: "Aug",
    /// Short month name
    month_sep: "Sep",
    /// Short month name
    month_oct: "Oct",
    /// Short month name
    month_nov: "Nov",
    /// Short month name
    month_dec: "Dec",
    /// Short weekday name
    day_sun: "Sun",
    /// Short weekday name
    day_mon: "Mon",
    /// Short weekday name
    day_tue: "Tue",
    /// Short weekday name
    day_wed: "Wed",
    /// Short weekday name
    day_thu: "Thu",
    /// Short weekday name
    day_fri: "Fri",
    /// Short weekday name
    day_sat: "Sat",
    /// A date: short month name and day number
    month_day: "{month} {day}",
    /// Under a game tile; name is a PC
    pc_asleep: "{name} is asleep",
    /// Under a game tile; name is a PC
    pc_off: "{name} is off",
    /// Under a game tile
    running_on_pc: "Running on your PC",
    /// In a list of settings, lowercase
    no_frame_limit: "no frame limit",
    /// In a list of settings: upscaler and resolution, like FSR at 800p
    upscale_at: "{upscale} at {size}",
    /// Hour of day, 12-hour clock
    hour_am: "{h} am",
    /// Hour of day, 12-hour clock
    hour_pm: "{h} pm",
    /// A whole hour on a 24-hour clock ("Free until 18:00"); used for Russian, Spanish and Portuguese
    hour_24: "{h}:00",
    /// Free-game line; when is a day or hour
    play_free_from: "Play free from {when}",
    /// Free-game line; when is a day or hour
    free_from: "Free from {when}",
    /// Free-game line; when is a day or hour
    play_free_until: "Play free until {when}",
    /// Free-game line; when is a day or hour
    free_until: "Free until {when}",
    /// Free-game line
    play_free_weekend: "Play free this weekend",
    /// Free-game line
    free_to_keep: "Free to keep",
    /// Error text
    sign_in_to_report: "Sign in with Rare Wallet to send reports.",
    /// Error text
    not_a_rom: "That isn't a ROM.",
    /// Error text
    esde_rom_folder_missing: "ES-DE's ROM folder isn't set up.",
    /// Error text
    rom_gone: "That game isn't in its ROM folder any more.",
    /// Error text
    not_in_library: "It isn't in your library anymore.",
    /// Stage line while a game starts
    steam_needs_answer: "Steam needs an answer before the game can start",
    /// Error text
    steam_didnt_remove: "Steam didn't remove it. Try again.",
    /// Error text
    steam_didnt_uninstall: "Steam didn't uninstall it.",
    /// Error text
    steam_didnt_answer: "Steam didn't answer. Open Steam and try again.",
    /// Error text
    close_game_first: "Close the game first.",
    /// Error text
    steam_not_installed: "Steam doesn't have this game installed any more.",
    /// Error text
    no_folder_to_check: "This game has no folder to check.",
    /// Error text
    not_running: "That game isn't running.",
    /// Error text
    quit_then_uninstall: "Quit it first, then uninstall it.",
    /// Error text
    rom_gone_scan: "That game isn't in its ROM folder any more. Scan again from the Library.",
    /// Lowercase name used in a sentence ("Quit the Android app that's open first.")
    open_android_app: "the Android app that's open",
    /// Error text
    android_one_app: "Quit {name} first. Android runs one app at a time.",
    /// Error text
    android_busy: "Android is busy with an install. Try again when it's done.",
    /// Toast and error when a ROM's RetroArch core is fetched at its start; system is the system's name
    emu_core_getting: "Getting the {system} emulator for RetroArch…",
    emu_core_failed: "Couldn't get the {system} emulator for RetroArch. Check Wi-Fi, or set up emulation in Library, Emulation.",
    /// Notification title; title is a game
    price_drop: "Price drop: {title}",
    /// Notification body; prices include currency symbols
    price_drop_body: "{price} at {store}, was {old}",
    /// Lowercase fallback in a sentence when the store is unknown
    a_store: "a store",
    /// A price of zero
    free: "Free",
    /// Error text
    collection_name_empty: "Type a name for the collection.",
    /// Error text
    collection_name_taken: "You already have a collection with that name.",
    /// Error text
    collection_gone: "That collection is gone.",
    /// Game status label
    status_live: "Live",
    /// Game status label
    status_beta: "Beta",
    /// Game status label
    status_coming: "Coming soon",
    /// Game status label
    status_announced: "Announced",
    /// Game status label
    status_paused: "Paused",
    /// Platform label for a game
    platform_mobile: "Mobile",
    /// Compatibility label
    deck_verified: "Steam Deck Verified",
    /// Compatibility label
    deck_playable: "Steam Deck Playable",
    /// Compatibility label
    deck_unsupported: "Not supported on Steam Deck",
    /// ProtonDB rating name
    tier_platinum: "Platinum",
    /// ProtonDB rating name
    tier_gold: "Gold",
    /// ProtonDB rating name
    tier_silver: "Silver",
    /// ProtonDB rating name
    tier_bronze: "Bronze",
    /// ProtonDB rating name for a game that does not run
    tier_borked: "Borked",
    /// Compatibility label; tier is a rating name
    protondb_tier: "ProtonDB {tier}",
    /// Lowercase file kind in a ROM copy label (a ROM that is a folder)
    copy_folder: "folder",
    /// Copy label under Play on; system is like PS2, kind is a file type
    copy_emu: "{system} {kind}",
    /// Copy label under Play on; system is like PS2, kind is a file type
    copy_emu_sd: "{system} {kind} on SD card",
    /// Copy label under Play on; base is the store
    copy_not_installed: "{base} · Not installed",
    /// Toast
    picture_save_failed: "Couldn't save your picture. Try again.",
    /// Error text
    unknown_app: "Unknown app",
    /// Error text
    nothing_to_install: "Nothing to install",
    /// Error text
    no_game_running: "No game is running",
}
