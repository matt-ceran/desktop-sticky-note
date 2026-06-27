#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Desktop Sticky Note"
BUNDLE_ID="com.mattceran.desktop-sticky-note"
APP_DIR="$HOME/Applications/$APP_NAME.app"
LAUNCH_AGENT="$HOME/Library/LaunchAgents/$BUNDLE_ID.plist"

launchctl bootout "gui/$UID/$BUNDLE_ID" >/dev/null 2>&1 || true
rm -f "$LAUNCH_AGENT"
rm -rf "$APP_DIR"

echo "Removed $APP_NAME"
echo "Notes were left in: $HOME/Library/Application Support/Desktop Sticky Note"
