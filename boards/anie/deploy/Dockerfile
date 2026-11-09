# The Anie Solutions BBS: built from the workspace (the context is the
# repository's root), run as a user of its own from a read-only image.
# compose.yaml builds it; nothing secret goes into it.

FROM rust:1.99-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p anie-bbs && cp target/release/anie-bbs /usr/local/bin/anie-bbs

FROM debian:trixie-slim
RUN useradd --system --uid 10001 --home-dir /var/lib/anie-bbs --shell /usr/sbin/nologin bbs \
 && mkdir -p /var/lib/anie-bbs && chown bbs:bbs /var/lib/anie-bbs && chmod 700 /var/lib/anie-bbs
COPY --from=build /usr/local/bin/anie-bbs /usr/local/bin/anie-bbs
COPY boards/anie/screens /opt/anie-bbs/screens
ENV ANIE_BBS_SCREENS=/opt/anie-bbs/screens \
    ANIE_BBS_HOST_KEY=/var/lib/anie-bbs/host_key
USER bbs
WORKDIR /var/lib/anie-bbs
EXPOSE 2222
ENTRYPOINT ["/usr/local/bin/anie-bbs"]
