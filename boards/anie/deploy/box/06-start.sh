#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 6: starts the database, the doors and the board from the
# images step 4 built, in the BBS's own Docker (anie-bbs's, rootless), and waits until the database
# is healthy and the board answers SSH on 2222. Then prints the board's host key fingerprint
# (public: the phonebook pins it). Builds nothing; prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
cd /

BBS=anie-bbs
BHOME=/srv/anie-bbs
REPO=$BHOME/flow-bbs
BUID="$(id -u "$BBS")"
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
compose() { as_bbs docker compose --env-file "$BHOME/secrets/bbs.env" -f "$REPO/boards/anie/deploy/compose.yaml" "$@"; }
say() { printf 'start: %s\n' "$*"; }

compose up -d --no-build
db=absent
key=""
for attempt in $(seq 1 60); do
  db="$(as_bbs docker inspect --format '{{.State.Health.Status}}' anie-bbs-db-1 2>/dev/null || echo absent)"
  key="$(ssh-keyscan -T 2 -t ed25519 -p 2222 127.0.0.1 2>/dev/null | grep -v '^#' || true)"
  if [ "$db" = healthy ] && [ -n "$key" ]; then break; fi
  sleep 2
done
say "db $db"
say "bbs $(as_bbs docker inspect --format '{{.State.Status}} restarts={{.RestartCount}}' anie-bbs-bbs-1 2>/dev/null || echo absent)"
say "doors $(as_bbs docker inspect --format '{{.State.Status}} restarts={{.RestartCount}} network={{.HostConfig.NetworkMode}}' anie-bbs-doors-1 2>/dev/null || echo absent)"
compose logs --no-color --tail 3 doors 2>&1 | sed 's/^/start: log: /'
compose logs --no-color --tail 8 bbs 2>&1 | sed 's/^/start: log: /'
if [ "$db" != healthy ] || [ -z "$key" ]; then
  say "NOT UP after two minutes"
  exit 1
fi
say "host key $(printf '%s\n' "$key" | ssh-keygen -lf - | head -n 1)"
say "known_hosts line: [arcanii.dynu.net]:50768 $(printf '%s\n' "$key" | head -n 1 | cut -d' ' -f2-)"
say "done"
