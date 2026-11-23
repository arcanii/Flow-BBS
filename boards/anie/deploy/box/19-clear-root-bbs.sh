#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 19, once, after 17-move-in.sh and Bryan's word: what was
# left of the BBS in root's Docker, and the move's copies, cleared for good. Root's three stopped
# containers, their network and volumes (the old database, host key and kept games), the images
# built for them (and PostgreSQL's, where none of root's other containers uses it), the old clone in
# /home/bryan/flow-bbs, and the move's dump and copies in /root/anie-bbs/move-in.
#
# Refuses unless the BBS runs in its own Docker (its database healthy, its doors and board up, 2222
# held by anie-bbs and answering with the board's own host key, every game the move copied kept
# there, and a backup of its database made since the move that pg_restore reads), root's containers
# are all stopped, and the old clone has nothing that isn't on GitHub. Leaves root's copies of the
# secrets, the Flow-Games clone and its key, the backups, root's build cache and base images (which
# its other projects share), and everything else of root's. After it, 18-move-back.sh has nothing
# to go back to. Run again, it finds nothing to clear. Prints names and counts, never a secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 077
cd /
unset DOCKER_HOST DOCKER_CONTEXT

BBS=anie-bbs
BHOME=/srv/anie-bbs
BUID="$(id -u "$BBS")"
MOVE=/root/anie-bbs/move-in
OLD=/home/bryan/flow-bbs
ORIGIN=https://github.com/arcanii/Flow-BBS.git
BACKUPS=/root/anie-bbs/backups
KEY=SHA256:87Vz5v/WU2LpFblsOuncrVeEtDdHTpsXCOU4HsQw+8k
PROJECT=label=com.docker.compose.project=anie-bbs
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }
rd() { docker -H unix:///var/run/docker.sock "$@"; }
og() { GIT_OPTIONAL_LOCKS=0 git -c safe.directory="$OLD" -C "$OLD" "$@"; }
say() { printf 'clear: %s\n' "$*"; }
refuse() { say "REFUSING: $*"; exit 1; }
words() { local w; w="$(sed '/^$/d' | paste -sd ' ' -)"; printf '%s\n' "${w:-none}"; }
fingerprint() { ssh-keyscan -T 5 -t ed25519 -p 2222 127.0.0.1 2>/dev/null | grep -v '^#' | ssh-keygen -lf - 2>/dev/null | awk '{print $2}' | head -n 1 || true; }

# What's left of the BBS in root's Docker: by the compose project's label, and by name.
case "$(rd info --format '{{range .SecurityOptions}}{{.}} {{end}}')" in *rootless*) refuse "the Docker at /var/run/docker.sock is a rootless one, not root's";; esac
containers="$(rd ps -a --filter "$PROJECT" --format '{{.Names}}' | sort -u)"
volumes="$( { rd volume ls -q --filter "$PROJECT"; for v in anie-bbs_db anie-bbs_home anie-bbs_doors-kept anie-bbs_doors-socket; do if rd volume inspect "$v" >/dev/null 2>&1; then echo "$v"; fi; done; } | sort -u)"
networks="$(rd network ls --filter "$PROJECT" --format '{{.Name}}' | sort -u)"
tags="$(for i in anie-bbs:latest anie-bbs-doors:latest; do if rd image inspect "$i" >/dev/null 2>&1; then echo "$i"; fi; done)"
ids="$(rd images -a --filter "$PROJECT" --format '{{.ID}}' | sort -u)"
if [ -z "$containers$volumes$networks$tags$ids" ] && [ ! -e "$MOVE" ] && [ ! -e "$OLD" ]; then
  say "nothing left to clear: root's Docker has none of the BBS's, and the move's copies and the old clone are gone"
  say "done"
  exit 0
fi

# Root's: all stopped, and moved out of.
for c in $containers; do
  st="$(rd inspect --format '{{.State.Status}}' "$c")"
  case "$st" in exited|created|dead) ;; *) refuse "root's $c is $st";; esac
