#!/usr/bin/env bash
# Builds Glasshouse for this Linux machine's architecture and packs it, with its desktop entry and
# icons, into dist/glasshouse-<version>-linux-<arch>.tar.gz. The archive mirrors a /usr/local
# prefix, so `sudo tar -xzf ... -C /usr/local --strip-components=1` installs it.
#
#   scripts/package-linux.sh            # version from Cargo.toml
#   VERSION=1.2.3 scripts/package-linux.sh
#
# Needs libxkbcommon-dev to link, and ImageMagick (`convert`) or `magick` for the icon sizes.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$PWD

VERSION=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
ARCH=$(uname -m)
NAME=glasshouse-$VERSION-linux-$ARCH
DIST=$ROOT/dist
STAGE=$ROOT/target/package/linux/$NAME

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$DIST"

cargo build --release --locked -p glasshouse -p glasshouse-agent
install -m 755 target/release/glasshouse target/release/glasshouse-agent "$STAGE/bin/"
install -m 644 packaging/linux/glasshouse.desktop "$STAGE/share/applications/"

# App icon: docs/icon.png (1024 px) at the hicolor theme's sizes.
if command -v magick >/dev/null; then RESIZE=(magick); else RESIZE=(convert); fi
for size in 16 32 48 64 128 256 512; do
  dir=$STAGE/share/icons/hicolor/${size}x${size}/apps
  mkdir -p "$dir"
  "${RESIZE[@]}" docs/icon.png -resize "${size}x${size}" "$dir/glasshouse.png"
done
mkdir -p "$STAGE/share/icons/hicolor/1024x1024/apps"
install -m 644 docs/icon.png "$STAGE/share/icons/hicolor/1024x1024/apps/glasshouse.png"

install -m 644 LICENSE README.md "$STAGE/"

tar -C "$(dirname "$STAGE")" -czf "$DIST/$NAME.tar.gz" "$NAME"
echo "Built:"
ls -lh "$DIST/$NAME.tar.gz"
