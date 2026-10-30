#!/usr/bin/bash
# Guest side of `test.sh handheld`: makes a stock Bazzite VM look like a handheld someone has used
# for a while, then checks that switching it to Rare OS keeps everything that matters for remote
# access. Runs as root inside the VM.
#
#   handheld.sh prep         user `bazzite` (uid 1000) with an SSH key, sshd enabled by a symlink in
#                            /etc (what `systemctl enable` does), a Wi-Fi profile, and the SuiPlay
#                            device setup allowed to run in a VM (QEMU reports a SuiPlay0X1)
#   handheld.sh prep-sddm    the game-mode autologin as an older Bazzite (F42) leaves it
#   handheld.sh check-base   the old OS: user, sshd, files (before the switch and after rollback)
#   handheld.sh check-rare   Rare OS after the switch: checks a-d and f (see test.sh)
set -uo pipefail
U=bazzite
HOME_DIR=/var/home/$U
NM=/etc/NetworkManager/system-connections/TP-Link-test.nmconnection
SUMS=/var/tmp/handheld.sums

fails=0
pass() { echo "CHECK $1 PASS: $2"; }
fail() { echo "CHECK $1 FAIL: $2"; fails=$((fails + 1)); }

prep() {
    getent passwd 1000 >/dev/null || useradd --create-home --uid 1000 --groups wheel $U
    echo "$U:rare-test-pw" | chpasswd
    install -d -m 0700 -o $U -g $U $HOME_DIR/.ssh
    install -m 0600 -o $U -g $U /var/tmp/bazzite.pub $HOME_DIR/.ssh/authorized_keys
    restorecon -R $HOME_DIR 2>/dev/null || true

    mkdir -p /etc/systemd/system/multi-user.target.wants
    ln -sf /usr/lib/systemd/system/sshd.service /etc/systemd/system/multi-user.target.wants/sshd.service

    cat > $NM <<'EOF'
[connection]
id=TP-Link-test
uuid=5b7c2a3e-8f4d-4c1b-9a6e-2d3f4a5b6c7d
type=wifi
interface-name=wlan0

[wifi]
mode=infrastructure
ssid=TP-Link-test

[wifi-security]
auth-alg=open
key-mgmt=wpa-psk
psk=rare-test-psk

[ipv4]
method=auto

[ipv6]
addr-gen-mode=default
method=auto

[proxy]
EOF
    chmod 0600 $NM
    restorecon $NM 2>/dev/null || true
    nmcli connection reload 2>/dev/null || true

    # The owner's device has an empty /etc/hostname: its name comes from the OS default.
    : > /etc/hostname

    sha256sum $HOME_DIR/.ssh/authorized_keys $NM > $SUMS
    cat $SUMS
}

prep_sddm() {
    # The owner's SuiPlay (Bazzite 42.20251018) has exactly this, written by its bazzite-autologin.
    mkdir -p /etc/sddm.conf.d
    rm -f /etc/sddm.conf.d/zz-*.conf
    cat > /etc/sddm.conf.d/steamos.conf <<EOF
[General]
DisplayServer=wayland

[Autologin]
Relogin=true
Session=gamescope-session.desktop
User=$U

[X11]
# Janky workaround for wayland sessions not stopping in sddm, kills
# all active sddm-helper sessions on teardown
DisplayStopCommand=/usr/bin/gamescope-wayland-teardown-workaround
EOF
    printf '[General]\nInputMethod=qtvirtualkeyboard\n' > /etc/sddm.conf.d/virtualkbd.conf
    ls -la /etc/sddm.conf.d
}

common() {
    # (a) sshd enabled the normal way and running
    local en act
    en="$(systemctl is-enabled sshd.service 2>&1)"; act="$(systemctl is-active sshd.service 2>&1)"
    if [[ "$en" == enabled && "$act" == active ]]; then pass a "sshd $en, $act"; else fail a "sshd is-enabled=$en is-active=$act"; fi
    if systemctl is-active -q firewalld; then
        local z; z="$(firewall-cmd --get-default-zone)"
        if firewall-cmd --zone="$z" --query-service=ssh >/dev/null; then pass a "firewall zone $z allows ssh"; else fail a "firewall zone $z blocks ssh"; fi
    else
        echo "INFO firewalld not active"
    fi
    # (b) the Wi-Fi profile and the SSH key are the same files
    if sha256sum --quiet -c $SUMS; then pass b "authorized_keys and Wi-Fi profile unchanged"; else fail b "files changed or missing"; fi
    if nmcli -g NAME connection show 2>/dev/null | grep -qx TP-Link-test; then pass b "NetworkManager still has the TP-Link-test profile"; else fail b "NetworkManager lost the profile"; fi
    [[ "$(getent passwd 1000 | cut -d: -f1)" == "$U" ]] && pass b "uid 1000 is still $U" || fail b "uid 1000 is $(getent passwd 1000)"
    # The device keeps its network name (ssh bazzite.local): its own name if one is set, otherwise
    # the OS's default name. (A name from DHCP or reverse DNS, as QEMU's network hands out, would
    # hide the difference; the owner's network gives none.)
    local h; h="$(grep -v '^[[:space:]]*$' /etc/hostname 2>/dev/null | head -1)"
    [[ -n "$h" ]] || h="$(. /etc/os-release; echo "${DEFAULT_HOSTNAME:-}")"
    [[ -f /var/tmp/hostname.before ]] || echo "$h" > /var/tmp/hostname.before
    echo "INFO hostname now: $(hostname), static: $(cat /etc/hostname 2>/dev/null)"
    [[ "$h" == "$(cat /var/tmp/hostname.before)" ]] && pass a "network name still $h" || fail a "network name changed from $(cat /var/tmp/hostname.before) to $h"
}

