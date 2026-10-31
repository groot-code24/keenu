#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Makes Keenu.app and a DMG in dist/ from a release build, on a Mac. Signs and notarizes when told how:
#
#   KEENU_BUNDLE_ID          the bundle identifier; required for a real release
#   KEENU_SIGNING_IDENTITY   a "Developer ID Application: ..." certificate in the keychain
#   KEENU_NOTARY_PROFILE     a profile saved with `xcrun notarytool store-credentials`
#
# Never run on a Mac at the time of writing: see docs/platform-support.md.
set -euo pipefail

cd "$(dirname "$0")/../.."
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
case "$(uname -m)" in
    arm64) ARCH=arm64 ;;
    x86_64) ARCH=x64 ;;
    *) echo "No Keenu build for $(uname -m)." >&2; exit 1 ;;
esac
BUNDLE_ID="${KEENU_BUNDLE_ID:-invalid.keenu.placeholder}"
if [ "$BUNDLE_ID" = invalid.keenu.placeholder ]; then
    echo "Warning: KEENU_BUNDLE_ID is not set; using a placeholder. Do not publish this build." >&2
fi
OUT="dist"
APP="$OUT/Keenu.app"
BINARY="${CARGO_TARGET_DIR:-target}/release/keenu-macos"
[ -x "$BINARY" ] || cargo build --release --locked -p keenu-macos

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/Keenu"
cp packaging/icons/keenu.icns "$APP/Contents/Resources/Keenu.icns"
# The numbers alone: macOS does not take a prerelease suffix here.
NUMERIC="${VERSION%%-*}"
sed -e "s/@BUNDLE_ID@/$BUNDLE_ID/" -e "s/@VERSION@/$NUMERIC/" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

if [ -n "${KEENU_SIGNING_IDENTITY:-}" ]; then
    codesign --force --options runtime --timestamp --entitlements packaging/macos/Keenu.entitlements \
        --sign "$KEENU_SIGNING_IDENTITY" "$APP"
    codesign --verify --strict --verbose=2 "$APP"
    SIGNED=1
else
    # Ad hoc, which Apple Silicon needs to run it at all; it proves nothing about who made it.
    codesign --force --sign - "$APP"
    SIGNED=0
fi

DMG="$OUT/Keenu-$VERSION-macos-$ARCH.dmg"
rm -f "$DMG"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Keenu $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
hdiutil verify -quiet "$DMG"

if [ "$SIGNED" = 1 ] && [ -n "${KEENU_NOTARY_PROFILE:-}" ]; then
    codesign --force --timestamp --sign "$KEENU_SIGNING_IDENTITY" "$DMG"
    xcrun notarytool submit "$DMG" --keychain-profile "$KEENU_NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
    spctl --assess --type open --context context:primary-signature --verbose "$DMG"
else
    echo "Note: $DMG is not notarized; macOS will warn people who open it." >&2
fi
echo "$DMG"
