#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 9: makes the handle this script names the board's sysop,
# and lets it in. The member must have applied on the board (address proved, authenticator set
# up); the board's own `anie-bbs sysop` does it, and writes it to the board's log. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
cd /

HANDLE=__HANDLE__
BBS=anie-bbs
BUID="$(id -u "$BBS")"
d() { runuser -u "$BBS" -- env HOME=/srv/anie-bbs XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin docker "$@"; }
say() { printf 'sysop: %s\n' "$*"; }

d exec anie-bbs-bbs-1 anie-bbs sysop "$HANDLE" 2>&1 | sed 's/^/sysop: /'
say "sysops $(d exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -c "SELECT string_agg(handle, ' ') FROM members WHERE role = 'sysop' AND status = 'active'")"
say "done"
