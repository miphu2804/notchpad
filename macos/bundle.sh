#!/bin/sh
# Builds target/release/Notchpad.app. With --install, also replaces /Applications/Notchpad.app.
# Ad-hoc signed: enough for this Mac, not for distributing to others.
set -eu
cd "$(dirname "$0")/.."

cargo build --release
app=target/release/Notchpad.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp macos/Info.plist "$app/Contents/"
cp macos/AppIcon.icns "$app/Contents/Resources/"
cp target/release/notchpad target/release/notchpad-mcp "$app/Contents/MacOS/"
codesign --force --sign - "$app"
echo "built $app"

if [ "${1:-}" = "--install" ]; then
    rm -rf /Applications/Notchpad.app
    cp -R "$app" /Applications/
    echo "installed /Applications/Notchpad.app"
fi
