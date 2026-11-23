#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 15, once (running it again changes nothing): a Docker of
# the BBS's own, rootless, run by a user of its own, so that nothing of the BBS runs as root and a
# container broken out of is that user, with the BBS's files and nothing else of the machine's.
#
# - The user anie-bbs (a system user, no login shell), its home /srv/anie-bbs (mode 700).
# - Subordinate ids for it, 300000-365535, apart from every other user's (refused if not).
# - Its user manager, kept running without a login (linger), handed the cgroup controllers that
#   compose's limits need (a drop-in for its user@ unit alone).
# - Rootless Docker for it (a user unit as dockerd-rootless-setuptool.sh makes), with slirp4netns for its network and
#   its published ports, so the board sees callers' own addresses.
#
# Needs uidmap (newuidmap, newgidmap). Ubuntu's own AppArmor profiles let rootlesskit and
# slirp4netns make their namespaces. Touches nothing of the BBS that runs now. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

BBS=anie-bbs
BHOME=/srv/anie-bbs
FIRST=300000
COUNT=65536
say() { printf 'rootless: %s\n' "$*"; }

for t in newuidmap newgidmap dockerd-rootless.sh rootlesskit slirp4netns; do
  command -v "$t" >/dev/null || { say "REFUSING: $t isn't here (uidmap, docker-ce-rootless-extras, slirp4netns)"; exit 1; }
done

if id "$BBS" >/dev/null 2>&1; then
  say "user $BBS there: $(id "$BBS")"
else
  useradd --system --user-group --create-home --home-dir "$BHOME" --shell /usr/sbin/nologin "$BBS"
  say "user $BBS made: $(id "$BBS")"
fi
chmod 700 "$BHOME"
BUID="$(id -u "$BBS")"

# Its subordinate ids, clear of everyone else's.
last=$((FIRST + COUNT - 1))
for f in /etc/subuid /etc/subgid; do
  if awk -F: -v me="$BBS" -v a="$FIRST" -v b="$last" '$1 != me && $2 <= b && $2 + $3 - 1 >= a { bad = 1 } END { exit !bad }' "$f"; then
    say "REFUSING: $FIRST-$last overlaps another user's range in $f"
    exit 1
  fi
done
grep -q "^$BBS:" /etc/subuid || usermod --add-subuids "$FIRST-$last" "$BBS"
grep -q "^$BBS:" /etc/subgid || usermod --add-subgids "$FIRST-$last" "$BBS"
say "subuid $(grep "^$BBS:" /etc/subuid), subgid $(grep "^$BBS:" /etc/subgid)"

# Its manager, with the controllers for memory, processes and the rest, kept running.
dropin="/etc/systemd/system/user@$BUID.service.d"
if [ ! -f "$dropin/delegate.conf" ]; then
  install -d -m 755 "$dropin"
  printf '[Service]\nDelegate=cpu cpuset io memory pids\n' > "$dropin/delegate.conf"
  systemctl daemon-reload
  if systemctl is-active --quiet "user@$BUID.service"; then systemctl restart "user@$BUID.service"; fi
fi
loginctl enable-linger "$BBS"
for i in $(seq 1 30); do [ -S "/run/user/$BUID/systemd/private" ] && break; sleep 1; done
[ -S "/run/user/$BUID/systemd/private" ] || { say "PROBLEM: $BBS's user manager didn't start"; exit 1; }
say "controllers for $BBS: $(cat "/sys/fs/cgroup/user.slice/user-$BUID.slice/user@$BUID.service/cgroup.controllers" 2>/dev/null)"

as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$BUID/bus" PATH=/usr/bin:/bin:/usr/sbin:/sbin "$@"; }

# Its Docker, as dockerd-rootless-setuptool.sh would set it up (its unit written here, not by the
# tool, whose checks look for AppArmor profiles by other names than Ubuntu's): slirp4netns for the
# network and for published ports, which keeps callers' addresses.
as_bbs mkdir -p "$BHOME/.config/systemd/user"
as_bbs tee "$BHOME/.config/systemd/user/docker.service" >/dev/null <<'UNIT'
[Unit]
Description=Docker Application Container Engine (Rootless), the Anie Solutions BBS's
Documentation=https://docs.docker.com/go/rootless/

[Service]
Environment=PATH=/usr/bin:/bin:/usr/sbin:/sbin
Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_NET=slirp4netns
Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_PORT_DRIVER=slirp4netns
ExecStart=/usr/bin/dockerd-rootless.sh
ExecReload=/bin/kill -s HUP $MAINPID
TimeoutSec=0
RestartSec=2
Restart=always
StartLimitBurst=3
StartLimitInterval=60s
LimitNOFILE=infinity
LimitNPROC=infinity
LimitCORE=infinity
TasksMax=infinity
Delegate=yes
Type=notify
NotifyAccess=all
KillMode=mixed

[Install]
WantedBy=default.target
UNIT
as_bbs systemctl --user daemon-reload
as_bbs systemctl --user enable docker.service >/dev/null 2>&1
as_bbs systemctl --user restart docker.service
for i in $(seq 1 30); do as_bbs docker -H "unix:///run/user/$BUID/docker.sock" info >/dev/null 2>&1 && break; sleep 1; done
say "docker for $BBS: $(as_bbs docker -H "unix:///run/user/$BUID/docker.sock" info --format '{{.ServerVersion}} {{.Driver}} {{.CgroupVersion}} {{range .SecurityOptions}}{{.}} {{end}}' 2>&1)"
say "its network: $(as_bbs systemctl --user show -P Environment docker.service | tr ' ' '\n' | grep ROOTLESSKIT | paste -sd ' ' -)"
say "done"
