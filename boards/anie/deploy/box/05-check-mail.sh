#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 5: READ-ONLY. Says whether smtp.env is in shape, then
# whether Gmail lets the board log in with it (the board's own check-mail, which sends nothing).
# NO VALUE IS EVER PRINTED: key names, lengths and the one-word class of a failure only.
set -u
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

REPO=/home/bryan/flow-bbs
FILE=/root/anie-bbs/smtp.env
compose() { docker compose --env-file /root/anie-bbs/bbs.env -f "$REPO/boards/anie/deploy/compose.yaml" "$@"; }
bad=0
say() { printf 'check-mail: %s\n' "$*"; }
problem() { say "PROBLEM: $*"; bad=1; }
value() { grep -m 1 "^$1=" "$FILE" | cut -d= -f2-; }

if [ -L "$FILE" ] || [ ! -f "$FILE" ]; then problem "no regular file $FILE"; say "NOT OK"; exit 0; fi
say "file mode $(stat -c '%a %U' "$FILE")"
[ "$(stat -c '%a %U' "$FILE")" = "600 root" ] || problem "the file is not mode 600 owned by root"
if grep -q "$(printf '\r')" "$FILE"; then problem "the file holds a carriage return: save it with Unix line endings"; fi
for k in SMTP_HOST SMTP_PORT SMTP_USERNAME SMTP_PASSWORD SMTP_FROM; do
  n="$(grep -c "^$k=" "$FILE")"
  v="$(value "$k")"
  if [ "$n" != 1 ]; then
    problem "$k starts $n lines, expected 1"
  elif [ -z "$v" ]; then
    problem "$k is empty"
  else
    say "has $k (length ${#v})"
  fi
  if printf '%s' "$v" | grep -q "[\"']"; then problem "$k holds a quote: write the bare value"; fi
done
password="$(value SMTP_PASSWORD | tr -d ' ')"
[ "${#password}" = 16 ] || problem "SMTP_PASSWORD is ${#password} letters without its spaces; a Gmail app password is 16"
[ "$(value SMTP_USERNAME)" = "$(value SMTP_FROM)" ] || say "note: SMTP_FROM isn't SMTP_USERNAME; Gmail sends only as the account or an alias it has verified"
if [ "$bad" -ne 0 ]; then say "NOT OK"; exit 0; fi

if compose run --rm --no-deps --quiet-pull bbs check-mail 2>&1 | sed 's/^/check-mail: /'; then
  say "ok"
else
  say "NOT OK"
fi
