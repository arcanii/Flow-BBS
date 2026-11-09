#!/bin/bash
# Anie Solutions BBS on tkaniedev03, once: the deploy key that reads arcanii/Flow-Games (private),
# whose flow-mines the game room runs. Made here if it isn't there, and never replaced, so its
# private half never leaves /root/anie-bbs. Prints its public half, to add to Flow-Games as a
# read-only deploy key, and writes GitHub's host key as the box will check it. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

DIR=/root/anie-bbs
KEY=$DIR/flow-games-key
HOSTS=$DIR/github_known_hosts
say() { printf 'games-key: %s\n' "$*"; }

install -d -m 700 "$DIR"
if [ -f "$KEY" ]; then
  say "kept $KEY"
else
  ssh-keygen -q -t ed25519 -N '' -C 'tkaniedev03 anie-bbs: reads arcanii/Flow-Games' -f "$KEY"
  say "made $KEY"
fi
chmod 600 "$KEY"
# GitHub's ed25519 host key, as https://api.github.com/meta publishes it
# (SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU).
printf '%s\n' 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' > "$HOSTS"
chmod 644 "$HOSTS"
say "public $(cat "$KEY.pub")"
say "fingerprint $(ssh-keygen -lf "$KEY.pub")"
say "github $(ssh-keygen -lf "$HOSTS" | awk '{print $2}')"
say "done"
