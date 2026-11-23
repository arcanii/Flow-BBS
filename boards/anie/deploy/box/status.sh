#!/bin/bash
# Anie Solutions BBS on tkaniedev03: READ-ONLY. How the board is doing, for its upkeep: the BBS's own
# Docker (anie-bbs's, rootless) and its containers, the members by status, the last backup, disk,
# and the board's recent log lines (which hold no secret and no member's address). Changes nothing.
set -u
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
cd /

BBS=anie-bbs
BHOME=/srv/anie-bbs
REPO=$BHOME/flow-bbs
BUID="$(id -u "$BBS" 2>/dev/null || echo none)"
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
d() { as_bbs docker "$@"; }
say() { printf 'status: %s\n' "$*"; }
sql() { d exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -c "$1" 2>&1 | head -n 1; }

say "at $(date -u +%Y-%m-%dT%H:%M:%SZ), clone at $(as_bbs git -C "$REPO" rev-parse --short HEAD 2>/dev/null)"
say "docker of $BBS: $(d info --format '{{.ServerVersion}} {{range .SecurityOptions}}{{.}} {{end}}' 2>&1 | grep -o 'rootless' || echo NOT rootless), root's docker runs $(docker ps -q --filter name=anie-bbs- 2>/dev/null | wc -l) of the BBS's containers"
for c in anie-bbs-db-1 anie-bbs-doors-1 anie-bbs-bbs-1; do
  say "$c $(d inspect --format '{{.State.Status}} since {{.State.StartedAt}} restarts={{.RestartCount}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}-{{end}}' "$c" 2>/dev/null || echo absent)"
done
say "flow-games at $(git -c safe.directory=/root/anie-bbs/flow-games -C /root/anie-bbs/flow-games rev-parse --short HEAD 2>/dev/null || echo absent), doors $(d exec anie-bbs-doors-1 flow-mines --version 2>&1 | head -n 1), $(d exec anie-bbs-doors-1 sh -c 'command -v flow-cat; command -v dungeon' 2>&1 | paste -sd ' ' -)"
say "doors as uid $(d exec anie-bbs-doors-1 id -u 2>&1), network $(d inspect --format '{{.HostConfig.NetworkMode}}' anie-bbs-doors-1 2>&1), socket $(d exec anie-bbs-doors-1 sh -c 'test -S /run/anie-doors/doors.sock && echo there || echo NOT there'), dungeon-kept $(d exec anie-bbs-doors-1 sh -c 'ls /var/lib/anie-doors/dungeon 2>/dev/null | wc -l'), board's own doors $(d exec anie-bbs-bbs-1 sh -c 'command -v flow-cat flow-mines dungeon || echo none' 2>&1 | paste -sd ' ' -)"
say "mines-best $(sql "SELECT count(*) FROM mines_best")"
if ss -ltnH "sport = :2222" | grep -q .; then say "port 2222 listening, by $(ss -ltnpH 'sport = :2222' | grep -o 'users:(("[^"]*"' | cut -d'"' -f2 | sort -u | paste -sd ' ' -) as $(ps -o user= -p "$(ss -ltnpH 'sport = :2222' | grep -o 'pid=[0-9]*' | head -n 1 | cut -d= -f2)" 2>/dev/null)"; else say "port 2222 NOT listening"; fi
say "members $(sql "SELECT string_agg(status || '=' || n, ' ') FROM (SELECT status, count(*) n FROM members GROUP BY status ORDER BY status) s")"
say "roles $(sql "SELECT string_agg(role || '=' || n, ' ') FROM (SELECT role, count(*) n FROM members WHERE status = 'active' GROUP BY role ORDER BY role) s")"
say "posts $(sql "SELECT count(*) FROM posts") bugs-open $(sql "SELECT count(*) FROM bugs WHERE status IN ('new', 'confirmed', 'in-progress')")"
say "logins-24h $(sql "SELECT count(*) FROM audit WHERE action = 'login' AND at > now() - interval '1 day'") failed-24h $(sql "SELECT count(*) FROM audit WHERE action = 'login failed' AND at > now() - interval '1 day'")"
say "db-size $(sql "SELECT pg_size_pretty(pg_database_size('anie_bbs'))")"
last="$(ls -1t /root/anie-bbs/backups/*.dump 2>/dev/null | head -n 1)"
if [ -n "$last" ]; then say "last backup $(basename "$last") $(stat -c '%s bytes' "$last")"; else say "no backup yet"; fi
say "backup timer $(systemctl is-active anie-bbs-backup.timer 2>/dev/null)"
say "disk-free-root $(df -h --output=avail / | tail -n 1 | tr -d ' ')"
d logs --tail 15 anie-bbs-bbs-1 2>&1 | sed 's/^/status: log: /'
say "done"
