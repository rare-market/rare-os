#!/usr/bin/env bash
# Tests for os-admin ssh / netapi and keep-ssh, run as an unprivileged user against a temp
# RARE_ROOT with stub systemctl, chpasswd, passwd, systemd-run and firewall-cmd. Needs ssh-keygen.
#   image/tests/os-admin-ssh.sh
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
lib="$here/../system_files/usr/libexec/rare"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
export RARE_ROOT="$T/root" RARE_SSH_USER=tester RARE_LIB_DIR="$lib" RARE_NO_FIREWALL=1
export RARE_SYSTEMCTL="$T/bin/systemctl" RARE_SYSTEMD_RUN="$T/bin/systemd-run" RARE_SELF="$lib/os-admin"
export RARE_FAIL2BAN_CLIENT="$T/none" RARE_SSHD_TEST_CMD="true" RARE_SHADOW="$T/shadow"
# A shadow line for tester with password $1 (yescrypt, through libcrypt like the real check).
mkhash() {
    PW="$1" python3 -I -c '
import ctypes, os
try:
    c = ctypes.CDLL("libcrypt.so.2")
except OSError:
    c = ctypes.CDLL("libcrypt.so.1")
c.crypt.restype = ctypes.c_char_p; c.crypt.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
c.crypt_gensalt.restype = ctypes.c_char_p; c.crypt_gensalt.argtypes = [ctypes.c_char_p, ctypes.c_ulong, ctypes.c_char_p, ctypes.c_int]
print(c.crypt(os.environ["PW"].encode(), c.crypt_gensalt(b"$y$", 0, None, 0)).decode())'
}
shadow_pw() { echo "tester:$(mkhash "$1"):20000:0:99999:7:::" >"$RARE_SHADOW"; }
shadow_pw "owner-chose-this"
mkdir -p "$T/bin" "$RARE_ROOT/home/tester" "$T/log"
touch "$T/log/systemd-run" "$T/log/systemctl"
export LOG="$T/log" PATH="$T/bin:$PATH"
cat >"$T/bin/systemctl" <<'S'
#!/bin/sh
echo "$*" >>"$LOG/systemctl"
case "$1" in
  is-active) grep -qx "active $*" "$LOG/active" 2>/dev/null; exit $? ;;
  is-enabled) exit 1 ;;
esac
exit 0
S
cat >"$T/bin/systemd-run" <<'S'
#!/bin/sh
echo "$*" >>"$LOG/systemd-run"
S
cat >"$T/bin/chpasswd" <<'S'
#!/bin/sh
echo "argv:$*" >>"$LOG/chpasswd"
cat >"$LOG/chpasswd.stdin"
S
cat >"$T/bin/passwd" <<'S'
#!/bin/sh
echo "tester P 2026-01-01 0 99999 7 -1"
S
cat >"$T/bin/firewall-cmd" <<'S'
#!/bin/sh
echo "$*" >>"$LOG/firewall"
case "$*" in
  *--get-default-zone*) echo public ;;
  *--get-active-zones*) printf 'public\n  interfaces: wlan0\n' ;;
  *--query-port*) grep -q -- "--add-port=${*##*--query-port=}" "$LOG/firewall" && ! grep -q -- "--remove-port=${*##*--query-port=}" "$LOG/firewall"; exit $? ;;
  *--query-service*) exit 1 ;;
