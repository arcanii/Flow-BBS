#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 13, once, after 12-move-saves.sh and Bryan's word that the
# moved games work: the old copies of members' saved Dungeon games are removed from the board's
# volume (anie-bbs_home, saves/). Refuses unless every saved game there has its copy in the doors'
# volume (anie-bbs_doors-kept). Removes nothing else: the host key stays. Done by the board's own
# user, whose files they are, in a container of the board's image with no network. Prints counts,
# never a member.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

say() { printf 'clear: %s\n' "$*"; }
home() { docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  --entrypoint /bin/sh "$@"; }
count() { printf '%s\n' "$1" | sed '/^$/d' | wc -l; }

if ! home -v anie-bbs_home:/h:ro anie-bbs:latest -c 'test -d /h/saves'; then
  say "nothing to clear: the board's volume has no saves"
  say "done"
  exit 0
fi
old="$(home -v anie-bbs_home:/h:ro anie-bbs:latest -c 'cd /h/saves/dungeon 2>/dev/null && find . -name dsave.dat | sort || true')"
new="$(docker exec anie-bbs-doors-1 sh -c 'cd /var/lib/anie-doors/dungeon && find . -name dsave.dat | sort')"
missing="$(comm -23 <(printf '%s\n' "$old" | sed '/^$/d') <(printf '%s\n' "$new" | sed '/^$/d') | wc -l)"
say "old saved games $(count "$old"), in the doors' volume $(count "$new"), old ones without a copy there $missing"
if [ "$missing" != 0 ]; then
  say "REFUSING: a saved game in the board's volume has no copy in the doors'"
  exit 1
fi
home -v anie-bbs_home:/h anie-bbs:latest -c 'rm -rf /h/saves'
say "the board's volume holds $(home -v anie-bbs_home:/h:ro anie-bbs:latest -c 'ls -A /h' | paste -sd ' ' -)"
say "done"
