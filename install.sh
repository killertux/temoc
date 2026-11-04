#!/bin/sh
# Installs the latest temoc release from GitHub.
#
#   curl -fsSL https://raw.githubusercontent.com/killertux/temoc/master/install.sh | sh
#
# Environment variables:
#   TEMOC_VERSION      Release tag to install (default: latest release)
#   TEMOC_INSTALL_DIR  Where to put the binary (default: $HOME/.local/bin)

set -eu

REPO="killertux/temoc"

err() {
    echo "temoc install: $*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || err "'$1' is required but was not found"
}

detect_target() {
    case "$(uname -m)" in
        x86_64 | amd64) arch="x86_64" ;;
        aarch64 | arm64) arch="aarch64" ;;
        *) err "unsupported architecture: $(uname -m)" ;;
    esac

    case "$(uname -s)" in
        Linux) echo "$arch-unknown-linux-musl" ;;
        Darwin) echo "$arch-apple-darwin" ;;
        *) err "unsupported operating system: $(uname -s)" ;;
    esac
}

main() {
    need curl
    need uname

    target="$(detect_target)"
    case "$target" in
        *-apple-darwin)
            need unzip
            asset="temoc-$target.zip"
            ;;
        *)
            need tar
            asset="temoc-$target.tar.gz"
            ;;
    esac

    version="${TEMOC_VERSION:-}"
    if [ -n "$version" ]; then
        url="https://github.com/$REPO/releases/download/$version/$asset"
    else
        url="https://github.com/$REPO/releases/latest/download/$asset"
    fi
    install_dir="${TEMOC_INSTALL_DIR:-$HOME/.local/bin}"

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    echo "Downloading $url"
    curl -fsSL --retry 3 -o "$tmp/$asset" "$url" \
        || err "download failed: $url"

    case "$asset" in
        *.zip) unzip -q "$tmp/$asset" -d "$tmp" ;;
        *) tar -xzf "$tmp/$asset" -C "$tmp" ;;
    esac
    [ -f "$tmp/temoc" ] || err "archive $asset does not contain a temoc binary"

    mkdir -p "$install_dir"
    # Stage next to the destination so the final rename is atomic and an
    # already-running temoc does not block the upgrade.
    cp "$tmp/temoc" "$install_dir/.temoc.tmp.$$"
    chmod 755 "$install_dir/.temoc.tmp.$$"
    mv -f "$install_dir/.temoc.tmp.$$" "$install_dir/temoc"

    echo "Installed temoc to $install_dir/temoc"

    case ":$PATH:" in
        *":$install_dir:"*) ;;
        *)
            echo
            echo "$install_dir is not in your PATH. Add it with:"
            echo "    export PATH=\"$install_dir:\$PATH\""
            ;;
    esac
}

# Everything runs from here so a truncated download cannot execute partially.
main "$@"
