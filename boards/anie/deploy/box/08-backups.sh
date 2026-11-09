#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 8: a nightly backup of the board's database. Installs
# /usr/local/sbin/anie-bbs-backup (pg_dump, custom format, into /root/anie-bbs/backups, mode 600,
# fourteen days kept) and a systemd timer that runs it at 03:30 Tokyo time, then runs it once now.
# The dumps hold the authenticator seeds sealed: without bbs.env's key they open nothing, and the
# key is not put beside them. Running it twice changes nothing the second time. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

say() { printf 'backups: %s\n' "$*"; }

install -d -m 700 -o root -g root /root/anie-bbs/backups

cat > /usr/local/sbin/anie-bbs-backup <<'SCRIPT'
#!/bin/bash
# The Anie Solutions BBS's nightly backup (installed by 08-backups.sh).
set -euo pipefail
umask 077
DIR=/root/anie-bbs/backups
at="$(date -u +%Y%m%dT%H%M%SZ)"
docker exec anie-bbs-db-1 pg_dump -U anie_bbs -d anie_bbs -Fc > "$DIR/anie_bbs-$at.dump.part"
mv "$DIR/anie_bbs-$at.dump.part" "$DIR/anie_bbs-$at.dump"
find "$DIR" -name 'anie_bbs-*.dump' -mtime +14 -delete
find "$DIR" -name '*.part' -mmin +60 -delete
echo "anie-bbs-backup: anie_bbs-$at.dump $(stat -c %s "$DIR/anie_bbs-$at.dump") bytes"
SCRIPT
chmod 755 /usr/local/sbin/anie-bbs-backup

cat > /etc/systemd/system/anie-bbs-backup.service <<'UNIT'
[Unit]
Description=The Anie Solutions BBS's nightly database backup
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/anie-bbs-backup
UNIT

cat > /etc/systemd/system/anie-bbs-backup.timer <<'UNIT'
[Unit]
Description=The Anie Solutions BBS's nightly database backup, at 03:30 Tokyo time

[Timer]
OnCalendar=*-*-* 03:30:00 Asia/Tokyo
Persistent=true
RandomizedDelaySec=10min

[Install]
WantedBy=timers.target
UNIT

systemctl daemon-reload
systemctl enable --now anie-bbs-backup.timer >/dev/null 2>&1
say "timer $(systemctl is-active anie-bbs-backup.timer), next $(systemctl show -P NextElapseUSecRealtime anie-bbs-backup.timer)"
systemctl start anie-bbs-backup.service
say "$(journalctl -u anie-bbs-backup.service -n 20 --no-pager -o cat 2>/dev/null | grep '^anie-bbs-backup:' | tail -n 1)"
say "backups $(ls -1 /root/anie-bbs/backups/*.dump 2>/dev/null | wc -l)"
say "done"
