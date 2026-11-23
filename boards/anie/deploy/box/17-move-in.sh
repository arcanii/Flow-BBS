#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 17, once: the BBS moved from root's Docker into its own
# (anie-bbs's, rootless: 15-rootless.sh), after 02, 11, 03 and 04 have made its clone, its Flow-Games
# tree, its secrets and its images there; 06-start.sh starts it after. In root's Docker the board and
# its doors are stopped for good (their restart policy taken off, so a reboot doesn't bring them back
# to fight for 2222), the database's rows counted and dumped, then it's stopped too; the board's
# volume (its host key) and the doors' kept games are copied as they are, owners and all. In the
# BBS's own Docker everything is made but not started, those copies put in its new volumes, and the
# database started alone and the dump restored into it; its rows must come to what they were. The
# copies are root's, in /root/anie-bbs/move-in; root's containers, images and volumes are left as
# they are (18-move-back.sh brings them back) until they're cleared by hand. Refuses to run twice,
# or over a database the BBS's own Docker has already. Prints counts, never a secret or a member.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 077
cd /

BBS=anie-bbs
BHOME=/srv/anie-bbs
REPO=$BHOME/flow-bbs
BUID="$(id -u "$BBS")"
MOVE=/root/anie-bbs/move-in
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
compose() { as_bbs docker compose --env-file "$BHOME/secrets/bbs.env" -f "$REPO/boards/anie/deploy/compose.yaml" "$@"; }
say() { printf 'move-in: %s\n' "$*"; }
ROWS="SELECT (SELECT count(*) FROM members) || ' ' || (SELECT count(*) FROM posts) || ' ' || (SELECT count(*) FROM bugs) || ' ' || (SELECT count(*) FROM audit) || ' ' || (SELECT count(*) FROM mines_best) || ' ' || (SELECT max(version) FROM schema_migrations)"

if [ -e "$MOVE/moved-in" ]; then
  say "already moved in, $(cat "$MOVE/moved-in")"
  say "done"
  exit 0
fi
as_bbs docker info --format '{{range .SecurityOptions}}{{.}} {{end}}' 2>/dev/null | grep -q rootless || { say "REFUSING: $BBS has no rootless Docker running: run 15-rootless.sh"; exit 1; }
for image in anie-bbs:latest anie-bbs-doors:latest postgres:18-alpine; do
  as_bbs docker image inspect "$image" >/dev/null 2>&1 || { say "REFUSING: no $image in $BBS's Docker: run 04-build.sh"; exit 1; }
done
[ -f "$BHOME/secrets/bbs.env" ] && [ -f "$BHOME/secrets/smtp.env" ] || { say "REFUSING: no secrets of $BBS's: run 03-secrets.sh"; exit 1; }
if as_bbs docker volume inspect anie-bbs_db >/dev/null 2>&1; then say "REFUSING: $BBS's Docker has a database already"; exit 1; fi
docker inspect anie-bbs-db-1 >/dev/null 2>&1 || { say "REFUSING: root's Docker has no BBS database to move"; exit 1; }
install -d -m 700 -o root -g root "$MOVE"

# Root's: the board and doors stopped for good, the database counted, dumped and stopped.
docker update --restart=no anie-bbs-bbs-1 anie-bbs-doors-1 anie-bbs-db-1 >/dev/null
docker stop anie-bbs-bbs-1 anie-bbs-doors-1 >/dev/null
before="$(docker exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -c "$ROWS")"
docker exec anie-bbs-db-1 pg_dump -U anie_bbs -d anie_bbs -Fc > "$MOVE/anie_bbs.dump"
docker stop anie-bbs-db-1 >/dev/null
say "root's BBS stopped; rows (members posts bugs audit mines-best schema) $before; dump $(stat -c %s "$MOVE/anie_bbs.dump") bytes"
docker run --rm --network none --read-only --cap-drop ALL --cap-add DAC_OVERRIDE --security-opt no-new-privileges:true --user 0 \
  --entrypoint tar -v anie-bbs_home:/v:ro anie-bbs:latest -C /v --numeric-owner -cf - . > "$MOVE/home.tar"
docker run --rm --network none --read-only --cap-drop ALL --cap-add DAC_OVERRIDE --security-opt no-new-privileges:true --user 0 \
  --entrypoint tar -v anie-bbs_doors-kept:/v:ro anie-bbs-doors:latest -C /v --numeric-owner -cf - . > "$MOVE/doors-kept.tar"
say "copied: the board's volume $(tar -tf "$MOVE/home.tar" | grep -c .) entries (host key $(tar -tf "$MOVE/home.tar" | grep -c '^\./host_key$')), kept games $(tar -tf "$MOVE/doors-kept.tar" | grep -c 'dsave\.dat$')"

# The BBS's own: made, not started; its volumes given the copies, owners and modes as they were.
compose create --no-build 2>&1 | sed 's/^/move-in: /'
as_bbs docker run --rm -i --network none --read-only --cap-drop ALL --cap-add CHOWN --cap-add FOWNER --cap-add DAC_OVERRIDE --user 0 \
  --entrypoint tar -v anie-bbs_home:/v anie-bbs:latest -C /v --numeric-owner -xpf - < "$MOVE/home.tar"
as_bbs docker run --rm -i --network none --read-only --cap-drop ALL --cap-add CHOWN --cap-add FOWNER --cap-add DAC_OVERRIDE --user 0 \
  --entrypoint tar -v anie-bbs_doors-kept:/v anie-bbs-doors:latest -C /v --numeric-owner -xpf - < "$MOVE/doors-kept.tar"
say "in $BBS's volumes: host key $(as_bbs docker run --rm --network none -v anie-bbs_home:/v:ro --entrypoint sh anie-bbs:latest -c 'test -s /v/host_key && echo there || echo MISSING'), kept games $(as_bbs docker run --rm --network none -v anie-bbs_doors-kept:/v:ro --entrypoint sh anie-bbs-doors:latest -c 'find /v -name dsave.dat | wc -l')"

# Its database, alone first, the dump restored into it and counted.
compose up -d --no-build db 2>&1 | sed 's/^/move-in: /'
health=absent
for i in $(seq 1 60); do
  health="$(as_bbs docker inspect --format '{{.State.Health.Status}}' anie-bbs-db-1 2>/dev/null || echo absent)"
  [ "$health" = healthy ] && break
  sleep 2
done
[ "$health" = healthy ] || { say "PROBLEM: $BBS's database isn't healthy ($health)"; exit 1; }
{ as_bbs docker exec -i anie-bbs-db-1 pg_restore -U anie_bbs -d anie_bbs --no-owner --no-privileges < "$MOVE/anie_bbs.dump" 2>&1 || true; } | tail -n 5 | sed 's/^/move-in: restore: /'
after="$(as_bbs docker exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -c "$ROWS")"
say "rows in $BBS's database $after"
if [ "$after" != "$before" ]; then
  say "PROBLEM: the rows there aren't the rows there were"
  exit 1
fi
date -u +%Y-%m-%dT%H:%M:%SZ > "$MOVE/moved-in"
say "done"
