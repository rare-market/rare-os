#!/usr/bin/bash
# Rare OS health check (greenboot, required): after an update, the new version counts as good
# only once the session reached Rare and the launcher stayed up for a minute.
#
# Only on the first starts of a new version (greenboot set boot_counter when the update was
# staged). A normal start passes at once: a failing check there would make greenboot reboot and
# count down for no update at all.
#
# Fails when, within 5 minutes of this check starting:
#   - the Rare session is the one that logs in (sddm autologin) and no launcher process (native
#     or web) has run for 60 s in a row, or
#   - another session logs in (desktop mode) and no graphical session process has run for 60 s.
# On a failure greenboot reboots and tries again; when its count runs out it boots the previous
# version (bootc rollback) and red.d/50-rare-record-failed.sh records the version, so the
# updater never stages it again.
#
# Test hooks: RARE_GRUBENV_LIST (command that prints grubenv), RARE_HC_LIMIT, RARE_HC_STABLE,
# RARE_HC_SDDM_DIR.
set -uo pipefail
LIMIT=${RARE_HC_LIMIT:-300}
STABLE=${RARE_HC_STABLE:-60}
SDDM_DIR=${RARE_HC_SDDM_DIR:-/etc/sddm.conf.d}

grubenv() {
    if [[ -n "${RARE_GRUBENV_LIST:-}" ]]; then $RARE_GRUBENV_LIST; else grub2-editenv - list 2>/dev/null; fi
}
if ! grubenv | grep -q '^boot_counter=[0-9]'; then
    echo "rare: not the first start of a new version, nothing to check"
    exit 0
fi

# The session that logs in by itself: the last Session= in sddm's drop-ins (the last file wins).
session=""
for f in "$SDDM_DIR"/*.conf; do
    [[ -r "$f" ]] || continue
    s="$(sed -n 's/^[[:space:]]*Session[[:space:]]*=[[:space:]]*//p' "$f" | tail -1)"
    [[ -n "$s" ]] && session="$s"
done
if [[ "$session" == *gamescope-session-rare* ]]; then
    want='^(rare-launcher-n|rare-launcher)$'
    what="the Rare launcher"
else
    want='^(rare-launcher-n|rare-launcher|plasmashell|gamescope|gamescope-wl|gnome-shell|kwin_wayland)$'
    what="a graphical session (${session:-none set})"
fi

# Longest running matching process of a user (uid 1000..59999), in seconds.
up_for() {
    ps -eo uid=,etimes=,comm= 2>/dev/null | awk -v re="$want" '$1 >= 1000 && $1 < 60000 && $3 ~ re { if ($2 > m) m = $2 } END { print m + 0 }'
}

start=$SECONDS
while (( SECONDS - start < LIMIT )); do
    t="$(up_for)"
    if (( t >= STABLE )); then
        echo "rare: $what has been up for ${t}s; this version is good"
        exit 0
    fi
    sleep 5
done
echo "rare: $what did not stay up for ${STABLE}s within ${LIMIT}s of the check; this version does not start properly" >&2
exit 1
