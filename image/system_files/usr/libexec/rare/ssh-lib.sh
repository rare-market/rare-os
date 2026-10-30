# shellcheck shell=bash
# Shared SSH helpers for os-admin and keep-ssh (sourced, not run). Shipped as
# /usr/libexec/rare/ssh-lib.sh.
#
# Every path can be redirected for tests; with none of these set it acts on the real system:
#   RARE_ROOT          prefix for /etc, /var/lib and home directories
#   RARE_SYSTEMCTL     systemctl command (default systemctl)
#   RARE_SYSTEMD_RUN   systemd-run command (default systemd-run)
#   RARE_NO_FIREWALL=1 never touch the firewall
#   RARE_SSHD_TEST_CMD command that checks the sshd config (default /usr/sbin/sshd -t)
#   RARE_SSH_USER      the login user
#   RARE_SELF          path the auto-off timer runs (default: os-admin next to this file)
#   RARE_SHADOW        a shadow-format file to read password hashes from (default: getent shadow)
#
# SSH is OFF by default. State lives in $R/var/lib/rare/ssh/: state (on|off), keep-password
# (yes), auto-off-at (epoch), netapi-port.

R="${RARE_ROOT:-}"
SYSTEMCTL="${RARE_SYSTEMCTL:-systemctl}"
SYSTEMD_RUN="${RARE_SYSTEMD_RUN:-systemd-run}"
SSHD_TEST="${RARE_SSHD_TEST_CMD:-/usr/sbin/sshd -t}"
STATE_DIR="$R/var/lib/rare/ssh"
SSHD_D="$R/etc/ssh/sshd_config.d"
# Named 00- so it is read first: sshd uses the first value it finds, and the base ships
# 40-redhat-crypto-policies.conf and 50-redhat.conf (plus the main file) that may set the same keys.
AUTH_CONF="$SSHD_D/00-rare-auth.conf"
HARDEN_CONF="$SSHD_D/01-rare-hardening.conf"
RARE_SELF_PATH="${RARE_SELF:-$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/os-admin}"

ssh_state_init() { mkdir -p "$STATE_DIR" && chmod 0755 "$STATE_DIR"; }

ssh_state_get() { # name -> content or empty
    local v=""
    [[ -r "$STATE_DIR/$1" ]] && v="$(tr -d '[:space:]' <"$STATE_DIR/$1")"
    printf '%s' "$v"
}
ssh_state_set() { ssh_state_init && printf '%s\n' "$2" >"$STATE_DIR/$1.tmp" && mv -f "$STATE_DIR/$1.tmp" "$STATE_DIR/$1"; }
ssh_state_clear() { rm -f "$STATE_DIR/$1"; }
ssh_is_on() { [[ "$(ssh_state_get state)" == on ]]; }

ssh_user() {
    if [[ -n "${RARE_SSH_USER:-}" ]]; then printf '%s\n' "$RARE_SSH_USER"; return; fi
    if [[ -n "${PKEXEC_UID:-}" ]] && id -un "$PKEXEC_UID" 2>/dev/null; then return; fi
    getent passwd | awk -F: '$3>=1000 && $3<60000 {print $1; exit}'
}
ssh_user_home() { # user
    if [[ -n "$R" ]]; then printf '%s\n' "$R/home/$1"; else getent passwd "$1" | cut -d: -f6; fi
}
ssh_auth_keys() { printf '%s/.ssh/authorized_keys\n' "$(ssh_user_home "$1")"; }

# Prints "<fingerprint> <type> <comment or ->" for every key in a file.
ssh_list_keys() { # file
    [[ -s "$1" ]] || return 0
    ssh-keygen -l -f "$1" 2>/dev/null | awk '{
        fp=$2; t=tolower($NF); gsub(/[()]/,"",t); c="";
        for (i=3;i<NF;i++) c=c (c==""?"":" ") $i;
        if (c=="") c="-"; print fp, t, c }'
}
ssh_key_count() { ssh_list_keys "$1" | wc -l | tr -d ' '; }

# Passwords a device can carry from the system it was switched from (GameOS ships playtron with
# the password playtron). Password sign-in stays off while one of these is set.
SSH_DEFAULT_PASSWORDS=(playtron)

