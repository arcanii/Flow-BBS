#!/bin/bash
# Anie Solutions BBS on tkaniedev03, before each build: arcanii/Flow-Games (private), whose
# flow-mines the game room runs, cloned by its deploy key (10-games-key.sh) into
# /root/anie-bbs/flow-games if it isn't there, then checked out at exactly the commit this script
# names. The board's image is built with it as a second context (compose.yaml's flow-games).
# Refuses if a tracked file in the clone is modified, or the commit isn't on the branch. GitHub's
# host key is checked against the one 10-games-key.sh wrote. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

DIR=/root/anie-bbs
REPO=$DIR/flow-games
ORIGIN=git@github.com:arcanii/Flow-Games.git
BRANCH=master
COMMIT=__GAMES_COMMIT__

say() { printf 'games: %s\n' "$*"; }
g() { git -c safe.directory="$REPO" -C "$REPO" "$@"; }

if [ ! -f "$DIR/flow-games-key" ] || [ ! -f "$DIR/github_known_hosts" ]; then
  say "REFUSING: no deploy key yet: run 10-games-key.sh first"
  exit 1
fi
export GIT_TERMINAL_PROMPT=0
export GIT_SSH_COMMAND="ssh -i $DIR/flow-games-key -o IdentitiesOnly=yes -o UserKnownHostsFile=$DIR/github_known_hosts -o StrictHostKeyChecking=yes -o BatchMode=yes"

if [ ! -d "$REPO/.git" ]; then
  say "cloning $ORIGIN into $REPO"
  git clone --quiet --branch "$BRANCH" "$ORIGIN" "$REPO"
fi
chmod 700 "$REPO"
if [ "$(g remote get-url origin)" != "$ORIGIN" ]; then
  say "REFUSING: the clone's origin isn't $ORIGIN"
  exit 1
fi
if [ -n "$(g status --porcelain --untracked-files=no)" ]; then
  say "REFUSING: a tracked file is modified in the clone"
  exit 1
fi
say "before $(g rev-parse HEAD)"
g fetch --quiet --prune origin "$BRANCH"
if ! g merge-base --is-ancestor "$COMMIT" "origin/$BRANCH"; then
  say "REFUSING: $COMMIT isn't on origin/$BRANCH"
  exit 1
fi
g checkout --quiet --detach "$COMMIT"
say "head $(g rev-parse HEAD)"
say "done"
