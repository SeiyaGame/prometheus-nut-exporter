## Builder stage: cross-compiles a static musl binary on the build platform (no emulation)
FROM --platform=$BUILDPLATFORM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app

# Fetch deps using dummy app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
&& echo "fn main() {}" > src/main.rs \
&& cargo fetch --locked \
&& rm -rf src/

# Build app
COPY src/ src/
ARG APP_VERSION=0.0.0-SNAPSHOT
ARG TARGETPLATFORM
RUN \
case $TARGETPLATFORM in \
"linux/386") RUST_TARGET="i686-unknown-linux-musl" ;; \
"linux/amd64") RUST_TARGET="x86_64-unknown-linux-musl" ;; \
"linux/arm/v6") RUST_TARGET="arm-unknown-linux-musleabi" ;; \
"linux/arm/v7") RUST_TARGET="armv7-unknown-linux-musleabihf" ;; \
"linux/arm64") RUST_TARGET="aarch64-unknown-linux-musl" ;; \
*) false ;; \
esac \
&& rustup target add $RUST_TARGET \
&& APP_VERSION="$APP_VERSION" cargo rustc --locked --bin prometheus-nut-exporter --target=$RUST_TARGET --release -- -C linker=rust-lld \
&& mv target/$RUST_TARGET/release/prometheus-nut-exporter .

## Runtime stage (no RUN: multi-arch images build without QEMU)
FROM alpine:3.22 AS runtime
WORKDIR /app
ARG APP_UID=5000
ARG APP_GID=5000
USER $APP_UID:$APP_GID
EXPOSE 9995
# The home page answers without scraping a NUT server (busybox wget, HTTP_ADDRESS must include 127.0.0.1)
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null "http://127.0.0.1:${HTTP_PORT:-9995}/" || exit 1
ENTRYPOINT ["./prometheus-nut-exporter"]

## The CI builds the binaries once and only packages them: --target prebuilt
FROM runtime AS prebuilt
ARG TARGETARCH
ARG TARGETVARIANT
COPY --chmod=755 dist/prometheus-nut-exporter-linux-${TARGETARCH}${TARGETVARIANT} ./prometheus-nut-exporter

## Default: built from source
FROM runtime
COPY --from=builder --chmod=755 /app/prometheus-nut-exporter ./
