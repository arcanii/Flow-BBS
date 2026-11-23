#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 3: the board's secrets, the BBS's own user's (anie-bbs,
# 15-rootless.sh), who runs its Docker and so reads them; never printed.
#
# /srv/anie-bbs/secrets/bbs.env: the database's password and the board's secret key. The key seals
# every member's authenticator seed: it is NEVER made again over one that exists (a new key would
# lock every member out of their codes). Where root's copy from before the BBS had a user of its own
# is there (/root/anie-bbs/bbs.env), it's copied; else it's made once with openssl.
# /srv/anie-bbs/secrets/smtp.env: the mail relay's settings, root's copied the same way, or Gmail's
# filled in and its login left for Bryan to write by hand.
# Both anie-bbs's, mode 600, in a folder of mode 700. Root's copies stay where they are. The nightly
# backups stay root's (/root/anie-bbs/backups), out of the BBS's user's reach. Running it twice
# changes nothing.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 077
cd /

BBS=anie-bbs
DIR=/srv/anie-bbs/secrets
OLD=/root/anie-bbs
say() { printf 'secrets: %s\n' "$*"; }

id "$BBS" >/dev/null 2>&1 || { say "REFUSING: no user $BBS: run 15-rootless.sh first"; exit 1; }
install -d -m 700 -o root -g root "$OLD"
install -d -m 700 -o root -g root "$OLD/backups"
install -d -m 700 -o "$BBS" -g "$BBS" "$DIR"

if [ -e "$DIR/bbs.env" ]; then
  say "bbs.env already there: left as it is"
elif [ -f "$OLD/bbs.env" ]; then
  install -m 600 -o "$BBS" -g "$BBS" "$OLD/bbs.env" "$DIR/bbs.env"
  say "bbs.env copied from root's"
else
  {
    echo "# The Anie Solutions BBS, made on this box by 03-secrets.sh. Keep a copy offline:"
    echo "# without ANIE_BBS_SECRET_KEY no member's authenticator works."
    echo "ANIE_BBS_DB_PASSWORD=$(openssl rand -hex 24)"
    echo "ANIE_BBS_SECRET_KEY=$(openssl rand -hex 32)"
    echo "# Where news of an application goes; empty is the sysops' own addresses."
    echo "ANIE_BBS_SYSOP_MAIL="
  } > "$DIR/bbs.env"
  say "bbs.env made"
fi

if [ -e "$DIR/smtp.env" ]; then
  say "smtp.env already there: left as it is"
elif [ -f "$OLD/smtp.env" ]; then
  install -m 600 -o "$BBS" -g "$BBS" "$OLD/smtp.env" "$DIR/smtp.env"
  say "smtp.env copied from root's"
else
  {
    echo "# The Anie Solutions BBS's mail: Gmail, with an app password (Google Account, Security,"
    echo "# 2-Step Verification, App passwords). Fill in the three empty lines; SMTP_FROM is the same"
    echo "# Gmail address as SMTP_USERNAME. Unix line endings, no quotes."
    echo "SMTP_HOST=smtp.gmail.com"
    echo "SMTP_PORT=587"
    echo "SMTP_USERNAME="
    echo "SMTP_PASSWORD="
    echo "SMTP_FROM="
    echo "SMTP_FROM_NAME=Anie Solutions BBS"
  } > "$DIR/smtp.env"
  say "smtp.env made: its login is Bryan's to fill in"
fi

chmod 600 "$DIR/bbs.env" "$DIR/smtp.env"
chown "$BBS:$BBS" "$DIR/bbs.env" "$DIR/smtp.env"
for f in bbs.env smtp.env; do say "$f mode $(stat -c '%a %U' "$DIR/$f")"; done
for k in ANIE_BBS_DB_PASSWORD ANIE_BBS_SECRET_KEY; do
  if grep -Eq "^$k=[0-9a-f]+$" "$DIR/bbs.env"; then say "bbs.env sets $k"; else say "bbs.env LACKS $k"; fi
done
if [ -f "$OLD/bbs.env" ]; then
  if cmp -s "$OLD/bbs.env" "$DIR/bbs.env"; then say "bbs.env the same as root's"; else say "bbs.env NOT the same as root's"; fi
fi
say "done"
