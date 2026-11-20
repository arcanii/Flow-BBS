#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 12, once, when the doors first run in a container of their
# own (after 04-build.sh, before 06-start.sh): members' saved Dungeon games are copied from the
# board's volume (anie-bbs_home, saves/dungeon/<member>) to the doors' own (anie-bbs_doors-kept,
# dungeon/<member>), made the doors' user's (10002) and theirs alone. The board is stopped first, so
# no game is saved meanwhile, and the doors' container started, which makes its volumes; 06-start.sh
# starts the board again. Copies only: the old saves stay where they were, for clearing by hand once
# the new ones are seen to work. Run again, it copies nothing. Prints counts, never a member.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

REPO=/home/bryan/flow-bbs
compose() { docker compose --env-file /root/anie-bbs/bbs.env -f "$REPO/boards/anie/deploy/compose.yaml" "$@"; }
say() { printf 'move: %s\n' "$*"; }

docker image inspect anie-bbs-doors:latest >/dev/null 2>&1 || { say "REFUSING: no anie-bbs-doors image: run 04-build.sh first"; exit 1; }
compose stop bbs 2>&1 | sed 's/^/move: /'
compose up -d --no-build doors 2>&1 | sed 's/^/move: /'
docker volume inspect anie-bbs_doors-kept >/dev/null || { say "PROBLEM: the doors' volume wasn't made"; exit 1; }

# The copy, by root in a container of the doors' image with only what a copy needs: reading the
# board's folders, giving the copies to the doors' user. No network; the board's volume read-only.
docker run --rm --network none --read-only --user 0 --entrypoint /bin/sh \
  --cap-drop ALL --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --security-opt no-new-privileges:true \
  -v anie-bbs_home:/from:ro -v anie-bbs_doors-kept:/to anie-bbs-doors:latest -c '
    set -eu
    if [ -e /to/dungeon ]; then
      echo "move: already moved: $(ls /to/dungeon | wc -l) members kept"
      exit 0
    fi
    if [ ! -d /from/saves/dungeon ]; then
      echo "move: no saved games to move"
      exit 0
    fi
    rm -rf /to/dungeon.new
    cp -a /from/saves/dungeon /to/dungeon.new
    chown -R 10002:10002 /to/dungeon.new
    chmod -R u+rwX,go-rwx /to/dungeon.new
    mv /to/dungeon.new /to/dungeon
    echo "move: moved $(ls /to/dungeon | wc -l) members, $(find /to/dungeon -name dsave.dat | wc -l) saved games"
  '
say "kept now $(docker exec anie-bbs-doors-1 sh -c 'ls /var/lib/anie-doors/dungeon 2>/dev/null | wc -l') members, saved games the doors can't read $(docker exec anie-bbs-doors-1 sh -c 'find /var/lib/anie-doors/dungeon -name dsave.dat ! -readable 2>/dev/null | wc -l')"
say "done"