done
if [ -n "$containers$volumes" ] && [ ! -s "$MOVE/moved-in" ]; then
  refuse "root's Docker still has the BBS's, and there's no record of the move (17-move-in.sh's $MOVE/moved-in)"
fi
for p in "$MOVE" "$OLD"; do
  if [ -L "$p" ]; then refuse "$p is a link"; fi
done
if [ -s "$MOVE/moved-in" ]; then say "moved in at $(cat "$MOVE/moved-in")"; fi

# The BBS's own: up, on 2222 as its user, with its host key, its kept games, and a backup since.
case "$(as_bbs docker info --format '{{range .SecurityOptions}}{{.}} {{end}}' 2>/dev/null || true)" in *rootless*) ;; *) refuse "$BBS has no rootless Docker running";; esac
db="$(as_bbs docker inspect --format '{{.State.Health.Status}}' anie-bbs-db-1 2>/dev/null || echo absent)"
doors="$(as_bbs docker inspect --format '{{.State.Status}}' anie-bbs-doors-1 2>/dev/null || echo absent)"
board="$(as_bbs docker inspect --format '{{.State.Status}}' anie-bbs-bbs-1 2>/dev/null || echo absent)"
say "$BBS's own: db $db, doors $doors, board $board"
if [ "$db" != healthy ] || [ "$doors" != running ] || [ "$board" != running ]; then refuse "the BBS isn't all up in its own Docker"; fi
pid="$(ss -ltnpH 'sport = :2222' | grep -o 'pid=[0-9]*' | head -n 1 | cut -d= -f2 || true)"
puid=none
if [ -n "$pid" ]; then puid="$(ps -o uid= -p "$pid" | tr -d ' ' || true)"; fi
if [ "$puid" != "$BUID" ]; then refuse "2222 isn't held by $BBS (uid ${puid:-none})"; fi
fp="$(fingerprint)"
if [ "$fp" != "$KEY" ]; then refuse "2222 answers with host key ${fp:-none}, not the board's $KEY"; fi
say "2222 held by $BBS, host key $fp"
kept="$(as_bbs docker exec anie-bbs-doors-1 sh -c 'find /var/lib/anie-doors -name dsave.dat 2>/dev/null | wc -l' 2>/dev/null || echo 0)"
if [ -f "$MOVE/doors-kept.tar" ]; then
  moved="$(tar -tf "$MOVE/doors-kept.tar" | grep -c 'dsave\.dat$' || true)"
  case "$moved" in ''|*[!0-9]*) refuse "the move's copy of the kept games can't be read";; esac
  if [ "$kept" -lt "$moved" ]; then refuse "$BBS's doors keep $kept saved games, the move copied $moved"; fi
  say "saved games: $kept kept by $BBS's doors, $moved moved"
fi
if [ -s "$MOVE/moved-in" ]; then since=(-newer "$MOVE/moved-in"); else since=(-mtime -2); fi
newest="$(find "$BACKUPS" -maxdepth 1 -name 'anie_bbs-*.dump' "${since[@]}" -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)"
if [ -z "$newest" ]; then refuse "no backup in $BACKUPS since the move"; fi
entries="$(as_bbs docker exec -i anie-bbs-db-1 pg_restore --list < "$newest" 2>/dev/null | grep -c '^[0-9]' || true)"
if [ "${entries:-0}" -lt 10 ]; then refuse "the backup since the move, $(basename "$newest"), doesn't read ($entries entries)"; fi
say "backup since the move: $(basename "$newest"), $(stat -c %s "$newest") bytes, $entries entries"

