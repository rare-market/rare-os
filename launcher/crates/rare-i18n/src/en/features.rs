strings! {
/// Ask Rare: toast. Rare Wallet is the wallet app
ask_wallet_unavailable: "Rare Wallet isn't available.",
/// Ask Rare: toast when saving an API key and the wallet has no account
ask_key_needs_account: "Add an account with a PIN in Rare Wallet first. Your key is sealed with it.",
/// Ask Rare: PIN prompt text when saving an API key
ask_pin_save_key: "Enter your PIN to save your API key in Rare Wallet",
/// Ask Rare: toast
ask_key_saved: "Key saved in Rare Wallet",
/// Ask Rare: toast
ask_key_removed: "Key removed",
/// Ask Rare: toast; error is the reason
ask_key_save_failed: "Couldn't save the key: {error}",
/// Ask Rare: error when no AI provider is configured
ask_setup_first: "Set up Ask Rare in Settings > Ask Rare first.",
/// Ask Rare: PIN prompt text when using a stored API key
ask_pin_use_key: "Enter your PIN to use your API key",
/// Ask Rare: error when the PIN prompt was dismissed
ask_unlock_wallet: "Unlock Rare Wallet to use your API key.",
/// Ask Rare: toast after the Test button; provider is a service name, model a model id, word what it replied
ask_test_answer: "{provider} answered with {model}: \"{word}\"",
/// Ask Rare: label on a tool step that was cut short
ask_step_stopped: "Stopped",
/// Ask Rare: error when the launcher took too long to run a tool
ask_no_answer: "Rare didn't answer in time.",
/// Ask Rare: toast when push to talk is used with speech off
ask_turn_on_speech: "Turn on Speak to Rare in Settings > Ask Rare to talk.",
/// Ask Rare: toast after the speech model downloaded; X is a gamepad button
ask_speech_ready: "Speech is ready. Hold X to talk.",
/// Ask Rare: toast when nothing was heard; X is a gamepad button
ask_not_caught: "I didn't catch that. Hold X and speak.",
/// Ask Rare: tool error shown in the chat; SUI is a coin, keep it
ask_give_price: "Give the price in SUI, for example \"12.5\".",
/// Ask Rare: tool error; name is a tool id, keep it
ask_no_tool: "There is no tool called {name}.",
/// Ask Rare: step result of a game search that found nothing; query is what was searched
ask_nothing_called: "Nothing called \"{query}\"",
/// Ask Rare: step result, count of search matches
ask_matches: "{n} match|{n} matches",
/// Ask Rare: step result; name is a game or app title or a page name
ask_opened: "Opened {name}",
/// Ask Rare: tool error; install_game is a tool id, keep it
ask_not_installed_install: "{title} isn't installed. install_game opens its install sheet.",
/// Ask Rare: step result when the game was already running
ask_back_to: "Back to {title}",
/// Ask Rare: step result when a game is launching
ask_starting: "Starting {title}",
/// Ask Rare: step result; Rare Store is the store in the launcher
ask_opened_store_page: "Opened its Rare Store page",
/// Ask Rare: tool error
ask_already_installed: "{title} is already installed.",
/// Ask Rare: step result
ask_install_sheet: "Install sheet for {title}",
/// Ask Rare: tool error
ask_not_installed: "{title} isn't installed.",
/// Ask Rare: step result
ask_uninstall_sheet: "Uninstall sheet for {title}",
/// Ask Rare: step result
ask_nothing_downloading: "Nothing is downloading",
/// Ask Rare: step result, count of downloads
ask_downloads: "{n} download|{n} downloads",
/// Ask Rare: step result
ask_measuring_storage: "Measuring storage",
/// Ask Rare: step result; size is a number of gigabytes
ask_gb_free: "{size} GB free",
/// Ask Rare: step result
ask_read_settings: "Read settings",
/// Ask Rare: tool error; the three words are setting values the model passes, keep them
ask_power_profile_values: "power_profile is quiet, balanced or performance.",
/// Ask Rare: step result; profile is quiet, balanced or performance
ask_power_profile: "Power profile: {profile}",
/// Ask Rare: tool error; setting is brightness or volume
ask_number_0_100: "{setting} is a number from 0 to 100.",
/// Ask Rare: step result; n is a percent
ask_brightness_pct: "Brightness {n}%",
/// Ask Rare: step result; n is a percent
ask_volume_pct: "Volume {n}%",
/// Ask Rare: tool error; wifi is a setting id
ask_wifi_on_off: "wifi is on or off.",
/// Ask Rare: step result
ask_wifi_on: "Wi-Fi on",
/// Ask Rare: step result
ask_wifi_off: "Wi-Fi off",
/// Ask Rare: tool error; do_not_disturb is a setting id
ask_dnd_on_off: "do_not_disturb is on or off.",
/// Ask Rare: step result
ask_dnd_on: "Do not disturb on",
/// Ask Rare: step result
ask_dnd_off: "Do not disturb off",
/// Ask Rare: tool error
ask_setting_unchangeable: "That setting can't be changed here.",
/// Ask Rare: tool error
ask_unknown_page: "Unknown page.",
/// Ask Rare: tool error
ask_watch_only: "This Rare Wallet account is watch only.",
/// Ask Rare: tool error
ask_wallet_loading: "The wallet is still loading. Ask again in a moment.",
/// Ask Rare: step result, count of coins in the wallet
ask_coins: "{n} coin|{n} coins",
/// Ask Rare: tool error
ask_give_amount: "Give the amount as a number, for example \"2.5\".",
/// Ask Rare: tool error
ask_bad_recipient: "That recipient isn't a 0x address, a name.sui or a friend.",
/// Ask Rare: review sheet title; amount is a number, coin a symbol like SUI
ask_send_title: "Send {amount} {coin}",
/// Ask Rare: step result; to is a shortened address
ask_review_send: "Review sheet: {amount} {coin} to {to}",
/// Ask Rare: toast after a send went through
ask_sent: "Sent {amount} {coin}",
/// Ask Rare: toast
ask_tx_rejected: "The network rejected this transaction.",
/// Ask Rare: tool error; coin is what the model typed
ask_unknown_coin: "I don't know the coin {coin}. Give its coin type (0x...::module::NAME).",
/// Ask Rare: tool error
ask_two_coins: "Pick two different coins.",
/// Ask Rare: review sheet title for a swap
ask_swap_title: "Swap {from_amount} {from_coin} for {to_coin}",
/// Ask Rare: step result
ask_review_swap: "Review sheet: {from_amount} {from_coin} for about {to_amount} {to_coin}",
/// Ask Rare: toast after a swap went through
ask_swapped: "Swapped {from_amount} {from_coin} for {to_coin}",
/// Ask Rare: tool error
ask_items_loading: "Your items are still loading. Ask again in a moment.",
/// Ask Rare: step result, count of owned items
ask_items: "{n} item|{n} items",
/// Ask Rare: tool error; Rare's market is the Market tab
ask_market_unreachable: "Couldn't reach Rare's market.",
/// Ask Rare: step result, count of collections
ask_collections: "{n} collection|{n} collections",
/// Ask Rare: tool error
ask_item_unreadable: "Couldn't read that item.",
/// Ask Rare: tool error
ask_item_not_listed: "This item is not listed.",
/// Ask Rare: tool error
ask_item_not_buyable: "This item can't be bought here.",
/// Ask Rare: step result; price is a number of SUI
ask_review_buy: "Review sheet: {name} for {price} SUI",
/// Ask Rare: review sheet title; name is an item name
ask_buy_title: "Buy {name}",
/// Ask Rare: toast after a purchase
ask_bought: "Bought {name}.",
/// Ask Rare: step result; n friends, online is how many of them are online
ask_friends_online: "{n} friends, {online} online",
/// Ask Rare: tool error; names is a comma separated list
ask_not_friends: "None of those are friends: {names}.",
/// Ask Rare: step result; who is a list of names
ask_invited: "Invited {who}",
/// Ask Rare: tool error
ask_not_a_friend: "{name} isn't one of your friends.",
/// Ask Rare: tool error
ask_message_empty: "The message is empty.",
/// Ask Rare: step result
ask_message_to: "Message to {name}",
/// Ask Rare: tool error; search_games is a tool id, keep it
ask_id_not_in_library_short: "That id isn't in the library. Use search_games.",
/// Ask Rare: tool error; search_games is a tool id, keep it
ask_id_not_in_library: "That id isn't in the library. Use search_games to get the id.",
/// Ask Rare: tool error
ask_no_wallet_account: "There is no Rare Wallet account on this device.",
/// Ask Rare: tool error; coin is a symbol
ask_wallet_lacks: "The wallet doesn't hold {coin}.",
/// Ask Rare: tool error; coin is a symbol
ask_more_than_held: "That is more {coin} than the wallet holds.",
/// Ask Rare: tool error; Friends is the Friends tab
ask_friends_not_running: "Friends isn't running.",
/// Android (F-Droid) keyboard title when searching for apps
android_find_title: "Find Android games on F-Droid",
/// Android: keyboard placeholder for the search text
android_find_placeholder: "Game name",
/// Android: toast when an install starts; name is an app name
android_getting: "Getting {name}",
/// Android: toast; query is the search text
android_searching: "Searching F-Droid for {query}",
/// Android: toast; query is the search text
android_nothing_found: "Nothing on F-Droid for {query}",
/// Android: toast on a network error
android_unreachable: "Couldn't reach F-Droid. Check the network and try again.",
/// Android: sheet title listing F-Droid results; query is the search text
android_results_title: "F-Droid: {query}",
/// Android: sheet subtitle under the F-Droid results
android_results_sub: "Free and open source. Pick one to install.",
/// Android: Storage row note, why an Android app cannot be moved to another drive
android_apps_stay: "Android apps stay inside Android on this device.",
/// Android: Storage row note for Android's own system files
android_system_stays: "Android's system and data stay on this device.",
/// Android: error when an id is not an Android app
android_not_app: "Not an Android app",
/// Android touch controls: toast; error is the reason
android_touch_save_failed: "Couldn't save the touch controls. {error}",
/// Android touch controls: hint on the empty preview; Rare is the Rare menu button
android_touch_empty_canvas: "Open the game, press Rare, then come back here to see it behind the spots.",
/// Android touch controls: a button with no screen spot (value in a row)
android_not_set: "Not set",
/// Android touch controls: row hint while placing a spot (short: button)
android_set: "Set",
/// Android touch controls: row hint and gamepad hint to place a spot (short: button)
android_set_spot: "Set spot",
/// Android touch controls: remove a spot (short: button)
android_clear: "Clear",
/// Android touch controls: instruction while placing; button is a pad button name. A and B are gamepad buttons
android_placing: "Placing {button}: move with the d-pad, A to set, B to cancel",
/// Android touch controls: same instruction with fine steps on
android_placing_fine: "Placing {button}: move with the d-pad (fine), A to set, B to cancel",
/// Android touch controls: help line under the title
android_touch_help: "Each button presses its spot on the screen while you play. Buttons with no spot do nothing.",
/// Android touch controls: page title; game is the game's title
android_touch_title: "Touch controls \u{b7} {game}",
/// Android touch controls: hint to switch to bigger d-pad steps (short: hint)
android_coarse: "Coarse",
/// Android touch controls: hint to switch to smaller d-pad steps (short: hint)
android_fine: "Fine",
/// Same game as sheet: first row when a search was already done (short: sheet row)
copies_search_again: "Search again",
/// Same game as sheet: first row, opens the keyboard (short: sheet row)
copies_search_by_name: "Search by name",
/// Same game as: toast; query is the name typed
copies_no_match: "No game in your library matches {query}",
/// Same game as sheet subtitle; title is the game, query the typed text
copies_sub_names_with: "{title} \u{b7} names with {query}",
/// Same game as: sheet title (merge copies the matcher missed)
copies_same_game_as: "Same game as",
/// Same game as: keyboard prompt
copies_name_prompt: "Name of the other game",
/// Same game as: toast; Play on is the label of the copy picker on a game
copies_merged: "{title} shows as one game now. Pick a copy under Play on.",
/// Not the same game: sheet title
copies_which_different: "Which one is a different game?",
/// Not the same game: sheet subtitle
copies_own_tile: "It gets its own tile again",
/// Not the same game: toast with two game names
copies_split_two: "{a} and {b} show apart now",
/// Not the same game: toast; name is a game title
copies_split_one: "{name} shows on its own now",
/// Claim voucher error; what is a field name such as nonce, expiry, item kind (left in English)
claim_no_valid: "The voucher has no valid {what}",
/// Claim voucher error
claim_no_attrs: "The voucher has no valid attributes",
/// Claim voucher error
claim_attrs_mismatch: "The voucher's attributes don't line up",
/// Claim voucher error
claim_attr_twice: "The voucher names an attribute twice",
/// Claim voucher error
claim_sig_unreadable: "The voucher signature is not readable",
/// Claim voucher error
claim_unreadable: "This voucher is not readable",
/// Claim voucher error
claim_newer_version: "This voucher is from a newer version of Rare",
/// Claim voucher error
claim_changes_nothing: "The voucher changes nothing",
/// Claim voucher error
claim_unsupported: "This voucher is for something Rare can't claim yet",
/// Claim review sheet title for an item voucher
claim_title_item: "Claim an item",
/// Claim review sheet title for a voucher that updates an item the player owns
claim_title_state: "Update your item",
/// Claim review sheet title for a trophy voucher
claim_title_trophy: "Claim a trophy",
/// Claim sheet: one attribute of an item, key and value (like power 12)
claim_attr_pair: "{key} {value}",
/// Claim sheet: an attribute change, like power becomes 13
claim_attr_becomes: "{key} becomes {value}",
/// Claim sheet: what is claimed; n is the trophy id number
claim_trophy_n: "Trophy {n}",
/// Claim sheet: what is claimed; kind is a number
claim_item_kind: "Item kind {kind}",
/// Claim sheet: what is claimed; attrs is a comma separated list of attributes
claim_item_kind_with: "Item kind {kind} with {attrs}",
/// Claim voucher error
claim_expired: "This voucher has expired. Ask the game for a new one.",
/// Claim voucher error
claim_other_account: "This voucher is for another account",
/// Claim voucher error
claim_bad_class: "The voucher's item class isn't a Rare items class",
/// Claim voucher error
claim_no_policy: "The item class has no transfer policy",
/// Claim voucher error
claim_bad_item: "The voucher's item isn't a Rare item",
/// Claim voucher error
claim_no_class: "The item has no class",
/// Rare Pass (device pass): error when offline
pass_no_network: "No network",
/// Rare Pass: error; the attestor is Rare's server that vouches for the device
pass_bad_device: "The attestor sent a bad device id",
/// Rare Pass: error; the attestor is Rare's server that vouches for the device
pass_bad_signature: "The attestor sent a bad signature",
/// Rare Pass: error; the attestor is Rare's server that vouches for the device
pass_bad_sequence: "The attestor sent a bad sequence",
/// Rare Pass: toast after claiming
pass_claimed: "Rare Pass claimed",
/// Rare Pass: review sheet title for a first claim
pass_claim_title: "Claim Rare Pass",
/// Rare Pass: review sheet title when the pass moves to this device
pass_move_title: "Move this device's Rare Pass here",
/// Rare Pass: error shown on the pass screen
pass_unverified: "This device isn't running a verified Rare OS build.",
/// Rare Pass: error shown on the pass screen
pass_paused: "Rare Pass claims are paused right now.",
/// Rare Pass: error shown on the pass screen
pass_not_open: "Rare Pass isn't open on this network yet.",
/// Rare Pass: error shown on the pass screen
pass_wrong_deployment: "The attestor answered for a different Rare Pass deployment.",
/// Capture: clip length label, n minutes (short: button/row value)
capture_minutes_short: "{n} min",
/// Capture: clip length label, n seconds (short: button/row value)
capture_seconds_short: "{n} s",
/// Capture: Save clip button; length is like "30 s" or "1 min" (short: button)
capture_save_last: "Save last {length}",
/// Capture: a span used inside a sentence (it keeps the last 2 minutes), plural of minutes
capture_last_minutes: "the last {n} minute|the last {n} minutes",
/// Capture: a span used inside a sentence, exactly one minute
capture_last_minute: "the last minute",
/// Capture: a span used inside a sentence, n seconds
capture_last_seconds: "the last {n} second|the last {n} seconds",
/// Capture: toast; Settings, Capture and streaming is a settings page path
capture_pair_request: "A device wants to pair. Type its PIN in Settings, Capture and streaming.",
/// Capture: toast; last is a span like "the last 30 seconds"
capture_replay_on: "Instant replay is on. It keeps {last} while a game runs.",
/// Capture: toast
capture_replay_off: "Instant replay is off",
/// Capture: toast after the storage cap was lowered; n clips deleted, gb the cap in GB
capture_clips_deleted: "Deleted {n} older clip to fit {gb} GB|Deleted {n} older clips to fit {gb} GB",
/// Capture: toast
capture_mic_on: "Discord microphone on",
/// Capture: toast
capture_mic_muted: "Discord microphone muted",
/// Capture: toast
capture_sound_on: "Discord sound on",
/// Capture: toast
capture_deafened: "Discord deafened",
/// Capture: toast while scanning for other Rare devices on the network
capture_looking: "Looking for Rare devices",
/// Capture: toast error; Sunshine is the streaming host program
capture_needs_sunshine: "Streaming this device needs Sunshine, which comes with the next Rare OS update.",
/// Capture: toast
capture_stream_starting: "Starting streaming",
/// Capture: toast
capture_stream_off: "Streaming is off",
/// Capture: toast after pairing a Moonlight client; Rare is the Rare host
capture_paired: "Paired. In Moonlight, pick Rare to play.",
/// Capture: toast after removing a paired device
capture_unpaired: "Removed. It has to pair again to stream.",
/// Downloads: time left under a minute (shown in a row)
dl_under_minute: "Less than a minute left",
/// Downloads: time left in minutes
dl_min_left: "{n} min left",
/// Downloads: time left in whole hours
dl_h_left: "{n} h left",
/// Downloads: time left in hours and minutes
dl_h_min_left: "{h} h {m} min left",
/// Downloads: status word on a row
dl_paused: "Paused",
/// Downloads: status word on a row for a game update
dl_updating: "Updating",
/// Downloads: status word, waiting in the queue
dl_queued: "Queued",
/// Downloads: status word while Steam starts an install; Steam is the store app
dl_starting_install: "Starting the install",
/// Downloads: line under the status; Steam is the store app
dl_steam_getting_ready: "Steam is getting it ready",
/// Downloads: button on the game page when Steam holds an update back (short: button)
dl_update_now: "Update now",
/// Downloads: button to pause a download (short: button, verb)
dl_pause: "Pause",
/// Downloads: status/button while an install starts (short)
dl_starting: "Starting",
/// Downloads: fallback name of a game; id is a number
dl_steam_app: "Steam app {id}",
/// Downloads: notification title; title is a game
dl_finished: "{title} finished downloading",
/// Downloads: Rare menu line; pct is a percent
dl_summary_one: "{title}, {pct}%",
/// Downloads: Rare menu line; n other downloads wait
dl_summary_more: "{title}, {pct}%. {n} more in the queue",
/// Downloads: Rare menu line when everything is paused
dl_summary_paused: "{n} paused",
/// Downloads: toast
dl_toast_starting: "Starting the install of {title}",
/// Downloads: toast; the Rare button is the Rare button on the device
dl_finish_in_steam: "Finish the install in Steam. The Rare button brings you back.",
/// Downloads: toast error
dl_close_game_install: "Close the game first, then install.",
/// Downloads: toast
dl_removed: "Removed from the queue",
/// Downloads: toast
dl_updating_now: "Updating now",
/// Downloads: toast; the Rare button is the Rare button on the device
dl_steam_has_screen: "Steam's Downloads has the screen. The Rare button brings you back.",
/// Downloads: toast error
dl_close_game_retry: "Close the game first, then try again.",
/// Downloads: toast error
dl_steam_declined: "Steam didn't take that. Try again.",
/// Downloads: fallback game name in the agreement sheet
dl_this_game: "This game",
/// Downloads: agreement sheet row (short: button)
dl_accept_install: "Accept and install",
/// Downloads: agreement sheet row when the agreement has no name
dl_read_agreement: "Read the agreement",
/// Downloads: agreement sheet row; name is the agreement's name
dl_read_named: "Read {name}",
/// Downloads: sheet row to abort an install
dl_cancel_install: "Cancel the install",
/// Downloads: agreement sheet subtitle
dl_eula_sub: "{title} asks you to accept its licence agreement.",
/// Downloads: agreement sheet subtitle; names is a comma separated list of agreements
dl_eula_sub_named: "{title} asks you to accept: {names}.",
/// Downloads: agreement sheet title
dl_eula_title: "Licence agreement",
/// Downloads: toast error; Steam is the store app
dl_no_library_folder: "Steam has no library folder yet. Open Steam once, then try again.",
/// Downloads: drive row; size is free space like 120 GB
dl_free: "{size} free",
/// Downloads: drive row when the game does not fit
dl_not_enough_space: "Not enough space",
/// Downloads: toast when picking a drive that is too small; size is the game's size
dl_needs_space: "{title} needs {size}. Free up space or pick another drive.",
/// Downloads: drive sheet subtitle
dl_install_size: "Install size {size}. Pick a drive.",
/// Downloads: drive sheet subtitle when the size is unknown
dl_pick_drive: "Pick a drive.",
/// Downloads: drive sheet title
dl_install_title: "Install {title}",
/// Downloads: status word for the next download in the queue
dl_next: "Next",
/// Downloads: status word for a game update waiting; also a section title
dl_waiting: "Waiting",
/// Downloads: row line for a waiting update; size is the update's size
dl_update_size: "Update, {size}",
/// Downloads: row action hint on a game page (short: hint)
dl_game_page: "Game page",
/// Downloads: empty state button (short: button)
dl_not_installed_games: "Not installed games",
/// Downloads: empty state text
dl_empty_body: "Games you install and updates Steam fetches show here, with their progress.",
/// Downloads: note under the list; Steam is the store app
dl_speed_note: "Speed and time left come from this device's network while Steam starts up.",
/// Downloads: gamepad hint, X button (short: hint)
dl_hint_control: "Pause, resume or update now",
/// Downloads: sheet row for a waiting download
dl_download_first: "Download this first",
/// Downloads: sheet row to cancel a queued update
dl_remove_update: "Remove the update from the queue",
/// Downloads: sheet subtitle for a waiting update
dl_waiting_sub: "Waiting. Steam updates it later, or now if you ask.",
/// Downloads: progress line; done and total are sizes
dl_of: "{done} of {total}",
/// Fan curve: preset name (short: chip)
fan_quiet: "Quiet",
/// Fan curve: preset name (short: chip)
fan_balanced: "Balanced",
/// Fan curve: preset name, the fan at full speed (short: chip)
fan_max: "Max",
/// Fan curve: preset name, the player's own curve (short: chip)
fan_custom: "Custom",
/// Fan curve: what the Quiet preset does; C is degrees Celsius
fan_quiet_note: "Stays low to about 65 C.",
/// Fan curve: what the Balanced preset does
fan_balanced_note: "The device's own curve.",
/// Fan curve: what the Max preset does; C is degrees Celsius
fan_max_note: "Spins up early and runs flat out from 80 C.",
/// Fan curve: what the Custom preset is; X is a gamepad button
fan_custom_note: "Your curve. Up and down change a point, X saves it.",
/// Fan curve: part of the live line like "54 C, 2,898 rpm, fan at 41%"; n is a percent
fan_duty: "fan at {n}%",
/// Fan curve: live line while the fan has not been read yet
fan_reading: "Reading the fan",
/// Fan curve: page title
fan_title: "Fan curve",
/// Fan curve: empty state title
fan_no_control: "No fan control here",
/// Fan curve: empty state text
fan_no_control_body: "The fan follows the device's own curve. Rare OS controls the fan on the SuiPlay0X1.",
/// Fan curve: marker under the bar for the current temperature (short)
fan_now: "Now",
/// Fan curve: button, row hint and gamepad hint to save the edited curve (short)
fan_save_curve: "Save curve",
/// Fan curve: note under the chart; X and B are gamepad buttons
fan_unsaved: "Not saved yet. X saves this curve; B leaves it as it was.",
/// Fan curve: note under the chart while editing
fan_editing: "Up and down change the point you are on. The fan never slows down as it gets hotter.",
/// Fan curve: note under the chart when a preset is shown
fan_start_custom: "Up or down on a point starts a custom curve from this one.",
/// Fan curve: warning when the fan is in emergency mode
fan_flat_out: "The fan is running flat out to cool the device down.",
/// Fan curve: toast error
fan_change_failed: "Couldn't change the fan.",
/// Fan curve: toast; t is a temperature in degrees Celsius
fan_up_down: "Up and down change the fan at {t} C.",
/// Fan curve: toast
fan_saved: "Fan curve saved",
/// Fan curve: toast error
fan_save_failed: "Couldn't save the fan curve.",
/// Fan curve: gamepad hint for LB and RB (short: hint)
fan_preset: "Preset",
/// Desktop mode on-screen notice (OSD) when the gamepad controls games
desktop_osd_controller: "Controller",
/// Desktop mode on-screen notice (OSD) when mouse and keyboard control the desktop
desktop_osd_mouse: "Mouse and keyboard",
/// Cloud saves: error; name is a game or emulator save set
cs_too_large: "{name} saves are too large to keep automatically",
/// Cloud saves: notice; name is the game, error the reason
cs_get_failed: "Couldn't get the cloud save for {name}: {error}",
/// Cloud saves: toast when turning on without a usable wallet account
cs_need_signer: "Add a Rare Wallet account you can sign with first",
/// Cloud saves: reason text shown by the wallet when it asks to sign
cs_turn_on_prompt: "Turn on cloud saves",
/// Cloud saves: error
cs_wallet_stopped: "The wallet stopped. Try again.",
/// Cloud saves: toast after turning on; funds are WAL and SUI tokens
cs_on_needs_funds: "Cloud saves are on. Add funds so Rare can upload.",
/// Cloud saves: toast after turning on
cs_on: "Cloud saves are on",
/// Cloud saves: wallet review sheet title
cs_add_funds_title: "Add funds for cloud saves",
/// Cloud saves: toast
cs_funds_added: "Funds added for cloud saves",
/// Cloud saves: notice after picking this device's save in a conflict
cs_kept_device: "Kept this device's save. Cloud saves need funds to upload it.",
/// Cloud saves: notice; error is the reason
cs_finish_failed: "Couldn't finish: {error}",
/// Cloud saves: restore error
cs_not_on_device: "These saves are not on this device",
/// Cloud saves: restore error
cs_copy_gone: "That cloud copy is gone",
/// Cloud saves: restore error
cs_turn_on_to_get: "Turn on cloud saves to get this copy",
/// Cloud saves: notice; Versions is the name of the versions list
cs_restored: "Restored. The saves you had are kept in Versions.",
/// Cloud saves: notice; error is the reason
cs_restore_failed: "Couldn't restore: {error}",
/// Cloud saves: status line while checking a game's saves
cs_checking: "Checking cloud saves",
/// Cloud saves: status line while restoring
cs_restoring: "Restoring",
/// Cloud saves: status line while downloading the cloud copy
cs_getting: "Getting your cloud save",
/// Cloud saves: status line while uploading
cs_saving: "Saving to the cloud",
/// Cloud saves: error; WAL and SUI are tokens, Saves is the Saves screen
cs_need_funds: "Cloud saves need funds. Add WAL and SUI in Saves.",
/// Cloud saves: error when offline
cs_no_network: "no network",
/// Cloud saves: error
cs_upload_later: "Couldn't upload yet. Rare tries again shortly.",
/// Phone control: toast on a build with no phone control
phone_device_only: "Phone control runs on the device (fixture).",
/// Phone control: result line; title is the open text field's title
phone_typed_into: "Typed into {title}",
/// Phone control: result line when the phone sent text and no field is open
phone_no_text_field: "Nothing on the device is waiting for text. Open a text field first.",
/// Phone control: result line; https:// is literal
phone_link_https: "Links need to start with https://.",
/// Phone control: toast
phone_opening_link: "Opening a link from your phone",
/// Phone control: result line; name is a web address host
phone_opened: "Opened {name}",
/// Phone control: result line
phone_unknown_action: "This version of Rare does not know that action yet. Update Rare.",
/// Phone control: result line; Steam is the store
phone_bad_steam_id: "That Steam app number is not valid.",
/// Phone control: result line
phone_in_library: "{name} is in the library",
/// Phone control: toast
phone_added_toast: "{name} added from your phone",
/// Phone control: result line
phone_added: "Added {name} to the library",
/// Phone control: result line
phone_opened_no_install: "Opened {name}. It has no install on this device yet.",
/// Phone control: result line; Rare's catalog is the Rare store
phone_not_in_catalog: "{name} is not in Rare's catalog on this device yet.",
/// Phone control: result line when the store is not Steam, Epic or GOG
phone_unknown_store: "Unknown store.",
/// Phone control: result line; Steam is the store
phone_steam_opened: "Steam opened {name}. Finish the install on the device.",
/// Phone control: toast
phone_eula_toast: "{name} has a licence agreement. Accept it on its page to install.",
/// Phone control: result line
phone_eula: "{name} has a licence agreement. Accept it on the device.",
/// Phone control: result line
phone_bad_game_id: "That game id is not valid.",
/// Phone control: result line
phone_nothing_pause: "Nothing to pause",
/// Phone control: result line
phone_nothing_resume: "Nothing to resume",
/// Phone control: result line, n downloads paused
phone_paused_n: "Paused {n} download|Paused {n} downloads",
/// Phone control: result line, n downloads resumed
phone_resumed_n: "Resumed {n} download|Resumed {n} downloads",
/// Phone control: result line; Share is the gallery of clips and screenshots
phone_saved_share: "Saved {name} to Share",
/// Phone control: error
phone_no_downloads: "No Downloads folder",
/// Phone control: result line; Downloads is a folder
phone_saved_downloads: "Saved {name} to Downloads",
/// Phone control: settings error; rare.market is a website name
phone_signin_failed: "This device could not sign in to rare.market: {error}",
/// Phone control: settings error
phone_signin_first: "Sign in with Rare Wallet first.",
/// Phone control: error
phone_not_signed_in: "Not signed in",
/// Phone control: result line
phone_save_failed: "Could not save {name}: {error}",
/// Phone control: result line, n characters could not be typed
phone_typed_skipped: "Typed it ({n} character this keyboard cannot type was left out)|Typed it ({n} characters this keyboard cannot type were left out)",
/// Phone control: result line
phone_typed_game: "Typed it into the game",
/// Phone control: entry in the recent list
phone_paired: "Paired with your phone",
/// Phone control: error
phone_no_keyboard: "No virtual keyboard on this device ({error})",
/// Phone control: result line
phone_already_installed: "{name} is already installed",
/// Phone control: result line
phone_in_downloads: "{name} is in the downloads",
/// Phone control: toast and result
phone_installing: "Installing {name}, sent from your phone",
/// Emulation: relative time, n minutes ago (short)
emu_min_ago: "{n} min ago",
/// Emulation: relative time, n hours ago (short)
emu_h_ago: "{n} h ago",
/// Emulation: relative time
emu_yesterday: "Yesterday",
/// Emulation: relative time, 2 days or more
emu_days_ago: "{n} day ago|{n} days ago",
/// Emulation: toast after setup finished
emu_set_up: "Emulation is set up",
/// Emulation: toast; error is the reason
emu_setup_problems: "Setup finished with problems: {error}",
/// Retro trophies: error
emu_make_account: "Make a Rare Wallet account first.",
/// Retro trophies: error; Friends is the Friends feature
emu_friends_failed: "Friends couldn't turn on here.",
/// Retro trophies: error
emu_turn_on_first: "Turn on retro trophies first.",
/// Retro trophies: error; rare.market is a website
emu_no_token: "rare.market didn't answer with a token.",
/// Retro trophies: toast
emu_retro_on: "Retro trophies are on",
/// Retro trophies: points count with unit, used in the unlock notice
emu_points: "{n} point|{n} points",
/// Retro trophies: unlock notice text; description is the achievement text, points like "10 points"
emu_unlock_body: "{description} \u{b7} {points}",
/// Emulation tab header: title while setup runs
emu_setting_up_title: "Setting up emulation",
/// Emulation tab header: title when ready
emu_ready_title: "Emulation is ready",
/// Emulation tab header: n emulators installed; folder is a path
emu_ready_line: "{n} emulators. ROMs go in {folder}",
/// Emulation tab header: title when setup is needed
emu_set_up_title: "Set up emulation",
/// Emulation tab header: line when nothing is installed; Flathub is an app source
emu_install_all_line: "Installs RetroArch and the standalone emulators from Flathub, and makes the ROM folders on this device and your SD card.",
/// Emulation tab header: line when some are installed; missing and total are counts
emu_install_some_line: "{missing} of {total} emulators to install. Rare installs them from Flathub and makes the ROM folders.",
/// Emulation: title (tab header and setup page)
emu_title: "Emulation",
/// Emulation tab header: line while status loads
emu_checking_installed: "Checking what is installed",
/// Emulation tab header: button while setup runs (short: button)
emu_setting_up: "Setting up",
/// Emulation tab header: button once everything is installed (short: button)
emu_emulators: "Emulators",
/// Emulation tab header: button to start setup (short: button)
emu_set_up_btn: "Set up",
/// Emulation tab header: BIOS button with a count of missing systems (short: button)
emu_bios_missing: "BIOS ({n} missing)",
/// Emulation tab header: BIOS button (short: button; BIOS is a console firmware file)
emu_bios: "BIOS",
/// Emulation tab header: button when retro trophies are on (short: button)
emu_retro_trophies_on: "Retro trophies: on",
/// Emulation: button and page title (short)
emu_retro_trophies: "Retro trophies",
/// Emulation: setup progress line
emu_making_folders: "Making the ROM and BIOS folders",
/// Emulation: setup progress line; name is an emulator, pct is like " 45%" or empty
emu_installing_name: "Installing {name}{pct}",
/// Emulation: setup progress line; pct is like " 45%" or empty
emu_getting_cores: "Getting RetroArch's cores{pct}",
/// Emulation: setup progress line
emu_working: "Working",
/// Emulation: game page line; system is a console name, emulator a program name
emu_plays_in: "{system} \u{b7} plays in {emulator}",
/// Emulation: game page line; Library, Emulation is the Library tab named Emulation
emu_needs: "{system} \u{b7} needs {emulator}, set it up in Library, Emulation",
/// Emulation: game page text; Rare trophies are the Rare achievements
emu_turn_on_to_earn: "Turn on retro trophies to earn Rare trophies in this game.",
/// Emulation: game page line while loading
emu_looking_up: "Looking up trophies",
/// Emulation: game page line
emu_no_trophies: "No trophies for this game yet.",
/// Emulation: game page line
emu_trophies_after_play: "Trophies show here after you play it once.",
/// Emulation: game page progress; all numbers
emu_progress: "{earned} of {total} trophies \u{b7} {pts} of {total_pts} points",
/// Emulation setup: note under the title
emu_all_installed: "Everything is installed",
/// Emulation setup: note under the title
emu_n_of_installed: "{n} of {total} installed",
/// Emulation setup: empty state title; Flatpak is an app format
emu_no_flatpak: "Flatpak isn't on this system",
/// Emulation setup: empty state text; AppImages are an app format, Applications is the Applications page
emu_no_flatpak_body: "Rare installs emulators with Flatpak. Emulators you put in Applications as AppImages still work.",
/// Emulation setup: button when only cores are missing (short: button)
emu_install_cores: "Install the cores",
/// Emulation setup: button when all is installed (short: button)
emu_check_again: "Check again",
/// Emulation setup: button, n emulators ticked
emu_set_up_n: "Set up {n} emulator|Set up {n} emulators",
/// Emulation setup: hint next to the buttons
emu_tick_systems: "Tick the systems you play, or set up everything.",
/// Emulation setup: state word while installing; pct is like " 45%" or empty
emu_installing_pct: "Installing{pct}",
/// Emulation setup: state word for an emulator found as an AppImage
emu_installed_appimage: "Installed (AppImage)",
/// Emulation setup: state word when an install failed
emu_didnt_install: "Didn't install",
/// Emulation setup: row hint to untick (short: hint)
emu_untick: "Untick",
/// Emulation setup: row hint to tick (short: hint)
emu_tick: "Tick",
/// Emulation setup: section subtitle
emu_emulators_sub: "Ticked for the systems you have games for",
/// Emulation setup: section title (controller mappings)
emu_controls: "Controls",
/// Emulation setup: section subtitle; SuiPlay is a device name
emu_controls_sub: "Every button set for the SuiPlay, hotkeys in the Rare menu",
/// Emulation setup: state word (short)
emu_ready: "Ready",
/// Emulation setup: row text when no emulator is installed
emu_set_when_installed: "Set when an emulator is installed",
/// Emulation setup: row title
emu_rares_controls: "Rare's controls",
/// Emulation setup: button while replacing (short: button)
emu_replacing: "Replacing",
/// Emulation setup: button asking for confirmation (short: button)
emu_replace_mine: "Replace my settings",
/// Emulation setup: button (short: button)
emu_use_rares: "Use Rare's controls",
/// Emulation setup: row text; names is a list of emulators; .before-rare is a file suffix
emu_replace_confirm: "{names}: press again to replace them. Yours stay next to them as .before-rare files.",
/// Emulation setup: row text; names is a list of emulators
emu_own_kept: "{names}: your own settings, kept as they are",
/// Emulation setup: row title
emu_own_settings: "Your own controller settings",
/// Emulation setup: row text
emu_all_cores: "All {n} cores installed",
/// Emulation setup: row text; libretro is a project name
emu_cores_missing: "{missing} of {total} to download from libretro",
/// Emulation setup: section title
emu_cores_head: "RetroArch cores",
/// Emulation setup: section subtitle; console names stay
emu_cores_sub: "NES, SNES, Game Boy, Genesis, arcade and more",
/// Emulation setup: state word (short)
emu_missing: "Missing",
/// Emulation setup: row title
emu_cores: "Cores",
/// Emulation setup: section title
emu_rom_folders: "ROM folders",
/// Emulation setup: section subtitle
emu_rom_folders_sub: "Copy games you own into the folder for their system",
/// Emulation setup: folder row; roms and bios are paths
emu_folder_line: "{roms}  \u{b7}  BIOS: {bios}",
/// Emulation setup: state word for a folder that setup will create
emu_made_by_setup: "Made by setup",
/// Emulation setup: note at the bottom; Emulation/saves is a folder path
emu_saves_note: "Saves and states are kept under Emulation/saves, which Rare's cloud saves back up. Rare never downloads games or BIOS files.",
emu_add_title: "Add games",
emu_add_intro: "Games go into {folder}, each in its system's folder, and show up in Library. Only games you own or that are free to share.",
emu_add_phone: "From a phone or a computer",
emu_add_phone_title: "Send over Wi-Fi",
emu_add_phone_line: "Opens a page on this Wi-Fi to drop game files on",
emu_add_open: "Open",
emu_add_scan: "Scan the code with your phone, or open the address in a browser on the same Wi-Fi.",
emu_add_same_wifi: "It is open while this page is. Drop the files there; each goes into its system's folder.",
emu_add_close: "Close",
emu_add_got: "In Library · {system}",
emu_add_no_network: "No Wi-Fi network to share the page on.",
emu_add_drive: "From a USB drive or an SD card",
emu_add_no_drive: "No drive in",
emu_add_no_drive_line: "Plug in a USB drive or put in an SD card with your games",
emu_add_drive_line: "{size}. Look for games on it",
emu_add_look: "Look for games",
emu_add_looking: "Looking for games…",
emu_add_none_found: "No games this device plays",
emu_add_unknown: "system unclear: {n}, put them in a system's folder on the drive",
emu_add_copy_n: "Copy games: {n}, {size}",
emu_add_on_drive: "On {drive}",
emu_add_copied: "Copied: {n}",
emu_add_in_library: "They are in Library",
/// Emulation setup: toast
emu_installed_toast: "{name} is installed. Set up checks it again.",
/// BIOS page: note under the title
emu_systems_ready: "{n} of {total} systems ready",
/// BIOS page: title
emu_bios_files: "BIOS files",
/// BIOS page: intro; folder is a path; Emulation/bios is a folder path
emu_bios_intro: "Some systems need files from the console itself. Copy the ones you dumped from your own consoles into {folder} (or Emulation/bios on your SD card). Rare checks each file against the known good copies.",
/// BIOS page: loading text
emu_checking: "Checking",
/// BIOS page: state word when a file is the wrong one (short)
emu_wrong_file: "Wrong file",
/// BIOS page: label on a console that has games in the library
emu_have_games: "You have games for it",
/// BIOS page: file state
emu_found: "Found",
/// BIOS page: file state
emu_not_right_file: "Not the right file",
/// BIOS page: file alternatives; names is a list of file names
emu_or: "or {names}",
/// BIOS page: marks a file that is not required
emu_optional: "optional",
/// Retro trophies page: note; user is the account's retro username
emu_show_as: "Emulators show you as {user}",
/// Retro trophies page: intro; RetroAchievements is a service name
emu_retro_intro: "Classic games earn Rare trophies as you play, with no other account. Achievement sets come from RetroAchievements; your unlocks stay with your Rare account.",
/// Retro trophies page: row text
emu_retro_off_line: "Off on this device. Turning them on lets this device act for your Rare account.",
/// Retro trophies page: button while turning on (short: button)
emu_turning_on: "Turning on",
/// Retro trophies page: row title (RetroAchievements hardcore mode)
emu_hardcore: "Hardcore",
/// Retro trophies page: row text
emu_hardcore_line: "No save state loads, rewind or slow motion. Unlocks are marked hardcore.",
/// Retro trophies page: row hint (short: hint)
emu_toggle: "Toggle",
/// Retro trophies page: row title
emu_works_in: "Works in",
/// Retro trophies page: row text, emulator names
emu_works_in_list: "RetroArch, DuckStation, PCSX2, PPSSPP and Dolphin",
/// Retro trophies page: section title
emu_recently_played: "Recently played",
/// Retro trophies page: row text, trophies earned of total
emu_earned_of: "{earned} of {total}",
/// Retro trophies page: row text; console is a console name
emu_console_earned_of: "{console} \u{b7} {earned} of {total}",
/// Retro trophies page: empty text; Library, Emulation is the Library tab named Emulation
emu_play_to_show: "Play a game from Library, Emulation and its trophies show here.",
    /// Fan: a temperature; t is whole degrees Celsius
    fan_temp: "{t} C",
    /// Fan: its speed; n is a grouped number like 2,898
    fan_rpm: "{n} rpm",
}
