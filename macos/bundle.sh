#!/bin/sh
# Builds target/release/Notchpad.app. With --install, also replaces /Applications/Notchpad.app.
# Signed with the first Apple Development identity in the keychain, so macOS keeps the
# app's privacy permissions across rebuilds; ad-hoc signing (the fallback) changes the
# app's identity every build and macOS asks for every permission again.
set -eu
cd "$(dirname "$0")/.."

# To embed the registered public Linear OAuth client ID in the app binary:
#   NOTCHPAD_LINEAR_CLIENT_ID=lin_api_... macos/bundle.sh
# The redirect URI must be registered exactly as http://127.0.0.1:53682/oauth/callback.
cargo build --release
app=target/release/Notchpad.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp macos/Info.plist "$app/Contents/"
cp macos/AppIcon.icns "$app/Contents/Resources/"
cp target/release/notchpad target/release/notchpad-mcp "$app/Contents/MacOS/"
identity=$(security find-identity -v -p codesigning | awk '/Apple Development/ { print $2; exit }')
codesign --force --sign "${identity:--}" "$app/Contents/MacOS/notchpad-mcp"
codesign --force --sign "${identity:--}" "$app"
echo "built $app"

if [ "${1:-}" = "--install" ]; then
    rm -rf /Applications/Notchpad.app
    cp -R "$app" /Applications/
    echo "installed /Applications/Notchpad.app"
fi
