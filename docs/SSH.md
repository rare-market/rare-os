# Remote access (SSH) and network apps

Rare OS can be opened up for people who want to mod it, copy files, run tools or let an AI agent work
on the device. Everything is off until the owner turns it on, in Settings on the device.

## Turn it on

Settings > Developer > Remote access (SSH), or the same switch in Settings > System. The switch starts
sshd and keeps it on at every start. No password prompt: the launcher asks the image's helper, which
polkit allows for the person at the device only (not for anyone signed in over SSH).

When it is on, Settings shows:

- the command to copy, for example `ssh bazzite@suiplay.local`, with a Copy button and a QR code of the
  same command (scan it with a phone to read or share it)
- the device's name (`suiplay.local`), its IP address on the network, and the user name
- the warning "Anyone on your network with the password can control this device."

## Sign in: a key or a password

Keys are recommended. Password sign-in turns off by itself as soon as a key exists, and comes back if
the last key is removed. "Keep password sign-in on" overrides that.

- Set a password: the on-screen keyboard (8 to 128 characters). It is the device user's password.
- Add a key:
  - Paste or type: the keyboard opens with a public key from the clipboard already in it, if there is one
  - USB stick: reads `authorized_keys` or any `*.pub` file from the top folder (or `.ssh`) of a mounted drive
  - GitHub: type a user name, the device fetches `https://github.com/<user>.keys` and asks you to
    confirm before adding anything. Whoever controls that account can then sign in.
- Each key is listed with its fingerprint and a Remove button (with a confirmation).

Only plain public keys are accepted. A line with options in front, such as `command="..."`, is refused.

## Safety

