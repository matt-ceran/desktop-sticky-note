#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Desktop Sticky Note"
BUNDLE_ID="com.mattceran.desktop-sticky-note"
BIN_NAME="desktop-sticky-note"
APP_DIR="$HOME/Applications/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
LAUNCH_AGENT="$HOME/Library/LaunchAgents/$BUNDLE_ID.plist"
LOG_DIR="$HOME/Library/Logs"

mkdir -p "$MACOS_DIR" "$HOME/Library/LaunchAgents" "$LOG_DIR"

cargo build --manifest-path "$ROOT/Cargo.toml" --release
cp "$ROOT/target/release/$BIN_NAME" "$MACOS_DIR/$BIN_NAME"
chmod 755 "$MACOS_DIR/$BIN_NAME"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>$BIN_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP_DIR"
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [ -x "$LSREGISTER" ]; then
  "$LSREGISTER" -f "$APP_DIR" >/dev/null 2>&1 || true
fi

cat > "$LAUNCH_AGENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$BUNDLE_ID</string>
  <key>ProgramArguments</key>
  <array>
    <string>$MACOS_DIR/$BIN_NAME</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>StandardOutPath</key>
  <string>$LOG_DIR/$BIN_NAME.log</string>
  <key>StandardErrorPath</key>
  <string>$LOG_DIR/$BIN_NAME.err.log</string>
</dict>
</plist>
PLIST

chmod 644 "$LAUNCH_AGENT"
launchctl bootout "gui/$UID/$BUNDLE_ID" >/dev/null 2>&1 || true
for _ in {1..20}; do
  launchctl print "gui/$UID/$BUNDLE_ID" >/dev/null 2>&1 || break
  sleep 0.25
done
launchctl enable "gui/$UID/$BUNDLE_ID"
launchctl bootstrap "gui/$UID" "$LAUNCH_AGENT" || {
  sleep 1
  launchctl bootstrap "gui/$UID" "$LAUNCH_AGENT"
}
launchctl kickstart -k "gui/$UID/$BUNDLE_ID"

echo "Installed $APP_NAME"
echo "App: $APP_DIR"
echo "Startup item: $LAUNCH_AGENT"
