#!/bin/bash
# Anie Solutions BBS on tkaniedev03: READ-ONLY. How the board is doing, for its upkeep: the
# containers, the members by status, the last backup, disk, and the board's recent log lines (which
# hold no secret and no member's address). Changes nothing.
set -u
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

REPO=/home/bryan/flow-bbs
say() { printf 'status: %s\n' "$*"; }
sql() { docker exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -c "$1" 2>&1 | head -n 1; }

say "at $(date -u +%Y-%m-%dT%H:%M:%SZ), clone at $(git -c safe.directory="$REPO" -C "$REPO" rev-parse --short HEAD 2>/dev/null)"
for c in anie-bbs-db-1 anie-bbs-bbs-1; do
  say "$c $(docker inspect --format '{{.State.Status}} since {{.State.StartedAt}} restarts={{.RestartCount}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}-{{end}}' "$c" 2>/dev/null || echo absent)"
done
if ss -ltnH "sport = :2222" | grep -q .; then say "port 2222 listening"; else say "port 2222 NOT listening"; fi
say "members $(sql "SELECT string_agg(status || '=' || n, ' ') FROM (SELECT status, count(*) n FROM members GROUP BY status ORDER BY status) s")"
say "roles $(sql "SELECT string_agg(role || '=' || n, ' ') FROM (SELECT role, count(*) n FROM members WHERE status = 'active' GROUP BY role ORDER BY role) s")"
say "posts $(sql "SELECT count(*) FROM posts") bugs-open $(sql "SELECT count(*) FROM bugs WHERE status IN ('new', 'confirmed', 'in-progress')")"
say "logins-24h $(sql "SELECT count(*) FROM audit WHERE action = 'login' AND at > now() - interval '1 day'") failed-24h $(sql "SELECT count(*) FROM audit WHERE action = 'login failed' AND at > now() - interval '1 day'")"
say "db-size $(sql "SELECT pg_size_pretty(pg_database_size('anie_bbs'))")"
last="$(ls -1t /root/anie-bbs/backups/*.dump 2>/dev/null | head -n 1)"
if [ -n "$last" ]; then say "last backup $(basename "$last") $(stat -c '%s bytes' "$last")"; else say "no backup yet"; fi
say "backup timer $(systemctl is-active anie-bbs-backup.timer 2>/dev/null)"
say "disk-free-root $(df -h --output=avail / | tail -n 1 | tr -d ' ')"
docker logs --tail 15 anie-bbs-bbs-1 2>&1 | sed 's/^/status: log: /'
say "done"
