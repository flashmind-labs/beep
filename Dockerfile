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

FROM source AS node-build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --package beep-node --bin beep-node \
    && cp target/release/beep-node /usr/local/bin/

# The user's environment: beep-node in Ubuntu, bundled with a single-user
# server (deploy/onprem) or run on any other machine, linked over iroh. The
# agent runs code and keeps the user's files in /home/user, a volume, as the
# user `user` with passwordless sudo, so it can apt install what it needs.
# Build it with `--target node`. The coding agents come from npm; set AGENTS
# to "" to leave them out.
FROM ubuntu:stonking-20261023.1 AS node
ARG AGENTS="@anthropic-ai/claude-code @agentclientprotocol/claude-agent-acp @openai/codex @agentclientprotocol/codex-acp"
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl git iproute2 openssh-client procps ripgrep sudo wget \
        build-essential nodejs npm poppler-utils \
        python3 python3-pip python3-venv python3-numpy python3-pandas \
        python3-scipy python3-matplotlib python3-openpyxl python3-pypdf \
        python3-cryptography python3-pil \
    && rm -rf /var/lib/apt/lists/* \
    # Ubuntu does not package pdfplumber.
    && pip install --no-cache-dir --break-system-packages pdfplumber \
    && if [ -n "$AGENTS" ]; then npm install -g $AGENTS && npm cache clean --force; fi \
    # The image's `ubuntu` user (uid 1000) becomes `user`, so a home volume
    # made for uid 1000 keeps working.
    && usermod --login user --home /home/user --move-home ubuntu \
    && groupmod --new-name user ubuntu \
    && echo 'user ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/user \
    && chmod 0440 /etc/sudoers.d/user \
    # pip installs for the user into ~/.local, which the volume keeps.
    && printf '[global]\nbreak-system-packages = true\n' > /etc/pip.conf
COPY --from=node-build /usr/local/bin/beep-node /usr/local/bin/
COPY deploy/onprem/egress.sh /usr/local/bin/beep-egress
COPY deploy/onprem/node-entrypoint.sh /usr/local/bin/beep-node-entrypoint
ENV BEEP_NODE_WORKSPACE=/home/user
# What pip installs for the user, also for the non-login shells the node runs.
ENV PATH=/home/user/.local/bin:$PATH
WORKDIR /home/user
VOLUME /home/user
# Starts as root to cap outgoing traffic, then runs the node as `user`.
ENTRYPOINT ["beep-node-entrypoint"]
CMD ["--dir", "/home/user/.config/beep-node", "local", "--link", "/link"]

FROM debian:trixie-slim
# iproute2 has the tc that caps a deploy/onprem server's outgoing traffic.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates iproute2 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home beep \
    && mkdir /data && chown beep:beep /data
COPY --from=build /usr/local/bin/beep-server /usr/local/bin/beep-admin /usr/local/bin/
COPY deploy/entrypoint.sh /usr/local/bin/beep-entrypoint
# deploy/onprem starts the server through this one instead.
COPY deploy/onprem/entrypoint.sh /usr/local/bin/beep-onprem
COPY deploy/onprem/egress.sh /usr/local/bin/beep-egress
# Starts as root, then runs the server as beep with no capabilities.
WORKDIR /data
# The server shuts down cleanly on ctrl-c only.
STOPSIGNAL SIGINT
ENTRYPOINT ["beep-entrypoint"]
CMD ["/data/config.toml"]