ssh_user_hash() { # user -> the password hash, or empty
    if [[ -n "${RARE_SHADOW:-}" ]]; then
        awk -F: -v u="$1" '$1 == u {print $2; exit}' "$RARE_SHADOW" 2>/dev/null
    else
        getent shadow "$1" 2>/dev/null | cut -d: -f2
    fi
}

# Exit 0 when the user's password is one of SSH_DEFAULT_PASSWORDS. The hash is checked with
# libcrypt (yescrypt, sha512crypt and the rest); the candidates go through the environment.
ssh_default_password() { # user
    local h
    h="$(ssh_user_hash "$1")"
    [[ "$h" == \$* ]] || return 1
    RARE_HASH="$h" RARE_CANDIDATES="$(printf '%s\n' "${SSH_DEFAULT_PASSWORDS[@]}")" python3 -I -c '
import ctypes, ctypes.util, os, sys
lib = None
for name in ("libcrypt.so.2", "libcrypt.so.1", ctypes.util.find_library("crypt")):
    try:
        lib = ctypes.CDLL(name)
        break
    except Exception:
        pass
if lib is None:
    sys.exit(2)
lib.crypt.restype = ctypes.c_char_p
lib.crypt.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
h = os.environ["RARE_HASH"].encode()
for pw in os.environ["RARE_CANDIDATES"].split("\n"):
    if pw and lib.crypt(pw.encode(), h) == h:
        sys.exit(0)
sys.exit(1)
' 2>/dev/null
}
ssh_is_default_word() { # password -> 0 when it is a default one
    local d
    for d in "${SSH_DEFAULT_PASSWORDS[@]}"; do [[ "$1" == "$d" ]] && return 0; done
    return 1
}

ssh_password_auth_now() {
    local v=""
    [[ -r "$AUTH_CONF" ]] && v="$(awk 'tolower($1)=="passwordauthentication" {print tolower($2); exit}' "$AUTH_CONF")"
    [[ "$v" == no ]] && echo no || echo yes
}

ssh_install_hardening() {
    mkdir -p "$SSHD_D"
    cat >"$HARDEN_CONF.tmp" <<'CONF'
# Rare OS: SSH hardening, written by os-admin / keep-ssh. Named 01- so it is read before the
# base's 40-redhat-crypto-policies.conf and 50-redhat.conf (sshd keeps the first value it reads).
MaxAuthTries 3
LoginGraceTime 20
MaxStartups 4:50:10
PermitEmptyPasswords no
ClientAliveInterval 60
ClientAliveCountMax 3
CONF
    mv -f "$HARDEN_CONF.tmp" "$HARDEN_CONF"
}

# Password login is on unless the user has a key and did not ask to keep passwords, and always
# off while the password is a default one (playtron after a switch from GameOS) until the owner
# sets a new one in Settings > Remote.
# Writes the drop-in, checks it with sshd -t (removes it and fails if the check fails) and reloads sshd.
ssh_apply_auth() {
    local user keys pw
    user="$(ssh_user)"
    keys=0
    [[ -n "$user" ]] && keys="$(ssh_key_count "$(ssh_auth_keys "$user")")"
    pw=yes
    if (( keys > 0 )) && [[ "$(ssh_state_get keep-password)" != yes ]]; then pw=no; fi
    if [[ -n "$user" ]] && ssh_default_password "$user"; then pw=no; fi
    mkdir -p "$SSHD_D"
    cat >"$AUTH_CONF.tmp" <<CONF
# Rare OS: SSH login policy, written by os-admin (Settings > System > SSH). Do not edit.
# Named 00- so it is read first: sshd keeps the first value it reads for a keyword, and the base's
# 40-redhat-crypto-policies.conf and 50-redhat.conf must not override these.
PasswordAuthentication $pw
KbdInteractiveAuthentication $pw
PermitRootLogin no
CONF
    mv -f "$AUTH_CONF.tmp" "$AUTH_CONF"
    # shellcheck disable=SC2086
    if ! err="$($SSHD_TEST 2>&1)"; then
        rm -f "$AUTH_CONF"
        echo "rare: sshd config does not load, login policy not applied: $err" >&2
        return 1
    fi
    if "$SYSTEMCTL" is-active -q sshd.service 2>/dev/null; then
        "$SYSTEMCTL" reload-or-restart --no-block sshd.service || true
    fi
    return 0
}