esac
exit 0
S
chmod +x "$T/bin/"*
A="$lib/os-admin"
pass=0; fail=0
ok() { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1"; }
eq() { if [[ "$2" == "$3" ]]; then ok "$1"; else bad "$1 (got '$2', want '$3')"; fi; }
st() { "$A" ssh status | sed -n "s/^$1=//p"; }

gen() { ssh-keygen -q -t ed25519 -N '' -C "$2" -f "$T/$1" >/dev/null; }
gen k1 laptop; gen k2 phone

eq "fresh enabled" "$(st enabled)" no
eq "fresh active" "$(st active)" no
eq "fresh user" "$(st user)" tester
eq "fresh auto_off" "$(st auto_off_at)" none
eq "fresh has_password" "$(st has_password)" yes
eq "fresh keys" "$("$A" ssh status | grep -c '^key=')" 0
eq "status key order" "$("$A" ssh status | cut -d= -f1 | paste -sd' ')" "enabled active user password_auth keep_password has_password auto_off_at netapi default_password"

"$A" ssh on >/dev/null || bad "ssh on failed"
eq "on enabled" "$(st enabled)" yes
eq "on state file" "$(cat "$RARE_ROOT/var/lib/rare/ssh/state")" on
grep -q 'enable sshd.service' "$LOG/systemctl" && ok "on enables sshd" || bad "on enables sshd"
grep -q 'start --no-block sshd.service' "$LOG/systemctl" && ok "on starts sshd" || bad "on starts sshd"
grep -q MaxAuthTries "$RARE_ROOT/etc/ssh/sshd_config.d/01-rare-hardening.conf" && ok "hardening written" || bad "hardening written"
grep -q 'PermitRootLogin no' "$RARE_ROOT/etc/ssh/sshd_config.d/00-rare-auth.conf" && ok "root login off" || bad "root login off"
eq "no key: password on" "$(st password_auth)" yes

out="$("$A" ssh add-key <"$T/k1.pub")"; eq "add-key valid" "$out" "added=1 skipped=0"
eq "authorized_keys mode" "$(stat -c %a "$RARE_ROOT/home/tester/.ssh/authorized_keys" 2>/dev/null || stat -f %Lp "$RARE_ROOT/home/tester/.ssh/authorized_keys")" 600
eq "ssh dir mode" "$(stat -c %a "$RARE_ROOT/home/tester/.ssh" 2>/dev/null || stat -f %Lp "$RARE_ROOT/home/tester/.ssh")" 700
eq "key listed" "$(st key)" "$(ssh-keygen -lf "$T/k1.pub" | awk '{print $2 " ed25519 laptop"}')"
eq "password off after key" "$(st password_auth)" no
grep -q 'KbdInteractiveAuthentication no' "$RARE_ROOT/etc/ssh/sshd_config.d/00-rare-auth.conf" && ok "kbd-interactive off" || bad "kbd-interactive off"

eq "duplicate skipped" "$("$A" ssh add-key <"$T/k1.pub")" "added=0 skipped=1"
"$A" ssh add-key <<<"command=\"/bin/sh\" $(cat "$T/k2.pub")" 2>"$T/err" && bad "command= key accepted" || ok "command= key rejected"
"$A" ssh add-key <<<"from=\"1.2.3.4\",no-pty $(cat "$T/k2.pub")" 2>/dev/null && bad "options key accepted" || ok "options key rejected"
"$A" ssh add-key <<<"ssh-ed25519 AAAAnotakey junk" 2>/dev/null && bad "garbage key accepted" || ok "garbage key rejected"
"$A" ssh add-key <<<"hello world" 2>/dev/null && bad "garbage line accepted" || ok "garbage line rejected"
eq "bad batch adds nothing" "$("$A" ssh status | grep -c '^key=')" 1
printf '%s\r\n' "$(cat "$T/k2.pub")" | "$A" ssh add-key >/dev/null && eq "CR stripped, two keys" "$("$A" ssh status | grep -c '^key=')" 2 || bad "CRLF add"
head -c 17000 /dev/zero | tr '\0' a | "$A" ssh add-key 2>/dev/null && bad "oversize accepted" || ok "oversize rejected"
for _ in $(seq 51); do cat "$T/k1.pub"; done | "$A" ssh add-key 2>/dev/null && bad "51 keys accepted" || ok "51 keys rejected"

"$A" ssh keep-password on; eq "keep-password on" "$(st password_auth)" yes
eq "keep_password yes" "$(st keep_password)" yes
"$A" ssh keep-password off; eq "keep-password off" "$(st password_auth)" no

fp2="$(ssh-keygen -lf "$T/k2.pub" | awk '{print $2}')"; fp1="$(ssh-keygen -lf "$T/k1.pub" | awk '{print $2}')"
"$A" ssh remove-key "$fp2"; eq "remove one key" "$("$A" ssh status | grep -c '^key=')" 1
eq "still no password" "$(st password_auth)" no
"$A" ssh remove-key "$fp2" 2>/dev/null && bad "removed missing key" || ok "missing key errors"
"$A" ssh remove-key "bogus; rm -rf" 2>/dev/null && bad "bad fp accepted" || ok "bad fp rejected"
"$A" ssh remove-key "$fp1"; eq "last key removed" "$("$A" ssh status | grep -c '^key=')" 0
eq "password back on" "$(st password_auth)" yes

short=0; for p in "" "short" "1234567"; do printf '%s\n' "$p" | "$A" ssh set-password 2>/dev/null && short=1; done
eq "short passwords rejected" "$short" 0
printf '%s' "$(head -c 129 /dev/zero | tr '\0' a)" | "$A" ssh set-password 2>/dev/null && bad "long password accepted" || ok "long password rejected"
printf 'goodpass\nsecond\n' | "$A" ssh set-password 2>/dev/null && bad "two-line password accepted" || ok "two-line password rejected"
printf 'bad\001pass1234\n' | "$A" ssh set-password 2>/dev/null && bad "control char accepted" || ok "control chars rejected"
[[ ! -e "$LOG/chpasswd" ]] && ok "chpasswd untouched on bad input" || bad "chpasswd called on bad input"
printf 'Sup3r:secret-pass\n' | "$A" ssh set-password && ok "good password accepted" || bad "good password"
eq "chpasswd stdin" "$(cat "$LOG/chpasswd.stdin")" "tester:Sup3r:secret-pass"
grep -q 'secret' "$LOG/chpasswd" "$LOG/systemctl" "$LOG/systemd-run" && bad "password in argv" || ok "password never in argv"
eq "chpasswd argv empty" "$(cat "$LOG/chpasswd")" "argv:"

before=$(date +%s)
"$A" ssh auto-off on
at="$(st auto_off_at)"
[[ "$at" =~ ^[0-9]+$ ]] && (( at >= before + 3590 && at <= before + 3610 )) && ok "auto-off timestamp" || bad "auto-off timestamp ($at)"
grep -q -- '--on-active=3600s.*os-admin ssh off' "$LOG/systemd-run" && ok "auto-off timer scheduled" || bad "auto-off timer"
# keep-ssh reschedules for the time left
: >"$LOG/systemd-run"; "$lib/keep-ssh" >/dev/null
grep -qE -- '--on-active=3[0-9]{3}s' "$LOG/systemd-run" && ok "keep-ssh reschedules" || bad "keep-ssh reschedules"
# ... and turns ssh off if the time has passed
echo $(( $(date +%s) - 5 )) >"$RARE_ROOT/var/lib/rare/ssh/auto-off-at"; "$lib/keep-ssh" >/dev/null
eq "keep-ssh expired auto-off" "$(st enabled)/$(st auto_off_at)" "no/none"
"$A" ssh on >/dev/null; "$A" ssh auto-off on; "$A" ssh auto-off off
eq "auto-off cancelled" "$(st auto_off_at)" none
grep -q 'stop rare-ssh-autooff.timer' "$LOG/systemctl" && ok "timer stopped" || bad "timer stopped"
"$A" ssh auto-off on

"$A" ssh off
eq "off enabled" "$(st enabled)" no
eq "off auto_off cleared" "$(st auto_off_at)" none
[[ ! -e "$RARE_ROOT/var/lib/rare/ssh/auto-off-at" ]] && ok "marker removed" || bad "marker removed"
grep -q 'disable --now sshd.service' "$LOG/systemctl" && ok "off disables sshd" || bad "off disables sshd"

# The first start of Rare OS (no state file) is off, even when the previous system had sshd on
rm -f "$RARE_ROOT/var/lib/rare/ssh/state"; mkdir -p "$RARE_ROOT/etc/systemd/system/multi-user.target.wants"; touch "$RARE_ROOT/etc/systemd/system/multi-user.target.wants/sshd.service"
: >"$LOG/systemctl"; "$lib/keep-ssh" >/dev/null
eq "first start: ssh off although sshd was enabled" "$(st enabled)" "no"
eq "first start writes state off" "$(cat "$RARE_ROOT/var/lib/rare/ssh/state")" off
grep -qE '^(enable|start)' "$LOG/systemctl" && bad "first start started sshd" || ok "first start leaves sshd off"
rm -f "$RARE_ROOT/etc/systemd/system/multi-user.target.wants/sshd.service"
# The owner's device: SSH on, keep-password yes, own password: password sign-in stays on
"$A" ssh on >/dev/null; "$A" ssh add-key <"$T/k1.pub" >/dev/null; "$A" ssh keep-password on
"$lib/keep-ssh" >/dev/null
eq "owner device: still on" "$(st enabled)" yes
eq "owner device: password sign-in kept" "$(st password_auth)" yes
eq "owner device: not a default password" "$(st default_password)" no
# GameOS's default password: password sign-in off even with keep-password, until it changes
shadow_pw playtron
"$lib/keep-ssh" >/dev/null
eq "default password detected" "$(st default_password)" yes
eq "default password: password sign-in off" "$(st password_auth)" no
printf 'playtron\n' | "$A" ssh set-password 2>/dev/null && bad "default password accepted again" || ok "default password refused"
printf 'a-new-pass-123\n' | "$A" ssh set-password && shadow_pw "a-new-pass-123" && "$lib/keep-ssh" >/dev/null
eq "new password: password sign-in back" "$(st password_auth)" yes
"$A" ssh keep-password off; "$A" ssh remove-key "$(ssh-keygen -lf "$T/k1.pub" | awk '{print $2}')"; "$A" ssh off >/dev/null
# keep-ssh with state off must not enable or start sshd
: >"$LOG/systemctl"
"$lib/keep-ssh"
grep -qE '^(enable|start)' "$LOG/systemctl" && bad "keep-ssh (off) enabled/started sshd" || ok "keep-ssh off leaves sshd off"
grep -q 'disable --now sshd.service' "$LOG/systemctl" && ok "keep-ssh off disables sshd" || bad "keep-ssh off disables sshd"
rm -rf "$RARE_ROOT/var/lib/rare/ssh"; : >"$LOG/systemctl"; "$lib/keep-ssh"
grep -qE '^(enable|start)' "$LOG/systemctl" && bad "keep-ssh (no state) started sshd" || ok "keep-ssh without state is off"
"$A" ssh on >/dev/null; : >"$LOG/systemctl"; "$lib/keep-ssh" >/dev/null
grep -q 'start --no-block sshd.service' "$LOG/systemctl" && ok "keep-ssh on starts sshd" || bad "keep-ssh on starts sshd"

# sshd -t failure removes the drop-in and fails
RARE_SSHD_TEST_CMD=false "$A" ssh keep-password on 2>/dev/null && bad "bad sshd config not reported" || ok "failed sshd -t exits non-zero"
[[ ! -e "$RARE_ROOT/etc/ssh/sshd_config.d/00-rare-auth.conf" ]] && ok "drop-in removed on failure" || bad "drop-in removed on failure"
"$A" ssh keep-password off

# netapi
for p in 80 1023 65536 abc "" "12 34" "8080;x" -5 "0x1F90"; do "$A" netapi on "$p" 2>/dev/null && bad "netapi accepted '$p'" || ok "netapi rejects '$p'"; done
"$A" netapi on 8088; eq "netapi file" "$(cat "$RARE_ROOT/var/lib/rare/ssh/netapi-port")" 8088
eq "netapi no firewall" "$(st netapi)" no
unset RARE_NO_FIREWALL
echo "active is-active -q firewalld.service" >"$LOG/active"
"$A" netapi on 8088
grep -q -- '--permanent --zone=public --add-port=8088/tcp' "$LOG/firewall" && ok "netapi permanent rule" || bad "netapi permanent rule"
grep -q -- '^--zone=public --add-port=8088/tcp' "$LOG/firewall" && ok "netapi runtime rule" || bad "netapi runtime rule"
eq "netapi=yes" "$(st netapi)" yes
"$A" netapi off 8088
eq "netapi=no after off" "$(st netapi)" no
[[ ! -e "$RARE_ROOT/var/lib/rare/ssh/netapi-port" ]] && ok "netapi file removed" || bad "netapi file removed"
: >"$LOG/active"; "$A" netapi on 9000 && ok "netapi no-op without firewalld" || bad "netapi no-op without firewalld"

echo; echo "$pass passed, $fail failed"
[[ $fail == 0 ]]
