#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 3: the board's secrets, made here and never printed.
#
# /root/anie-bbs/bbs.env: the database's password and the board's secret key, made once with
# openssl. The key seals every member's authenticator seed: it is NEVER made again over one that
# exists (a new key would lock every member out of their codes), so this script only ever adds.
# /root/anie-bbs/smtp.env: the mail relay's settings, Gmail's filled in, its login left for Bryan to
# write by hand. Both root's, mode 600, in a directory of mode 700. Running it twice changes nothing.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 077

DIR=/root/anie-bbs
say() { printf 'secrets: %s\n' "$*"; }

install -d -m 700 -o root -g root "$DIR"
install -d -m 700 -o root -g root "$DIR/backups"

if [ -e "$DIR/bbs.env" ]; then
  say "bbs.env already there: left as it is"
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
chown root:root "$DIR/bbs.env" "$DIR/smtp.env"
for f in bbs.env smtp.env; do say "$f mode $(stat -c '%a %U' "$DIR/$f")"; done
for k in ANIE_BBS_DB_PASSWORD ANIE_BBS_SECRET_KEY; do
  if grep -Eq "^$k=[0-9a-f]+$" "$DIR/bbs.env"; then say "bbs.env sets $k"; else say "bbs.env LACKS $k"; fi
done
say "done"
