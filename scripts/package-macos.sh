#!/usr/bin/env bash
# Builds Glasshouse.app (a universal arm64 + x86_64 binary, with the app icon) and packs it into
# dist/Glasshouse-<version>-macos-universal.{zip,dmg}. The Glasshouse Agent ships inside the
# bundle, next to the app, in Contents/MacOS.
#
#   scripts/package-macos.sh            # version from Cargo.toml
#   VERSION=1.2.3 scripts/package-macos.sh
#
# Signing: ad hoc unless CODESIGN_IDENTITY names a Developer ID certificate in the keychain.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$PWD

VERSION=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
BUILD=${BUILD:-${GITHUB_RUN_NUMBER:-1}}
MIN_MACOS=${MIN_MACOS:-13.0}
BINARIES=(glasshouse glasshouse-agent)

# Universal needs rustup to add the other architecture; without it (e.g. Homebrew's Rust), build
# for this Mac only.
if command -v rustup >/dev/null; then
  TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
  FLAVOR=universal
else
  TARGETS=("$(rustc -vV | sed -n 's/^host: //p')")
  FLAVOR=$(uname -m)
  echo "rustup not found: building for $FLAVOR only" >&2
fi

DIST=$ROOT/dist
WORK=$ROOT/target/package/macos
APP=$WORK/Glasshouse.app
NAME=Glasshouse-$VERSION-macos-$FLAVOR

rm -rf "$WORK"
mkdir -p "$WORK" "$DIST"

# --- Build both architectures -------------------------------------------------------------
export MACOSX_DEPLOYMENT_TARGET=$MIN_MACOS
for target in "${TARGETS[@]}"; do
  if command -v rustup >/dev/null; then rustup target add "$target" >/dev/null; fi
  cargo build --release --locked --target "$target" -p glasshouse -p glasshouse-agent
done

# --- Bundle -------------------------------------------------------------------------------
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
for bin in "${BINARIES[@]}"; do
  inputs=()
  for target in "${TARGETS[@]}"; do inputs+=("target/$target/release/$bin"); done
  lipo -create -output "$APP/Contents/MacOS/$bin" "${inputs[@]}"
done

sed -e "s/@VERSION@/$VERSION/" -e "s/@BUILD@/$BUILD/" -e "s/@MIN_MACOS@/$MIN_MACOS/" \
  packaging/macos/Info.plist >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
printf 'APPL????' >"$APP/Contents/PkgInfo"

# --- App icon: docs/icon.png (1024 px) -> AppIcon.icns -------------------------------------
ICONSET=$WORK/AppIcon.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size docs/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) docs/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

# --- Sign ---------------------------------------------------------------------------------
IDENTITY=${CODESIGN_IDENTITY:--}
SIGN_FLAGS=(--force --timestamp=none --sign "$IDENTITY")
[[ $IDENTITY != - ]] && SIGN_FLAGS=(--force --timestamp --options runtime --sign "$IDENTITY")
codesign "${SIGN_FLAGS[@]}" "$APP/Contents/MacOS/glasshouse-agent"
codesign "${SIGN_FLAGS[@]}" "$APP"
codesign --verify --strict "$APP"

# --- Archives -----------------------------------------------------------------------------
rm -f "$DIST/$NAME.zip" "$DIST/$NAME.dmg"
ditto -c -k --keepParent "$APP" "$DIST/$NAME.zip"

DMG_ROOT=$WORK/dmg
mkdir -p "$DMG_ROOT"
cp -R "$APP" "$DMG_ROOT/"
ln -s /Applications "$DMG_ROOT/Applications"
hdiutil create -quiet -volname Glasshouse -srcfolder "$DMG_ROOT" -fs HFS+ -format UDZO "$DIST/$NAME.dmg"

echo "Built:"
ls -lh "$DIST/$NAME.zip" "$DIST/$NAME.dmg"
