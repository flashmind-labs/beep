# syntax=docker/dockerfile:1
FROM rust:1.99-slim-trixie AS source
WORKDIR /src
COPY . .

# CI runs this stage before building the image.
FROM source AS check
RUN rustup component add rustfmt clippy
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo fmt --all --check \
    && cargo clippy -p beep-server -p beep-proto --all-targets -- -D warnings \
    && cargo test -p beep-server -p beep-proto

FROM source AS build
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --package beep-server --bin beep-server \
    && cp target/release/beep-server /usr/local/bin/

# The static beep-node release binaries for Linux, cross-built with zig:
# `docker build --target node-dist --output type=local,dest=<dir> .` writes
# <dir>/<target>/beep-node. The node workflow runs it.
FROM rust:1.99-slim-trixie AS node-dist-build
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir --break-system-packages ziglang==0.15.2 cargo-zigbuild==0.23.4 \
    && rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
WORKDIR /src
COPY . .
ARG BEEP_SERVER_URL
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do \
        cargo zigbuild --release --locked --package beep-node --bin beep-node --target "$target" \
        && mkdir -p "/dist/$target" \
        && cp "target/$target/release/beep-node" "/dist/$target/" || exit 1; \
    done

# The Windows build, cross-compiled with cargo-xwin against the MSVC CRT it
# downloads. `--target node-dist-windows` writes
# <dir>/x86_64-pc-windows-msvc/beep-node.exe.
FROM rust:1.99-slim-trixie AS node-dist-windows-build
RUN apt-get update \
    && apt-get install -y --no-install-recommends clang lld llvm python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir --break-system-packages cargo-xwin==0.23.1 \
    && rustup target add x86_64-pc-windows-msvc
WORKDIR /src
COPY . .
ARG BEEP_SERVER_URL
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    --mount=type=cache,target=/root/.cache/cargo-xwin,sharing=locked \
    cargo xwin build --release --locked --package beep-node --bin beep-node --target x86_64-pc-windows-msvc \
    && mkdir -p /dist/x86_64-pc-windows-msvc \
    && cp target/x86_64-pc-windows-msvc/release/beep-node.exe /dist/x86_64-pc-windows-msvc/

FROM scratch AS node-dist
COPY --from=node-dist-build /dist /

FROM scratch AS node-dist-windows
COPY --from=node-dist-windows-build /dist /

# The beep-node container image: the static binary, git, a shell, CA
# certificates and curl (the node fetches gh and its tools with it), running as uid 1000 with its state in /data. Its files are
# put together on the build machine for either architecture, so no emulation
# is needed: `docker build --platform linux/arm64 --target node-image .`.
# The node workflow hands in the binaries it built with
# `--build-context node-dist=<dir>` instead of compiling them again.
FROM --platform=$BUILDPLATFORM alpine:3.22 AS node-image-root
ARG TARGETARCH
RUN --mount=from=node-dist,target=/dist set -eu; \
    case "$TARGETARCH" in \
        amd64) arch=x86_64 ;; \
        arm64) arch=aarch64 ;; \
        *) echo "There is no beep-node for $TARGETARCH." >&2; exit 1 ;; \
    esac; \
    apk add --root /out --initdb --arch "$arch" --no-scripts --no-cache \
        --keys-dir "/usr/share/apk/keys/$arch" --repositories-file /etc/apk/repositories \
        alpine-baselayout busybox ca-certificates-bundle curl git; \
    # --no-scripts skips the busybox links, which this one's list matches.
    for applet in $(busybox --list-full); do \
        [ -e "/out/$applet" ] || ln -s /bin/busybox "/out/$applet"; \
    done; \
    echo 'beep:x:1000:1000:beep:/data:/bin/sh' >> /out/etc/passwd; \
    echo 'beep:x:1000:' >> /out/etc/group; \
    mkdir -p /out/data /out/usr/local/bin; \
    chown 1000:1000 /out/data; \
    install -m 0755 "/dist/$arch-unknown-linux-musl/beep-node" /out/usr/local/bin/beep-node
COPY deploy/node-entrypoint.sh /out/usr/local/bin/beep-node-entrypoint

FROM scratch AS node-image
COPY --from=node-image-root /out /
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    HOME=/data
USER 1000:1000
WORKDIR /data
VOLUME /data
EXPOSE 47120/udp
ENTRYPOINT ["beep-node-entrypoint"]

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home beep \
    && mkdir /data && chown beep:beep /data
COPY --from=build /usr/local/bin/beep-server /usr/local/bin/
COPY deploy/entrypoint.sh /usr/local/bin/beep-entrypoint
# Starts as root, then runs the server as beep with no capabilities.
WORKDIR /data
ENTRYPOINT ["beep-entrypoint"]
CMD ["/data/config.toml"]
