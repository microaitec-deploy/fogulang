#!/bin/sh
# Installs Fogu on Linux and macOS: downloads the executable (or copies a local
# one), puts it in a folder and adds that folder to the PATH.
#
#   curl -fsSL @FOGU_SERVER@/install.sh | sh
#   curl -fsSL @FOGU_SERVER@/install.sh | FOGU_VERSION=0.7.4 sh   # a specific version
#   sh scripts/install.sh                        # ~/.fogu/bin, added to your shell's PATH
#   sh scripts/install.sh --system               # /usr/local/bin (for every user; asks for sudo)
#   sh scripts/install.sh --from target/release/fogu   # copy a local build instead of downloading
#   sh scripts/install.sh --dir /opt/fogu/bin    # another folder
#   sh scripts/install.sh --uninstall
#
# The executables come from the GitHub release of the version
# (https://github.com/$FOGU_REPO/releases); when that download fails, from the
# Fogu download server (server/ in this project): $FOGU_SERVER/download/<version>/<file>.
# The server writes its own address in place of @FOGU_SERVER@ when it sends this script.
#
# Environment: FOGU_VERSION (default: latest), FOGU_REPO (the GitHub repository),
# FOGU_SERVER (the download server), FOGU_BASE_URL (the one folder the executables
# are downloaded from: no GitHub, no fallback), FOGU_DOWNLOAD_URL (a full URL of the
# executable), FOGU_INSTALL (= --dir).
set -eu

SERVER="${FOGU_SERVER:-@FOGU_SERVER@}"
# run from a checkout (not sent by a server): the official server
case "$SERVER" in @*@) SERVER="https://fogulang.microaitec.com" ;; esac
REPO="${FOGU_REPO:-microaitec-deploy/fogulang}"
VERSION="${FOGU_VERSION:-latest}"
VERSION="${VERSION#v}"
if [ "$VERSION" = latest ]; then
    GITHUB_URL="https://github.com/$REPO/releases/latest/download"
else
    GITHUB_URL="https://github.com/$REPO/releases/download/v$VERSION"
fi
SITE_URL="$SERVER/download/$VERSION"
DIR="${FOGU_INSTALL:-$HOME/.fogu/bin}"
FROM=""
SYSTEM=0
UNINSTALL=0

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --system) SYSTEM=1; DIR="/usr/local/bin" ;;
        --dir) shift; DIR="${1:?--dir needs a folder}" ;;
        --from) shift; FROM="${1:?--from needs a file}" ;;
        --uninstall) UNINSTALL=1 ;;
        -h|--help) sed -n '2,20p' "$0" 2>/dev/null || true; exit 0 ;;
        *) die "unknown option: $1 (see --help)" ;;
    esac
    shift
done

# sudo only when the folder is not writable (e.g. /usr/local/bin)
SUDO=""
need_sudo() {
    target="$1"
    while [ ! -e "$target" ]; do target=$(dirname "$target"); done
    if [ ! -w "$target" ]; then
        command -v sudo >/dev/null 2>&1 || die "$DIR is not writable and sudo is not available"
        SUDO="sudo"
    fi
}

# the shell start-up files that set the PATH
profiles() {
    for f in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
        [ -f "$f" ] && printf '%s\n' "$f"
    done
    case "${SHELL:-}" in
        */zsh) [ -f "$HOME/.zshrc" ] || printf '%s\n' "$HOME/.zshrc" ;;
        */bash) [ -f "$HOME/.bashrc" ] || printf '%s\n' "$HOME/.bashrc" ;;
    esac
}
MARK="# added by the Fogu installer"

if [ "$UNINSTALL" = 1 ]; then
    need_sudo "$DIR"
    $SUDO rm -f "$DIR/fogu" && say "removed $DIR/fogu"
    for f in $(profiles); do
        if [ -f "$f" ] && grep -q "$MARK" "$f"; then
            tmp=$(mktemp)
            grep -v "$MARK" "$f" > "$tmp" && cat "$tmp" > "$f" && rm -f "$tmp"
            say "removed the PATH line from $f"
        fi
    done
    fish="$HOME/.config/fish/conf.d/fogu.fish"
    [ -f "$fish" ] && rm -f "$fish" && say "removed $fish"
    exit 0
