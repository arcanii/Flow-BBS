#!/bin/bash
# Anie Solutions BBS on tkaniedev03, step 1: READ-ONLY. Says what is on the box before anything
# changes. It writes nothing, starts nothing and stops nothing. It prints no secret: of the env
# files it prints only which keys are set, never a value.
set -u
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

REPO=/home/bryan/flow-bbs
OWNER=bryan
SECRETS=/root/anie-bbs

say() { printf 'inspect: %s\n' "$*"; }
g() { runuser -u "$OWNER" -- env HOME="/home/$OWNER" GIT_TERMINAL_PROMPT=0 git -C "$REPO" "$@"; }
keys() {
  local file="$1"; shift
  for k in "$@"; do
    if grep -Eq "^$k=.+" "$file" 2>/dev/null; then say "$(basename "$file") sets $k"; else say "$(basename "$file") does not set $k"; fi
  done
}

say "host $(hostname) at $(date -u +%Y-%m-%dT%H:%M:%SZ)"
say "os $(. /etc/os-release && printf '%s' "$PRETTY_NAME") kernel $(uname -r)"
if id "$OWNER" >/dev/null 2>&1; then say "user $OWNER uid=$(id -u "$OWNER") groups=$(id -nG "$OWNER" | tr ' ' ',')"; else say "user $OWNER MISSING"; fi

if [ -d "$REPO/.git" ]; then
  say "clone present at $REPO"
  say "head $(g rev-parse HEAD 2>&1 | head -n 1)"
  say "branch $(g rev-parse --abbrev-ref HEAD 2>&1 | head -n 1)"
  say "origin $(g remote get-url origin 2>&1 | head -n 1)"
  say "dirty-entries $(g status --porcelain 2>/dev/null | wc -l)"
else
  say "clone absent at $REPO"
fi

if [ -d "$SECRETS" ]; then
  say "secrets dir mode $(stat -c '%a %U' "$SECRETS")"
  for f in bbs.env smtp.env; do
    if [ -f "$SECRETS/$f" ]; then say "$f mode $(stat -c '%a %U' "$SECRETS/$f")"; else say "$f absent"; fi
  done
  keys "$SECRETS/bbs.env" ANIE_BBS_DB_PASSWORD ANIE_BBS_SECRET_KEY ANIE_BBS_SYSOP_MAIL
  keys "$SECRETS/smtp.env" SMTP_HOST SMTP_PORT SMTP_USERNAME SMTP_PASSWORD SMTP_FROM SMTP_FROM_NAME
else
  say "secrets dir absent at $SECRETS"
fi

say "docker $(docker version --format '{{.Server.Version}}' 2>&1 | head -n 1)"
say "compose $(docker compose version --short 2>&1 | head -n 1)"
say "containers-in-project $(docker ps -a --filter label=com.docker.compose.project=anie-bbs --format '{{.Names}}={{.State}}' 2>/dev/null | tr '\n' ' ')"
say "volumes-in-project $(docker volume ls --format '{{.Name}}' 2>/dev/null | grep '^anie-bbs' | tr '\n' ' ')"
for i in postgres:18-alpine rust:1.99-slim-trixie debian:trixie-slim anie-bbs:latest; do
  if docker image inspect "$i" >/dev/null 2>&1; then say "image $i present"; else say "image $i absent"; fi
done

for p in 2222 5440; do
  if ss -ltnH "sport = :$p" 2>/dev/null | grep -q .; then say "port $p LISTENING"; else say "port $p free"; fi
done

say "ufw $(ufw status 2>&1 | head -n 1)"
say "ufw ipv6 $(grep -E '^IPV6=' /etc/default/ufw 2>/dev/null)"
ufw show added 2>/dev/null | grep '^ufw ' | sed 's/^/inspect: ufw rule: /'
say "docker-user-chain $(iptables -S DOCKER-USER 2>/dev/null | wc -l) rules"

route="$(ip -4 route get 1.1.1.1 2>/dev/null | head -n 1)"
say "ipv4 route out: $route"
say "lan addresses $(ip -4 -o addr show scope global 2>/dev/null | awk '{print $2"="$4}' | tr '\n' ' ')"
say "arcanii.dynu.net resolves to $(getent ahostsv4 arcanii.dynu.net 2>/dev/null | awk '{print $1}' | sort -u | tr '\n' ' ')"

for target in github.com:443 smtp.gmail.com:587 smtp.gmail.com:465 index.crates.io:443 registry-1.docker.io:443; do
  host="${target%:*}"; port="${target#*:}"
  if timeout 6 bash -c "</dev/tcp/$host/$port" 2>/dev/null; then say "outbound $target reachable"; else say "outbound $target NOT reachable"; fi
done

if command -v ssh-keygen >/dev/null 2>&1; then say "ssh-keygen present"; else say "ssh-keygen absent"; fi
say "disk-free-root $(df -h --output=avail / | tail -n 1 | tr -d ' ')"
say "done"