# The old clone: a clone of Flow-BBS with nothing of its own.
if [ -e "$OLD" ]; then
  if [ ! -d "$OLD/.git" ]; then refuse "$OLD isn't a clone"; fi
  if [ "$(og remote get-url origin 2>/dev/null)" != "$ORIGIN" ]; then refuse "$OLD's origin isn't $ORIGIN"; fi
  dirty="$(og status --porcelain | wc -l)"
  ahead="$(og log --branches --not --remotes --oneline | wc -l)"
  stashed="$(og stash list | wc -l)"
  if [ "$dirty $ahead $stashed" != "0 0 0" ]; then refuse "$OLD has files changed or untracked $dirty, commits not on GitHub $ahead, stashes $stashed"; fi
  if ! og merge-base --is-ancestor HEAD origin/main; then refuse "$OLD's checkout isn't on origin/main"; fi
  say "the old clone: $OLD at $(og rev-parse --short HEAD), nothing of its own, $(du -sh "$OLD" | cut -f1)"
fi

say "to clear in root's Docker: containers $(printf '%s\n' "$containers" | words); networks $(printf '%s\n' "$networks" | words); volumes $(printf '%s\n' "$volumes" | words); images $(printf '%s\n' "$tags" | words), $(printf '%s\n' "$ids" | sed '/^$/d' | wc -l) by the project's label"
if [ -e "$MOVE" ]; then say "the move's copies: $(ls -A "$MOVE" | words), $(du -sh "$MOVE" | cut -f1)"; fi
say "root's docker before: $(rd system df --format '{{.Type}} {{.Size}}' | paste -sd ',' -)"

# Cleared: root's containers, network, volumes, images; then the old clone; the move's copies last.
for c in $containers; do rd rm "$c" >/dev/null || { say "PROBLEM: root's container $c wasn't removed"; exit 1; }; done
for n in $networks; do rd network rm "$n" >/dev/null || { say "PROBLEM: root's network $n wasn't removed"; exit 1; }; done
for v in $volumes; do rd volume rm "$v" >/dev/null || { say "PROBLEM: root's volume $v wasn't removed"; exit 1; }; done
for i in $tags $(rd images -a --filter "$PROJECT" --format '{{.ID}}' | sort -u); do
  if ! rd image inspect "$i" >/dev/null 2>&1; then continue; fi
  if out="$(rd image rm "$i" 2>&1)"; then say "removed image $i"; else say "kept image $i: $(printf '%s\n' "$out" | tail -n 1)"; fi
done
if rd image inspect postgres:18-alpine >/dev/null 2>&1; then
  out=""
  users="$(rd ps -a -q --filter ancestor=postgres:18-alpine | wc -l)"
  if [ "$users" = 0 ] && out="$(rd image rm postgres:18-alpine 2>&1)"; then
    say "removed image postgres:18-alpine"
  else
    say "kept image postgres:18-alpine: $users of root's other containers use it $(printf '%s\n' "${out:-}" | tail -n 1)"
  fi
fi
if [ -e "$OLD" ]; then rm -rf --one-file-system -- "$OLD"; say "removed the old clone $OLD"; fi
if [ -e "$MOVE" ]; then rm -rf --one-file-system -- "$MOVE"; say "removed the move's copies $MOVE"; fi

# After: nothing of the BBS's in root's Docker, and the BBS's own as it was.
say "root's docker after: the BBS's containers $(rd ps -a -q --filter "$PROJECT" | wc -l), volumes $(rd volume ls -q --filter "$PROJECT" | wc -l), networks $(rd network ls -q --filter "$PROJECT" | wc -l); $(rd system df --format '{{.Type}} {{.Size}}' | paste -sd ',' -)"
fp="$(fingerprint)"
say "$BBS's own: db $(as_bbs docker inspect --format '{{.State.Health.Status}}' anie-bbs-db-1 2>/dev/null || echo absent), board $(as_bbs docker inspect --format '{{.State.Status}}' anie-bbs-bbs-1 2>/dev/null || echo absent), host key ${fp:-none}"
if [ "$fp" != "$KEY" ]; then say "PROBLEM: 2222 doesn't answer with the board's host key after"; exit 1; fi
say "done"
