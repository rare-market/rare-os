// Quick settings: the right-hand sheet (radios, sliders, performance, capture, power, Now playing).
strings! {
    /// Quick settings tile label
    airplane: "Airplane",
    /// Quick settings tile label (do not disturb mode)
    do_not_disturb: "Do not disturb",
    /// Tile state under Wi-Fi when the portal needs the player to sign in
    sign_in_needed: "Sign in needed",
    /// Slider labels
    brightness: "Brightness",
    volume: "Volume",
    night_dim: "Night dim. Darker to look at, but it does not save battery.",
    /// Section heading and the label of the profile row
    performance: "Performance",
    /// Power profile names (also fan presets for Quiet and Balanced); short: button
    profile_quiet: "Quiet",
    profile_balanced: "Balanced",
    profile_performance: "Performance",
    /// Toast; name is a power profile name
    profile_is_on: "{name} is on.",
    /// Performance overlay choices; short: chip
    hud_compact: "Compact",
    hud_full: "Full",
    performance_overlay: "Performance overlay",
    /// Fan presets; short: chip
    fan_max: "Max",
    fan_custom: "Custom",
    fan: "Fan",
    /// Row label: switch to apply these settings to one game; name is the game name, or "this game"
    use_for_game: "Use for {name}",
    this_game: "this game",
    use_for_saved: "Starts with these settings, overlay included",
    use_for_default: "Starts on Balanced",
    /// Controller hint when the More section is open
    show_less: "Show less",
    power_limit: "Power limit",
    cpu_boost: "CPU boost",
    cpu_boost_sub: "Faster CPU, more heat and battery",
    frame_limit: "Frame limit",
    /// Upscaling rows
    upscaling: "Upscaling",
    sharpness: "Sharpness",
    render_size: "Render size",
    /// Sharpness choices; short: chip
    sharp_soft: "Soft",
    sharp_medium: "Medium",
    sharp_sharp: "Sharp",
    /// Render size choice: the screen's own resolution; short: chip
    size_native: "Native",
    upscale_saved: "Saved. It applies when the next game starts.",
    /// Frame generation
    frame_generation: "Frame generation",
    flow_scale: "Flow scale",
    fg_no_layer: "Frame generation comes with the next Rare OS update.",
    fg_no_dll: "Frame generation needs your own copy of Lossless Scaling from Steam.",
    fg_old_branch: "If frame generation does not start, pick Lossless Scaling's lsfg-vk beta in Steam.",
    fg_ready: "Starts with the game's next launch. 2x, 3x and Off switch while you play.",
    /// Battery line next to the percentage
    battery_full: "Full",
    /// Time until charged or empty; time is like "3 h 10 min"
    full_in: "Full in {time}",
    time_left: "{time} left",
    /// Hours and minutes in the battery line; short
    hours_minutes: "{h} h {m} min",
    hours: "{h} h",
    minutes: "{m} min",
    /// Capture section
    capture: "Capture",
    screenshot: "Screenshot",
    take_screenshot: "Take a screenshot",
    save_clip: "Save the clip",
    instant_replay: "Instant replay",
    /// Button and controller hint that open the Share screen
    open_share: "Open Share",
    /// Power section: Sleep button
    power: "Power",
    sleep: "Sleep",
    /// Bottom buttons and hint (Y opens Steam)
    keyboard: "Keyboard",
    open_keyboard: "Open the keyboard",
    /// Row over a running game: what the controller is in it (value: the game page's pad_as_mouse / pad_as_gamepad)
    pad_in_game: "Controller in this game",
    all_settings: "All settings",
    open_steam: "Open Steam",
    /// Toasts for failed changes
    err_overlay: "Couldn't change the performance overlay.",
    err_frame_limit: "Couldn't change the frame limit.",
    err_framegen: "Couldn't change frame generation.",
    err_fan: "Couldn't change the fan.",
    err_cpu_boost: "Couldn't change CPU boost.",
    err_game_settings: "Couldn't save the settings for this game.",
    /// Sound rows
    output: "Output",
    microphone: "Microphone",
    mic_level: "Mic level",
    /// Microphone row state when it is muted
    muted: "Muted",
    err_mic_on: "Couldn't turn the mic on.",
    err_mic_mute: "Couldn't mute the mic.",
    err_output: "Couldn't switch the output.",
    /// Now playing card: media buttons (controller hints)
    previous: "Previous",
    pause: "Pause",
    player_no_response: "The player didn't respond.",
}
