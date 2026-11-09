#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 4: builds the board's image from the clone, and pulls
# PostgreSQL's. Starts nothing. The build downloads the pinned base images and the crates in
# Cargo.lock; no secret goes into the image. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

REPO=/home/bryan/flow-bbs
compose() { docker compose --env-file /root/anie-bbs/bbs.env -f "$REPO/boards/anie/deploy/compose.yaml" "$@"; }
say() { printf 'build: %s\n' "$*"; }

say "start $(date -u +%Y-%m-%dT%H:%M:%SZ) at $(git -C "$REPO" rev-parse HEAD)"
compose pull --quiet db
compose build --progress plain bbs
say "images $(docker images --format '{{.Repository}}:{{.Tag}}={{.ID}}' | grep -E '^(anie-bbs|postgres):' | sort | tr '\n' ' ')"
say "end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
say "done"