ssh_firewall_zones() {
    local zones
    zones="$(firewall-cmd --get-default-zone 2>/dev/null)"
    zones+=" $(firewall-cmd --get-active-zones 2>/dev/null | awk '/^[^[:space:]]/ {print $1}' || true)"
    zones+=" $(sed -n 's/^zone=//p' "$R"/etc/NetworkManager/system-connections/*.nmconnection 2>/dev/null || true)"
    tr ' ' '\n' <<<"$zones" | grep -E '^[A-Za-z0-9_.-]+$' | sort -u
}
ssh_firewall_active() {
    [[ "${RARE_NO_FIREWALL:-}" != 1 ]] && "$SYSTEMCTL" is-active -q firewalld.service 2>/dev/null && command -v firewall-cmd >/dev/null
}

# Make sshd enabled, running and reachable. Only adds.
ssh_ensure_on() {
    local unit=sshd.service z
    if [[ "$(readlink -f "$R/etc/systemd/system/$unit" 2>/dev/null)" == /dev/null ]]; then
        rm -f "$R/etc/systemd/system/$unit"
        "$SYSTEMCTL" daemon-reload
        echo "rare: sshd was masked; unmasked"
    fi
    if ! "$SYSTEMCTL" is-enabled -q $unit 2>/dev/null; then
        "$SYSTEMCTL" enable $unit && echo "rare: sshd was disabled; enabled"
    fi
    ssh_install_hardening
    # shellcheck disable=SC2086
    if err="$($SSHD_TEST 2>&1)"; then
        "$SYSTEMCTL" start --no-block $unit
    else
        echo "rare: sshd config does not load: $err"
        if /usr/sbin/sshd -t -f /usr/etc/ssh/sshd_config >/dev/null 2>&1 && ! "$SYSTEMCTL" is-active -q rare-sshd-fallback.service; then
            "$SYSTEMD_RUN" --unit=rare-sshd-fallback --description="Rare OS: fallback sshd (image config)" \
                -p Restart=always /usr/sbin/sshd -D -e -f /usr/etc/ssh/sshd_config \
                && echo "rare: started a fallback sshd with the image's config"
        fi
    fi
    if ssh_firewall_active; then
        for z in $(ssh_firewall_zones); do
            firewall-cmd --permanent --zone="$z" --query-service=ssh >/dev/null 2>&1 \
                || { firewall-cmd --permanent --zone="$z" --add-service=ssh >/dev/null && echo "rare: firewall zone $z now allows ssh"; }
            firewall-cmd --zone="$z" --query-service=ssh >/dev/null 2>&1 \
                || firewall-cmd --zone="$z" --add-service=ssh >/dev/null 2>&1 || true
        done
    fi
    return 0
}

ssh_stop_all() {
    "$SYSTEMCTL" stop rare-sshd-fallback.service 2>/dev/null || true
    "$SYSTEMCTL" disable --now sshd.service 2>/dev/null || true
}

ssh_autooff_cancel() {
    "$SYSTEMCTL" stop rare-ssh-autooff.timer rare-ssh-autooff.service 2>/dev/null || true
    "$SYSTEMCTL" reset-failed rare-ssh-autooff.service 2>/dev/null || true
}
ssh_autooff_schedule() { # seconds
    ssh_autooff_cancel
    "$SYSTEMD_RUN" --unit=rare-ssh-autooff --on-active="${1}s" --description="Rare OS: turn SSH off" \
        "$RARE_SELF_PATH" ssh off >/dev/null
}

ssh_off() {
    ssh_stop_all
    ssh_autooff_cancel
    ssh_state_set state off
    ssh_state_clear auto-off-at
}

ssh_fail2ban() {
    local bin="${RARE_FAIL2BAN_CLIENT:-/usr/bin/fail2ban-client}"
    [[ -x "$bin" ]] || return 0
    mkdir -p "$R/etc/fail2ban/jail.d"
    cat >"$R/etc/fail2ban/jail.d/rare-sshd.local" <<'CONF'
# Rare OS: written by os-admin ssh on.
[sshd]
enabled = true
maxretry = 5
findtime = 10m
bantime = 15m
backend = systemd
CONF
    "$SYSTEMCTL" enable --now fail2ban.service 2>/dev/null || true
}
