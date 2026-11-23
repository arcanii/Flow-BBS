#!/bin/bash
# Anie Solutions BBS on tkaniedev03: READ-ONLY. How a game of Dungeon just played went: the BBS's
# containers (restarted or not since), the board's and the doors' log lines of the last twelve
# hours, the logins and other doings the audit kept in that time (handles, never an address), each
# member's kept Dungeon folder (its files, sizes and times), anything a call left in the doors'
# temporary folder, what runs in the doors' container now, and the system disk since this boot.
# Changes nothing. Prints no secret and no caller's address.
set -u
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
cd /

BBS=anie-bbs
BHOME=/srv/anie-bbs
BUID="$(id -u "$BBS" 2>/dev/null || echo none)"
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
d() { as_bbs docker "$@"; }
say() { printf 'check: %s\n' "$*"; }
sql() { d exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -F ' | ' -c "$1" 2>&1 | head -n 40; }
doors() { d exec anie-bbs-doors-1 sh -c "$1" 2>&1; }

say "at $(date -u +%Y-%m-%dT%H:%M:%SZ), up since $(uptime -s)"
for c in anie-bbs-db-1 anie-bbs-doors-1 anie-bbs-bbs-1; do
  say "$c $(d inspect --format '{{.State.Status}} since {{.State.StartedAt}} restarts={{.RestartCount}} oom={{.State.OOMKilled}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}-{{end}}' "$c" 2>&1)"
done
say "doors' use now: $(d stats --no-stream --format '{{.MemUsage}} pids {{.PIDs}}' anie-bbs-doors-1 2>&1)"

sql "SELECT to_char(a.at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') || 'Z', coalesce(m.handle, '-'), a.action, a.subject FROM audit a LEFT JOIN members m ON m.id = a.actor_id WHERE a.at > now() - interval '12 hours' ORDER BY a.at" | sed 's/^/check: audit: /'
sql "SELECT id, handle, status, role FROM members ORDER BY id" | sed 's/^/check: member: /'

d logs --timestamps --since 12h anie-bbs-bbs-1 2>&1 | tail -n 40 | sed 's/^/check: board log: /'
d logs --timestamps --since 12h anie-bbs-doors-1 2>&1 | tail -n 40 | sed 's/^/check: doors log: /'

doors 'for f in /var/lib/anie-doors/dungeon/*; do [ -d "$f" ] || continue; echo "folder $(basename "$f") $(stat -c "%a %u" "$f")"; for g in "$f"/* "$f"/.[!.]*; do [ -e "$g" ] && echo "  $(basename "$g") $(stat -c "%s bytes, changed %y" "$g")"; done; done' | sed 's/^/check: kept dungeon: /'
doors 'ls -la /tmp 2>&1; df -h /tmp /var/lib/anie-doors 2>&1 | tail -n 2' | sed 's/^/check: doors tmp: /'
doors 'for p in /proc/[0-9]*; do [ -r "$p/comm" ] && printf "%s(%s) " "$(cat "$p/comm")" "$(basename "$p")"; done; echo' | sed 's/^/check: doors running: /'

say "root filesystem $(findmnt -no OPTIONS / | cut -d, -f1), $(df -h --output=avail / | tail -n 1 | tr -d ' ') free"
journalctl -k -b --no-pager -o short-iso 2>/dev/null | grep -i -E 'nvme|I/O error|EXT4-fs (error|warning)|blk_update|oom-kill|out of memory|killed process|segfault' | tail -n 15 | sed 's/^/check: kernel this boot: /'
say "kernel lines of note this boot: $(journalctl -k -b --no-pager 2>/dev/null | grep -c -i -E 'I/O error|EXT4-fs error|blk_update|oom-kill|segfault')"
say "done"
