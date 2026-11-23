#!/bin/bash
# Anie Solutions BBS on tkaniedev03, after step 15: whether a port the BBS's own Docker publishes
# hands its container the caller's own address, as the board needs for its limits. A container of
# that Docker listens on 2299 for a moment; this machine calls it at its own LAN address, and the
# container says whose address it saw. Leaves nothing running. Prints no secret.
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

BBS=anie-bbs
BHOME=/srv/anie-bbs
BUID="$(id -u "$BBS")"
say() { printf 'address: %s\n' "$*"; }
as_bbs() { runuser -u "$BBS" -- env HOME="$BHOME" XDG_RUNTIME_DIR="/run/user/$BUID" DOCKER_HOST="unix:///run/user/$BUID/docker.sock" PATH=/usr/bin:/bin "$@"; }

lan="$(ip -4 route get 1.1.1.1 | awk '{ for (i = 1; i < NF; i++) if ($i == "src") print $(i + 1) }')"
as_bbs docker pull -q debian:trixie-slim >/dev/null
as_bbs docker rm -f anie-address-check >/dev/null 2>&1 || true
as_bbs docker run -d --rm --name anie-address-check -p 0.0.0.0:2299:2299 debian:trixie-slim perl -MIO::Socket::INET -e '
  my $l = IO::Socket::INET->new(LocalPort => 2299, Listen => 1, ReuseAddr => 1) or die "listen: $!\n";
  my $c = $l->accept; print $c "peer ", $c->peerhost, "\n"; close $c;' >/dev/null
seen=""
for i in $(seq 1 10); do
  seen="$(perl -MIO::Socket::INET -e 'my $s = IO::Socket::INET->new(PeerAddr => $ARGV[0], PeerPort => 2299, Timeout => 3) or exit 1; my $l = <$s>; print $l // ""' "$lan" 2>/dev/null || true)"
  [ -n "$seen" ] && break
  sleep 1
done
as_bbs docker rm -f anie-address-check >/dev/null 2>&1 || true
seen="$(printf '%s' "$seen" | tr -d '\r\n')"
say "called from $lan, the container saw: ${seen:-nothing}"
if [ "$seen" = "peer $lan" ]; then
  say "callers' own addresses reach the board"
else
  say "PROBLEM: the container doesn't see the caller's own address"
fi
say "done"
