
# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS toolchain

ARG DEBIAN_FRONTEND=noninteractive

# Cross-compilation toolchains (gcc for Linux arm64/armv7, MinGW for Windows).
RUN dpkg --add-architecture arm64 \
    && dpkg --add-architecture armhf \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        xz-utils \
        pkg-config \
        build-essential \
        clang \
        libssl-dev \
        libssl-dev:arm64 \
        libssl-dev:armhf \
        gcc-aarch64-linux-gnu \
        g++-aarch64-linux-gnu \
        libc6-dev-arm64-cross \
        gcc-arm-linux-gnueabihf \
        g++-arm-linux-gnueabihf \
        libc6-dev-armhf-cross \
        gcc-mingw-w64-x86-64 \
        g++-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add \
    x86_64-unknown-linux-gnu \
    aarch64-unknown-linux-gnu \
    armv7-unknown-linux-gnueabihf \
    x86_64-pc-windows-gnu \
    x86_64-apple-darwin \
    aarch64-apple-darwin

# Linkers and C compilers for the Linux and Windows targets.
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

# .dockerignore keeps target/, .git/ and other local files out of the context.
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    set -eu; \
    mkdir -p /out; \
    build() { \
        triple="$1"; \
        name="$2"; \
        ext="$3"; \
        echo "==> Compiling fogu-$name ($triple)"; \
        cargo build --profile dist --locked --target "$triple"; \
        src="target/$triple/dist/fogu$ext"; \
        test -f "$src" || { echo "Binary not found: $src" >&2; exit 1; }; \
        cp "$src" "/out/fogu-$name$ext"; \
        echo "==> Compiling the C API for $name ($triple)"; \
        cargo build --profile dist --locked --target "$triple" -p fogu-ffi; \
        if [ "$ext" = ".exe" ]; then \
            cp "target/$triple/dist/fogu_rt.dll" "/out/fogu_rt-$name.dll"; \
        else \
            cp "target/$triple/dist/libfogu_rt.so" "/out/libfogu_rt-$name.so"; \
        fi; \
    }; \
    for t in $TARGETS; do \
        case "$t" in \
            linux-x64) \
                build x86_64-unknown-linux-gnu linux-x64 "" ;; \
            linux-arm64) \
                build aarch64-unknown-linux-gnu linux-arm64 "" ;; \
            linux-armv7) \
                build armv7-unknown-linux-gnueabihf linux-armv7 "" ;; \
            windows-x64) \
                build x86_64-pc-windows-gnu windows-x64 ".exe" ;; \
            macos-x64|macos-arm64) \
                test -n "$MACOS_SDK_URL" || { \
                    echo "macOS requires MACOS_SDK_URL" >&2; exit 1; \
                } ;; \
            *) \
                echo "Unknown target: $t" >&2; exit 1 ;; \
        esac; \
    done; \
    case " $TARGETS " in \
        *" macos-x64 "*|*" macos-arm64 "*) \
            echo "==> Installing Zig $ZIG_VERSION"; \
            case "$(uname -m)" in \
                x86_64) zig_arch=x86_64 ;; \
                aarch64) zig_arch=aarch64 ;; \
                *) echo "Unsupported build architecture: $(uname -m)" >&2; exit 1 ;; \
            esac; \
            curl -fsSL "https://ziglang.org/download/$ZIG_VERSION/zig-linux-$zig_arch-$ZIG_VERSION.tar.xz" \
                | tar -xJ -C /opt; \
            export PATH="/opt/zig-linux-$zig_arch-$ZIG_VERSION:$PATH"; \
            cargo install --locked cargo-zigbuild; \
            mkdir -p /opt/sdk; \
            curl -fsSL "$MACOS_SDK_URL" \
                | tar -xJ -C /opt/sdk --strip-components=1; \
            export SDKROOT=/opt/sdk; \
            case " $TARGETS " in \
                *" macos-x64 "*) \
                    cargo zigbuild --profile dist --locked --target x86_64-apple-darwin; \
                    cp target/x86_64-apple-darwin/dist/fogu /out/fogu-macos-x64 ;; \
            esac; \
            case " $TARGETS " in \
                *" macos-arm64 "*) \
                    cargo zigbuild --profile dist --locked --target aarch64-apple-darwin; \
                    cp target/aarch64-apple-darwin/dist/fogu /out/fogu-macos-arm64 ;; \
            esac ;; \
    esac; \
    echo "==> Build output"; \
    ls -lh /out

# --------------------------------------------------------------------------

FROM scratch AS dist
COPY --from=build /out/ /
