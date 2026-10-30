#!/usr/bin/bash
# Back to the stock session: no override, cpu mode, the image's launcher; launcher on screen.
rm -f ~/.config/gamescope-session-plus/sessions.d/rare "$XDG_RUNTIME_DIR/rare-launcher-bin"
echo cpu > "$XDG_RUNTIME_DIR/rare-webkit"
systemctl --user restart gamescope-session-plus@rare.service
sleep 15
DISPLAY=:0 xprop -root GAMESCOPE_FOCUSED_APP
pgrep -a rare-launcher
