# The Anie Solutions BBS: built from the workspace (the context is the
# repository's root), run as a user of its own from a read-only image, with
# flow-cat (Neko, a door of the game room's) on its PATH and Neko's notice
# beside it, as its sprites' terms ask, and flow-mines (Mines, the game
# room's other door) from arcanii/Flow-Games, a second context (flow-games)
# that compose.yaml names, and Dungeon (the game room's third door) built
# from devshane/zork. compose.yaml builds it; nothing secret goes into it:
# Flow-Games' clone is made on the box with its deploy key, which the build
# never sees.

FROM rust:1.99-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p anie-bbs -p flow-cat  && cp target/release/anie-bbs target/release/flow-cat /usr/local/bin/

# Mines, from Flow-Games' own workspace and lock.
FROM rust:1.99-slim-trixie AS games
WORKDIR /games
COPY --from=flow-games . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/games/target \
    cargo build --release --locked -p flow-mines && cp target/release/flow-mines /usr/local/bin/

# Dungeon, the mainframe original of Zork (public domain), from
# devshane/zork at a pinned commit, as it is but for dungeon.patch: no `!`
# shell escape, and a line's words kept to what its parser holds. Built
# without its debugging tool (GDT) or a pager, and hardened, so a slip
# elsewhere in its 1980s C stops the game rather than going on.
FROM debian:trixie-slim AS dungeon
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates git gcc libc6-dev make \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /dungeon
RUN git init -q && git remote add origin https://github.com/devshane/zork.git \
 && git fetch -q --depth 1 origin bcf3403b2db3ce3cde499fdf2cbabc251f6f1876 && git checkout -q FETCH_HEAD
COPY boards/anie/deploy/dungeon.patch /tmp/dungeon.patch
RUN git apply /tmp/dungeon.patch \
 && make LIBS= TERMFLAG=-DMORE_NONE GDTFLAG= DATADIR=/usr/local/share/dungeon \
      CFLAGS="-O2 -fstack-protector-strong -fstack-clash-protection -D_FORTIFY_SOURCE=3 -fPIE" \
      LDFLAGS="-pie -Wl,-z,relro,-z,now" \
 && install -D -m 755 zork /usr/local/bin/dungeon \
 && install -D -m 644 dtextc.dat /usr/local/share/dungeon/dtextc.dat \
 && install -D -m 644 readme.txt /usr/share/doc/dungeon/readme.txt

FROM debian:trixie-slim
RUN useradd --system --uid 10001 --home-dir /var/lib/anie-bbs --shell /usr/sbin/nologin bbs \
 && mkdir -p /var/lib/anie-bbs && chown bbs:bbs /var/lib/anie-bbs && chmod 700 /var/lib/anie-bbs
COPY --from=build /usr/local/bin/anie-bbs /usr/local/bin/flow-cat /usr/local/bin/
COPY --from=games /usr/local/bin/flow-mines /usr/local/bin/
COPY --from=dungeon /usr/local/bin/dungeon /usr/local/bin/
COPY --from=dungeon /usr/local/share/dungeon /usr/local/share/dungeon
COPY --from=dungeon /usr/share/doc/dungeon /usr/share/doc/dungeon
COPY doors/cat/sprites/NOTICE.md /usr/share/doc/flow-cat/NOTICE.md
COPY boards/anie/screens /opt/anie-bbs/screens
ENV ANIE_BBS_SCREENS=/opt/anie-bbs/screens \
    ANIE_BBS_HOST_KEY=/var/lib/anie-bbs/host_key \
    ANIE_BBS_SAVES=/var/lib/anie-bbs/saves
USER bbs
WORKDIR /var/lib/anie-bbs
EXPOSE 2222
ENTRYPOINT ["/usr/local/bin/anie-bbs"]
