#!/bin/sh
set -eu

main() {
    if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
        echo 'Bipolar prebuilt releases support Linux x86_64 only.' >&2
        exit 1
    fi

    for command in curl tar sha256sum mktemp install; do
        if ! command -v "$command" >/dev/null 2>&1; then
            echo "Required command not found: $command" >&2
            exit 1
        fi
    done

    prefix=${BIPOLAR_INSTALL_DIR:-"$HOME/.local"}
    asset=bipolar-x86_64-unknown-linux-gnu.tar.gz
    release=https://github.com/iltumio/bipolar/releases/latest/download
    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT
    trap 'exit 1' HUP INT TERM

    echo 'Downloading Bipolar…'
    curl -fsSL "$release/$asset" -o "$tmpdir/$asset"
    curl -fsSL "$release/SHA256SUMS" -o "$tmpdir/SHA256SUMS"
    # Verify only the portable archive, not the other release assets.
    (cd "$tmpdir" && grep "  $asset\$" SHA256SUMS > archive.sha256 && sha256sum -c archive.sha256)
    mkdir -p "$tmpdir/bundle"
    tar -xzf "$tmpdir/$asset" -C "$tmpdir/bundle"

    install -Dm755 "$tmpdir/bundle/bin/bipolar" "$prefix/bin/bipolar"
    install -Dm644 "$tmpdir/bundle/share/applications/dev.iltumio.Bipolar.desktop" "$prefix/share/applications/dev.iltumio.Bipolar.desktop"
    install -Dm644 "$tmpdir/bundle/share/icons/hicolor/scalable/apps/dev.iltumio.Bipolar.svg" "$prefix/share/icons/hicolor/scalable/apps/dev.iltumio.Bipolar.svg"

    echo "Installed Bipolar in $prefix."
    case ":$PATH:" in
        *":$prefix/bin:"*) ;;
        *) echo "Add $prefix/bin to your shell's PATH to run bipolar from the terminal." ;;
    esac
}

main "$@"