fi

# which executable: fogu (Linux x64), fogu-<os>-<arch> otherwise
os=$(uname -s); arch=$(uname -m)
case "$os-$arch" in
    Linux-x86_64|Linux-amd64) name="fogu" ;;
    Linux-aarch64|Linux-arm64) name="fogu-linux-arm64" ;;
    Linux-armv7l|Linux-armv6l) name="fogu-linux-armv7" ;;
    Darwin-arm64) name="fogu-macos-arm64" ;;
    Darwin-x86_64) name="fogu-macos-x64" ;;
    *) die "no Fogu executable for $os $arch — build it from source: cargo build --release" ;;
esac

need_sudo "$DIR"
$SUDO mkdir -p "$DIR"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

if [ -n "$FROM" ]; then
    [ -f "$FROM" ] || die "no such file: $FROM"
    cp "$FROM" "$tmp"
    say "copying $FROM"
else
    fetch() { # fetch <url>: into $tmp; false when the download fails
        say "downloading $1"
        if command -v curl >/dev/null 2>&1; then
            curl -fSL --progress-bar "$1" -o "$tmp"
        elif command -v wget >/dev/null 2>&1; then
            wget -q --show-progress -O "$tmp" "$1"
        else
            die "curl or wget is needed"
        fi
    }
    if [ -n "${FOGU_DOWNLOAD_URL:-}" ]; then
        fetch "$FOGU_DOWNLOAD_URL" || die "download failed: $FOGU_DOWNLOAD_URL"
    elif [ -n "${FOGU_BASE_URL:-}" ]; then
        fetch "$FOGU_BASE_URL/$name" || die "download failed (no executable for $os $arch at $FOGU_BASE_URL/$name?)"
    elif ! fetch "$GITHUB_URL/$name"; then
        say "not on GitHub; trying the download server"
        fetch "$SITE_URL/$name" || die "download failed (no executable for $os $arch: $GITHUB_URL/$name, $SITE_URL/$name)"
    fi
fi
chmod +x "$tmp"
"$tmp" --version >/dev/null 2>&1 || die "the file does not run on this machine ($os $arch)"
$SUDO mv -f "$tmp" "$DIR/fogu"
trap - EXIT
say "installed $DIR/fogu ($("$DIR/fogu" --version 2>/dev/null))"

# remember the server: `fogu build --compile --target <platform>` downloads the
# runtimes of other platforms from it
case "$SERVER" in
    http://*|https://*)
        home="${FOGU_HOME:-$HOME/.fogu}"
        mkdir -p "$home" && printf '%s\n' "$SERVER" > "$home/server" ;;
esac

# the PATH
case ":$PATH:" in
    *":$DIR:"*) say "$DIR is already on the PATH" ;;
    *)
        if [ "$SYSTEM" = 1 ]; then
            say "add $DIR to the PATH of your system to use 'fogu' everywhere"
        else
            line="export PATH=\"$DIR:\$PATH\" $MARK"
            for f in $(profiles); do
                if ! grep -qs "$MARK" "$f"; then
                    printf '\n%s\n' "$line" >> "$f"
                    say "added $DIR to the PATH in $f"
                fi
            done
            if command -v fish >/dev/null 2>&1; then
                mkdir -p "$HOME/.config/fish/conf.d"
                printf 'fish_add_path %s %s\n' "$DIR" "$MARK" > "$HOME/.config/fish/conf.d/fogu.fish"
                say "added $DIR to the PATH of fish"
            fi
            say "open a new terminal (or run: export PATH=\"$DIR:\$PATH\") and try: fogu --version"
        fi
        ;;
esac
