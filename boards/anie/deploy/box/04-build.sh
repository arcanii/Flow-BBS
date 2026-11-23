#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 4: builds the board's image and its doors' from the clone,
# and pulls PostgreSQL's, in the BBS's own Docker (anie-bbs's, rootless: 15-rootless.sh). Starts
# nothing. The build downloads the pinned base images and the crates in Cargo.lock; no secret goes
# into the image. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
cd /

BBS=anie-bbs
BHOME=/srv/anie-bbs
REPO=$BHOME/flow-bbs
BUID="$(id -u "$BBS")"
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
compose() { as_bbs docker compose --env-file "$BHOME/secrets/bbs.env" -f "$REPO/boards/anie/deploy/compose.yaml" "$@"; }
say() { printf 'build: %s\n' "$*"; }

say "start $(date -u +%Y-%m-%dT%H:%M:%SZ) at $(as_bbs git -C "$REPO" rev-parse HEAD)"
compose pull --quiet db
compose build --progress plain doors bbs
say "images $(as_bbs docker images --format '{{.Repository}}:{{.Tag}}={{.ID}}' | grep -E '^(anie-bbs|anie-bbs-doors|postgres):' | sort | tr '\n' ' ')"
say "end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
say "done"
