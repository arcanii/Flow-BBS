#!/bin/bash
# Anie Solutions BBS on tkaniedev03, only if 17-move-in.sh or what follows it fails: the BBS back as
# it was before, in root's Docker. The BBS's own Docker's containers are stopped (port 2222 let go)
# and left, with their volumes, for a look; root's database, doors and board are given their restart
# policy back and started, from the volumes 17 left as they were. Whatever was done on the board
# after the move is in the BBS's own volumes, not root's. Deletes nothing. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
cd /

BBS=anie-bbs
BHOME=/srv/anie-bbs
REPO=$BHOME/flow-bbs
BUID="$(id -u "$BBS")"
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
say() { printf 'move-back: %s\n' "$*"; }

for c in anie-bbs-bbs-1 anie-bbs-doors-1 anie-bbs-db-1; do as_bbs docker stop "$c" >/dev/null 2>&1 || true; done
say "$BBS's containers: $(as_bbs docker ps -a --filter name=anie-bbs- --format '{{.Names}}={{.State}}' 2>&1 | paste -sd ' ' -)"
docker update --restart=unless-stopped anie-bbs-db-1 anie-bbs-doors-1 anie-bbs-bbs-1 >/dev/null
docker start anie-bbs-db-1 >/dev/null
health=absent
for i in $(seq 1 60); do
  health="$(docker inspect --format '{{.State.Health.Status}}' anie-bbs-db-1 2>/dev/null || echo absent)"
  [ "$health" = healthy ] && break
  sleep 2
done
docker start anie-bbs-doors-1 anie-bbs-bbs-1 >/dev/null
key=""
for i in $(seq 1 30); do
  key="$(ssh-keyscan -T 2 -t ed25519 -p 2222 127.0.0.1 2>/dev/null | grep -v '^#' || true)"
  [ -n "$key" ] && break
  sleep 2
done
say "root's db $health, board $(docker inspect --format '{{.State.Status}}' anie-bbs-bbs-1), doors $(docker inspect --format '{{.State.Status}}' anie-bbs-doors-1)"
if [ -z "$key" ]; then say "NOT UP: nothing answers SSH on 2222"; exit 1; fi
say "host key $(printf '%s\n' "$key" | ssh-keygen -lf - | head -n 1)"
rm -f /root/anie-bbs/move-in/moved-in
say "done"
