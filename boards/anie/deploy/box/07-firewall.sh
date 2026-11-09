#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 7: opens the board's port, 2222/tcp, in the host
# firewall, from anywhere (Bryan's word: the internet as well). The board listens on IPv4 only.
# The router's port forward (arcanii.dynu.net:50768 to 192.168.50.210:2222) is Bryan's own step;
# this opens the box alone.
# Refuses if ufw isn't active (then nothing here would decide who reaches the port). Running it
# twice changes nothing the second time.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

TAG=anie-bbs
say() { printf 'firewall: %s\n' "$*"; }

if ! ufw status | head -n 1 | grep -q 'Status: active'; then
  say "REFUSING: ufw isn't active"
  exit 1
fi
if ufw show added | grep -q "comment '$TAG ssh'"; then
  say "the rule is there already"
else
  ufw allow 2222/tcp comment "$TAG ssh" >/dev/null
  say "rule added"
fi
ufw show added | grep '^ufw ' | sed 's/^/firewall: rule: /'
say "done"
