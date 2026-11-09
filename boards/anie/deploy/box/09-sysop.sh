#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 9: makes the handle this script names the board's sysop,
# and lets it in. The member must have applied on the board (address proved, authenticator set
# up); the board's own `anie-bbs sysop` does it, and writes it to the board's log. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

HANDLE=__HANDLE__
say() { printf 'sysop: %s\n' "$*"; }

docker exec anie-bbs-bbs-1 anie-bbs sysop "$HANDLE" 2>&1 | sed 's/^/sysop: /'
say "sysops $(docker exec anie-bbs-db-1 psql -U anie_bbs -d anie_bbs -At -c "SELECT string_agg(handle, ' ') FROM members WHERE role = 'sysop' AND status = 'active'")"
say "done"
