# syntax=docker/dockerfile:1.7
#
# Builds the fogu executable for several platforms and exports them to dist/:
#
#   docker build -f docker/Dockerfile --output dist .
#
#   dist/fogu-linux-x64          Linux x86_64
#   dist/fogu-linux-arm64        Linux aarch64 (Raspberry Pi 3/4/5, 64-bit OS)
#   dist/fogu-linux-armv7        Linux armv7   (Raspberry Pi 2/3/4, 32-bit OS; no JIT)
#   dist/fogu-windows-x64.exe    Windows x86_64
#   dist/fogu-macos-x64          macOS Intel        (only with MACOS_SDK_URL)
#   dist/fogu-macos-arm64        macOS Apple Silicon (only with MACOS_SDK_URL)
#
# Choose the platforms with --build-arg TARGETS="linux-x64 linux-arm64".
# macOS needs Apple's SDK, which cannot be redistributed: pass a link to a
# MacOSX*.sdk.tar.xz you are allowed to use with --build-arg MACOS_SDK_URL=...
#
# These binaries are also the runtimes of `fogu build --compile --target ...`:
# fogu finds them in dist/, next to itself or in ~/.fogu/runtimes.

FROM rust:1-bookworm AS toolchain

RUN dpkg --add-architecture arm64 && \
    dpkg --add-architecture armhf && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
        pkg-config build-essential clang \
        libssl-dev libssl-dev:arm64 libssl-dev:armhf \
        gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross \
        gcc-arm-linux-gnueabihf g++-arm-linux-gnueabihf libc6-dev-armhf-cross \
        gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
        xz-utils curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN rustup target add \
        aarch64-unknown-linux-gnu \
        armv7-unknown-linux-gnueabihf \
        x86_64-pc-windows-gnu \
        x86_64-apple-darwin \
        aarch64-apple-darwin

# linkers, C compilers (for crates with C code) and OpenSSL of each target
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc \
    CXX_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-g++ \
    AR_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-ar \
    CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
    CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++ \
    AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar \
    PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_PATH_aarch64_unknown_linux_gnu=/usr/lib/aarch64-linux-gnu/pkgconfig \
    PKG_CONFIG_PATH_armv7_unknown_linux_gnueabihf=/usr/lib/arm-linux-gnueabihf/pkgconfig

# --------------------------------------------------------------------------

FROM toolchain AS build

ARG TARGETS="linux-x64 linux-arm64 linux-armv7 windows-x64"
ARG MACOS_SDK_URL=""
ARG ZIG_VERSION=0.13.0

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    set -eu; \
    mkdir -p /out; \
    build() { \
        triple="$1"; name="$2"; ext="$3"; \
        echo "==> fogu-$name ($triple)"; \
        cargo build --release --locked --target "$triple"; \
        cp "target/$triple/release/fogu$ext" "/out/fogu-$name$ext"; \
    }; \
    for t in $TARGETS; do \
        case "$t" in \
            linux-x64)   build x86_64-unknown-linux-gnu linux-x64 "" ;; \
            linux-arm64) build aarch64-unknown-linux-gnu linux-arm64 "" ;; \
            linux-armv7) build armv7-unknown-linux-gnueabihf linux-armv7 "" ;; \
            windows-x64) build x86_64-pc-windows-gnu windows-x64 ".exe" ;; \
            macos-*) ;; \
            *) echo "unknown target: $t" >&2; exit 1 ;; \
        esac; \
    done; \
    if [ -n "$MACOS_SDK_URL" ]; then \
        echo "==> macOS (zig + Apple SDK)"; \
        curl -fsSL "https://ziglang.org/download/$ZIG_VERSION/zig-linux-$(uname -m)-$ZIG_VERSION.tar.xz" | tar -xJ -C /opt; \
        export PATH="/opt/zig-linux-$(uname -m)-$ZIG_VERSION:$PATH"; \
        cargo install --locked cargo-zigbuild; \
        mkdir -p /opt/sdk && curl -fsSL "$MACOS_SDK_URL" | tar -xJ -C /opt/sdk --strip-components=1; \
        export SDKROOT=/opt/sdk; \
        for triple in x86_64-apple-darwin aarch64-apple-darwin; do \
            cargo zigbuild --release --locked --target "$triple"; \
        done; \
        cp target/x86_64-apple-darwin/release/fogu /out/fogu-macos-x64; \
        cp target/aarch64-apple-darwin/release/fogu /out/fogu-macos-arm64; \
    fi; \
    ls -la /out

# --------------------------------------------------------------------------

# only the executables: `docker build --output dist` writes them to dist/
FROM scratch AS dist
COPY --from=build /out/ /
