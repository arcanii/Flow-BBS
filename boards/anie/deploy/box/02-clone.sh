#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 2: the clone of arcanii/Flow-BBS (public), made as bryan
# if it isn't there, then checked out at exactly the commit this script names. Refuses if a tracked
# file in the clone is modified, or the commit isn't on the branch. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

REPO=/home/bryan/flow-bbs
OWNER=bryan
ORIGIN=https://github.com/arcanii/Flow-BBS.git
BRANCH=anie-bbs
COMMIT=__COMMIT__

say() { printf 'clone: %s\n' "$*"; }
as_owner() { runuser -u "$OWNER" -- env HOME="/home/$OWNER" GIT_TERMINAL_PROMPT=0 "$@"; }
g() { as_owner git -C "$REPO" "$@"; }

if [ ! -d "$REPO/.git" ]; then
  say "cloning $ORIGIN into $REPO"
  as_owner git clone --quiet --branch "$BRANCH" "$ORIGIN" "$REPO"
fi
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
