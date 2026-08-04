#!/usr/bin/env sh
set -eu

REPO="guisolski/muaddib"
INSTALL_DIR="${MUADDIB_INSTALL_DIR:-$HOME/.local/bin}"

die() {
	echo "muaddib: $1" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || die "$1 is required to install"
}

detect_target() {
	os="$(uname -s)"
	arch="$(uname -m)"
	case "$os/$arch" in
	Darwin/arm64) echo "aarch64-apple-darwin" ;;
	Darwin/x86_64) echo "x86_64-apple-darwin" ;;
	Linux/aarch64 | Linux/arm64) echo "aarch64-unknown-linux-gnu" ;;
	Linux/x86_64) echo "x86_64-unknown-linux-gnu" ;;
	*) die "no prebuilt binary for $os/$arch — build from source with 'cargo install muaddib'" ;;
	esac
}

latest_tag() {
	curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
		sed -n 's/.*"tag_name" *: *"\([^"]*\)".*/\1/p' |
		head -1
}

need curl
need tar

target="$(detect_target)"
tag="${MUADDIB_VERSION:-$(latest_tag)}"
[ -n "$tag" ] || die "could not resolve the latest release tag"

archive="muaddib-$target.tar.gz"
url="https://github.com/$REPO/releases/download/$tag/$archive"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "muaddib: downloading $tag for $target"
curl -fsSL "$url" -o "$tmp/$archive" || die "download failed: $url"

if command -v shasum >/dev/null 2>&1 &&
	curl -fsSL "$url.sha256" -o "$tmp/$archive.sha256" 2>/dev/null; then
	expected="$(cut -d' ' -f1 <"$tmp/$archive.sha256")"
	actual="$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)"
	[ "$expected" = "$actual" ] || die "checksum mismatch for $archive"
	echo "muaddib: checksum verified"
fi

tar -xzf "$tmp/$archive" -C "$tmp"
[ -f "$tmp/muaddib" ] || die "the archive did not contain a muaddib binary"

mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/muaddib" "$INSTALL_DIR/muaddib"
echo "muaddib: installed to $INSTALL_DIR/muaddib"

case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*) echo "muaddib: add $INSTALL_DIR to your PATH to run it by name" ;;
esac