check_base() {
    . /etc/os-release; echo "INFO booted: $PRETTY_NAME"
    common
    rpm-ostree status 2>&1 | sed 's/^/INFO /'
    echo "RESULT fails=$fails"
}

check_rare() {
    . /etc/os-release; echo "INFO booted: $PRETTY_NAME"
    [[ "$NAME" == "Rare OS" ]] && pass rare "running Rare OS" || fail rare "running $NAME"
    common
    journalctl -b -u rare-keep-ssh --no-pager -o cat 2>/dev/null | sed 's/^/INFO keep-ssh: /' | tail -5

    # (c) game mode logged the existing user in by itself, into the Rare session, with no greeter
    echo "INFO sddm config:"; head -20 /etc/sddm.conf.d/*.conf 2>/dev/null | sed 's/^/INFO   /'
    local sid ok_session=0
    for sid in $(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $1}'); do
        local info; info="$(loginctl show-session "$sid" -p Name -p Class -p Type -p Service -p Desktop -p Seat 2>/dev/null | tr '\n' ' ')"
        echo "INFO session $sid: $info"
        if [[ "$info" == *"Name=$U "* && "$info" == *"Class=user "* && "$info" == *"Service=sddm-autologin"* ]]; then ok_session=1; fi
    done
    if (( ok_session )); then pass c "sddm autologin session for $U"; else fail c "no sddm-autologin session for $U"; fi
    if pgrep -f sddm-greeter >/dev/null; then fail c "an SDDM greeter (password prompt) is running"; else pass c "no SDDM greeter"; fi
    local lp; lp="$(pgrep -x rare-launcher | head -1)"
    if [[ -n "$lp" ]]; then
        local lu; lu="$(ps -o user= -p "$lp")"
        [[ "$lu" == "$U" ]] && pass c "rare-launcher runs as $lu (pid $lp)" || fail c "rare-launcher runs as $lu"
        if tr '\0' '\n' </proc/"$lp"/environ | grep -qE '^XDG_SESSION_DESKTOP=(gamescope|cage)|^DESKTOP_SESSION=.*rare'; then
            pass c "launcher is in the Rare game-mode session ($(tr '\0' '\n' </proc/"$lp"/environ | grep -E '^(DESKTOP_SESSION|XDG_SESSION_DESKTOP)=' | tr '\n' ' '))"
        else
            echo "INFO launcher env: $(tr '\0' '\n' </proc/"$lp"/environ | grep -E '^(DESKTOP_SESSION|XDG_SESSION_DESKTOP|XDG_CURRENT_DESKTOP)=' | tr '\n' ' ')"
        fi
        pass d "Rare Launcher started"
    else
        fail c "rare-launcher is not running"; fail d "rare-launcher is not running"
    fi
    pgrep -a -f 'rare-session|gamescope-session|cage ' | sed 's/^/INFO proc: /' | head -5

    # (d) Steam starts from the session the way the launcher starts it (steam steam://open/bigpicture)
    if [[ -n "$lp" ]]; then
        local envs=(); while IFS= read -r l; do envs+=("$l"); done < <(tr '\0' '\n' </proc/"$lp"/environ | grep -E '^(WAYLAND_DISPLAY|DISPLAY|XDG_RUNTIME_DIR|DBUS_SESSION_BUS_ADDRESS|XDG_SESSION_TYPE|HOME|USER|PATH)=')
        runuser -u $U -- env "${envs[@]}" systemd-run --user --scope --collect --quiet --unit=rare-steam-check -- steam steam://open/bigpicture >/var/tmp/steam-check.log 2>&1 </dev/null &
        local t found=""
        for t in $(seq 1 36); do
            sleep 5
            found="$(pgrep -u $U -a -f 'steam\.sh|ubuntu12_32/steam|steamwebhelper|steam-runtime|pressure-vessel|srt-bwrap' | head -3 | cut -c1-120 | tr '\n' ';')"
            [[ -n "$found" ]] && break
        done
        if [[ -n "$found" ]]; then pass d "Steam started as $U: $found"; else fail d "Steam did not start"; pgrep -u $U -a -f steam | sed 's/^/INFO steam proc: /'; tail -20 /var/tmp/steam-check.log | sed 's/^/INFO steam: /'; fi
    fi

    # (f) the old OS is pinned
    local json; json="$(rpm-ostree status --json)"
    rpm-ostree status 2>&1 | sed 's/^/INFO /'
    local pinned; pinned="$(jq -r '[.deployments[] | select((.["container-image-reference"] // "") | test("bazzite")) | .pinned] | map(tostring) | join(",")' <<<"$json")"
    if [[ "$pinned" == *true* ]]; then pass f "old OS deployment pinned ($pinned)"; else fail f "old OS deployment not pinned ($pinned)"; fi
    journalctl -b -u rare-keep-previous-os --no-pager -o cat 2>/dev/null | sed 's/^/INFO /' | tail -8
    systemctl --failed --no-legend --no-pager 2>/dev/null | sed 's/^/INFO failed unit: /'
    echo "RESULT fails=$fails"
}

case "${1:-}" in
    prep) prep ;;
    prep-sddm) prep_sddm ;;
    check-base) check_base ;;
    check-rare) check_rare ;;
    *) echo "usage: $0 prep|prep-sddm|check-base|check-rare" >&2; exit 2 ;;
esac
(( fails == 0 ))
