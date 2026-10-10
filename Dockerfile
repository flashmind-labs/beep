# syntax=docker/dockerfile:1
FROM rust:1-slim-trixie AS source
WORKDIR /src
COPY . .

# CI runs this stage before building the image. The tests need the Postgres
# named by BEEP_TEST_DATABASE_URL, so build it with `--network host`.
FROM source AS check
RUN rustup component add rustfmt clippy
ARG BEEP_TEST_DATABASE_URL
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo fmt --all --check \
    && cargo clippy -p beep-server -p beep-proto --all-targets -- -D warnings \
    && BEEP_TEST_DATABASE_URL="$BEEP_TEST_DATABASE_URL" cargo test -p beep-server -p beep-proto

FROM source AS build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --package beep-server --bin beep-server --bin beep-admin \
    && cp target/release/beep-server target/release/beep-admin /usr/local/bin/

FROM debian:trixie-slim
# passt gives Drop its network, poppler-utils has pdftotext.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates passt poppler-utils python3 python3-venv \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home beep \
    && mkdir /data && chown beep:beep /data
# Python for the sandbox, under /usr so Drop exposes it read-only.
RUN python3 -m venv /usr/local/beep-python \
    && /usr/local/beep-python/bin/pip install --no-cache-dir \
        numpy pandas scipy matplotlib openpyxl pypdf pdfplumber
ENV PATH=/usr/local/beep-python/bin:$PATH
ARG DROP_VERSION=v0.3.0
ARG TARGETARCH
ADD --chmod=755 https://github.com/wrr/drop/releases/download/${DROP_VERSION}/drop-linux-${TARGETARCH} /usr/local/bin/drop
COPY --from=build /usr/local/bin/beep-server /usr/local/bin/beep-admin /usr/local/bin/
COPY deploy/entrypoint.sh /usr/local/bin/beep-entrypoint
# Starts as root to prepare /etc for Drop, then runs the server as beep.
WORKDIR /data
# The server shuts down cleanly on ctrl-c only.
STOPSIGNAL SIGINT
ENTRYPOINT ["beep-entrypoint"]
CMD ["/data/config.toml"]
