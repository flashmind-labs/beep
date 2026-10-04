# syntax=docker/dockerfile:1
FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --package beep-server --bin beep-server \
    && cp target/release/beep-server /usr/local/bin/

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home beep \
    && mkdir /data && chown beep:beep /data
COPY --from=build /usr/local/bin/beep-server /usr/local/bin/
USER beep
WORKDIR /data
# The server shuts down cleanly on ctrl-c only.
STOPSIGNAL SIGINT
ENTRYPOINT ["beep-server"]
CMD ["/data/config.toml"]
