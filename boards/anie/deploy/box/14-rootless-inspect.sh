#!/bin/bash
# Anie Solutions BBS on tkaniedev03: READ-ONLY. What the box has for the board's move to a Docker of
# its own (rootless, as the user anie-bbs): whether Ubuntu restricts unprivileged user namespaces and
# which AppArmor profiles let rootlesskit and slirp4netns through, the tools rootless Docker needs,
# the subordinate id ranges already given out (names and ranges only), systemd's version and the
# cgroup controllers it hands a user's manager, whether anie-bbs exists yet, and what of the BBS
# lives where now. Changes nothing; prints no secret.
set -u
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

say() { printf 'inspect: %s\n' "$*"; }

say "kernel $(uname -r), $(. /etc/os-release && echo "$PRETTY_NAME")"
say "restrict unprivileged userns: $(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>/dev/null || echo unset), unprivileged userns clone: $(sysctl -n kernel.unprivileged_userns_clone 2>/dev/null || echo unset), max user namespaces: $(sysctl -n user.max_user_namespaces 2>/dev/null)"
for p in rootlesskit slirp4netns; do
  files="$(grep -lsE "/usr/(local/)?s?bin/$p\b|^profile $p\b" /etc/apparmor.d/* 2>/dev/null | paste -sd ' ' -)"
  say "apparmor profile for $p: ${files:-none} loaded: $(grep -c "$p" /sys/kernel/security/apparmor/profiles 2>/dev/null || echo ?)"
done
for t in rootlesskit dockerd-rootless.sh dockerd-rootless-setuptool.sh slirp4netns newuidmap newgidmap pasta fuse-overlayfs; do
  say "tool $t: $(command -v "$t" || echo missing)"
done
say "rootlesskit $(rootlesskit --version 2>/dev/null | head -n 1)"
say "subuid: $(cut -d: -f1-3 /etc/subuid 2>/dev/null | paste -sd ' ' -)"
say "subgid: $(cut -d: -f1-3 /etc/subgid 2>/dev/null | paste -sd ' ' -)"
say "systemd $(systemctl --version | head -n 1)"
say "cgroup: $(stat -fc %T /sys/fs/cgroup), controllers $(cat /sys/fs/cgroup/cgroup.controllers 2>/dev/null)"
say "user@ delegates: $(systemctl show -P Delegate user@.service 2>/dev/null) $(systemctl show -P DelegateControllers user@.service 2>/dev/null)"
for d in /etc/systemd/system/user@.service.d /etc/systemd/system/user@*.service.d; do [ -d "$d" ] && say "drop-ins $d: $(ls "$d" | paste -sd ' ' -)"; done
if id anie-bbs >/dev/null 2>&1; then say "user anie-bbs: $(id anie-bbs), home $(getent passwd anie-bbs | cut -d: -f6)"; else say "user anie-bbs: none yet"; fi
say "uids 10001 10002 on the host: $(getent passwd 10001 | cut -d: -f1) $(getent passwd 10002 | cut -d: -f1)"
say "/srv: $(ls -A /srv 2>/dev/null | paste -sd ' ' -)"
say "lingering: $(ls /var/lib/systemd/linger 2>/dev/null | paste -sd ' ' -)"
say "root's docker: $(docker info --format '{{.ServerVersion}} {{.Driver}} rootless={{range .SecurityOptions}}{{.}} {{end}}' 2>&1)"
say "root's containers: $(docker ps --format '{{.Names}}' | sort | paste -sd ' ' -)"
say "the BBS's volumes: $(docker volume ls --format '{{.Name}}' | grep '^anie-bbs_' | paste -sd ' ' -)"
for v in anie-bbs_db anie-bbs_home anie-bbs_doors-kept; do
  say "volume $v: $(du -sh "$(docker volume inspect --format '{{.Mountpoint}}' "$v" 2>/dev/null)" 2>/dev/null | cut -f1)"
done
say "/root/anie-bbs: $(ls -A /root/anie-bbs | paste -sd ' ' -)"
say "the clone: $(stat -c '%U %a' /home/bryan/flow-bbs 2>/dev/null)"
say "ufw 2222: $(ufw status 2>/dev/null | grep -E '^2222' | paste -sd ';' -)"
say "listening on 2222: $(ss -ltnpH 'sport = :2222' 2>/dev/null | awk '{print $4, $6}' | paste -sd ';' -)"
say "done"