- Off by default. The first start of Rare OS (no `/var/lib/rare/ssh/state` yet) turns it off even when
  the system it was switched from had it on (GameOS's Remote Access, password `playtron`). A device
  whose owner already chose in Rare OS keeps that choice.
- While the password is still a default one (`playtron`), password sign-in stays off, keep-password or
  not, and Settings says so; a new password turns it back on. `os-admin ssh set-password` refuses the
  default. `ssh status` reports `default_password=yes|no` (the hash is checked with libcrypt).
- Optional "Turn off after 1 hour". The timer survives a restart.
- A terminal icon shows in the top bar while SSH is on.
- Failed logins are limited by sshd: 3 tries per connection, 20 seconds to sign in, at most 4 unfinished
  connections at once (`MaxAuthTries`, `LoginGraceTime`, `MaxStartups`). fail2ban is not in the image;
  if it is installed, `ssh on` adds an sshd jail (5 tries in 10 minutes, 15 minute ban).
- Root login is off.

## For the AI era: developer tools and network apps

Settings > Developer > Developer tools points at the Local API (`docs/LOCAL-API.md`) and `@rare/sdk`
(`rare-sdk`).

"Allow network apps" (off by default) lets tools and agents on a laptop call the device's Local API:

- It listens on port 47100 on the local network, and the helper opens that port in the firewall
  (`os-admin netapi on 47100`). Turning it off closes the listener and the port.
- Auth is the Local API's: `Authorization: Bearer <token>`. The token is made on the device when you
  first turn this on and stored in `~/.config/rare/network-api-token` (readable by the owner only).
  Settings shows it with Copy and New token (the old one stops working at once).
- `Host` must be the device's IP address or its own name (`suiplay.local`), which stops DNS rebinding.
  Any request with an `Origin` header (a web page) is refused.
- The network listener has its own token, rate limits and lockout, so a stranger guessing tokens can't
  lock games out of the loopback API.
- Scope: every capability is a separate switch under Settings > Developer, all off until you turn them on:
  Read profile, Read inventory and library, Launch and close games and apps, Send controller input, Request
  purchases and listings, Screenshots. A call whose switch is off gets `403 capability_off`. A buy or sell
  request always opens the review sheet on the device and nothing is signed without a press there. Claim,
  notices and the overlay stay with games on the device. While a remote sends controller input, the top bar
  shows a controller icon.
- Audit: each call writes a `[netapi] cap=...` line to the log, and Settings > Developer > Recent network
  activity shows the latest ones.
- Endpoints, limits and the input format are in `docs/LOCAL-API.md` (Network apps). A small client for a
  laptop or an agent is in `tools/netapi-example/rare_netapi.py`.

```
curl -H "Authorization: Bearer $TOKEN" http://suiplay.local:47100/v1/capabilities
curl -H "Authorization: Bearer $TOKEN" http://suiplay.local:47100/v1/me      # needs Read profile
curl -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"button":"a"}' http://suiplay.local:47100/v1/input                  # needs Send controller input
```

## Where it lives

- Settings screens: `launcher/native/src/screens/settings/remote.rs` (rows), `live.rs` (actions)
- Device calls: `launcher/crates/rare-system/src/ssh.rs`
- LAN listener: `launcher/crates/rare-localapi` (`network_enable`, `gate_network`, `CAPS`); virtual pad: `launcher/crates/rare-input/src/remote.rs`; calls: `launcher/native/src/features/localapi.rs`
- Image: below

## Image and helper

SSH is off by default. Settings > System turns it on through the privileged helper
`/usr/libexec/rare/os-admin` (run as `pkexec --disable-internal-agent /usr/libexec/rare/os-admin <args>`;
polkit action `org.rare.os.admin`, local active session only). Passwords and keys are read from
stdin, never argv. The logic lives in `/usr/libexec/rare/ssh-lib.sh`, shared with `keep-ssh`.

State is in `/var/lib/rare/ssh/` (0755 root): `state` (`on` or `off`; absent means off),
`keep-password` (`yes`), `auto-off-at` (epoch seconds), `netapi-port`.

Commands:

- `os-admin ssh on`: unmask, enable and start sshd, allow ssh in the firewall zones, write state `on`,
  install the hardening drop-in, apply the login policy. If `/usr/bin/fail2ban-client` exists it also
  writes `/etc/fail2ban/jail.d/rare-sshd.local` and enables fail2ban (nothing is installed for this).
- `os-admin ssh off`: stop and disable sshd and `rare-sshd-fallback.service`, state `off`, cancel the
  auto-off timer, remove `auto-off-at`.
- `os-admin ssh status`: key=value lines in this order: `enabled`, `active`, `user`, `password_auth`,
  `keep_password`, `has_password`, `auto_off_at` (epoch or `none`), `netapi`, then zero or more
  `key=<SHA256:fingerprint> <type> <comment or ->`. The type is ssh-keygen's name in lower case
  (`ed25519`, `rsa`, `ecdsa`, `ed25519-sk`, `ecdsa-sk`).
- `os-admin ssh set-password`: one line on stdin, 8 to 128 characters, no control characters, no
  second line. Sets it with chpasswd for the login user. Exit 1 with one line on stderr otherwise.
- `os-admin ssh add-key`: public key lines on stdin. Plain keys only (ssh-ed25519, ssh-rsa,
  ecdsa-sha2-*, sk-ssh-ed25519@openssh.com, sk-ecdsa-sha2-nistp256@openssh.com); a line with options
  such as `command=` is rejected, every line is checked with `ssh-keygen -l`, CR is stripped, at most
  50 keys and 16 KiB per call. One bad line rejects the whole call and adds nothing. Duplicates (by
  fingerprint) are skipped. Prints `added=<n> skipped=<n>`.
- `os-admin ssh remove-key <SHA256:fingerprint>`, `os-admin ssh keep-password on|off`,
  `os-admin ssh auto-off on|off` (one hour, transient unit `rare-ssh-autooff`; requires SSH on).
- `os-admin netapi on|off <port>`: opens or closes a TCP port (1024 to 65535) in the default zone,
  the active zones and NetworkManager zones, runtime and permanent. It does nothing if firewalld is
  not running. `ssh status` reports `netapi=yes` when the port is open, or when no firewall runs.

Login policy: `/etc/ssh/sshd_config.d/00-rare-auth.conf` sets `PasswordAuthentication` and
`KbdInteractiveAuthentication` to `no` when the user has at least one key and keep-password is not
`yes`, or when the password is a default one, otherwise `yes`, plus `PermitRootLogin no`. It is named `00-` because sshd keeps the first
value it reads and the base ships `40-redhat-crypto-policies.conf` and `50-redhat.conf`. The file is
checked with `sshd -t` and removed (exit non-zero) if the check fails; sshd is reloaded if it is
running. Removing the last key turns password login back on.

Hardening: `/etc/ssh/sshd_config.d/01-rare-hardening.conf` (shipped in the image, rewritten by
`ssh on`): MaxAuthTries 3, LoginGraceTime 20, MaxStartups 4:50:10, PermitEmptyPasswords no,
ClientAliveInterval 60, ClientAliveCountMax 3.

Boot: `rare-keep-ssh.service` runs `keep-ssh`. State not `on`: sshd is disabled and stopped. State
`on`: sshd is kept enabled and reachable (fallback sshd with the image config if /etc config does not
load), the login policy is applied again, and an auto-off timer is rescheduled for the time left or
SSH is turned off if the time has passed. This replaces the old policy that forced SSH on at every
boot; `10-rare.preset` now says `disable sshd.service`.

Tests: `image/tests/os-admin-ssh.sh` runs the helper against a temp root with stubbed systemctl,
chpasswd, passwd, systemd-run and firewall-cmd. Overrides: `RARE_ROOT`, `RARE_SYSTEMCTL`,
`RARE_SYSTEMD_RUN`, `RARE_NO_FIREWALL=1`, `RARE_SSHD_TEST_CMD`, `RARE_SSH_USER`, `RARE_SELF`,
`RARE_LIB_DIR`, `RARE_FAIL2BAN_CLIENT`.
