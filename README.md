# desktop sticky-note

A tiny native macOS sticky note app written in Rust.

## What it does

- Shows editable sticky notes on the desktop layer.
- Lets you create more notes from the menu bar item.
- Saves note text, size, and position automatically.
- Starts on login through a user LaunchAgent.
- Keeps data in `~/Library/Application Support/Desktop Post-It/notes.json`.

## Install

```sh
./scripts/install.sh
```

The installed app lives at `~/Applications/Desktop Sticky Note.app`.

## Remove

```sh
./scripts/uninstall.sh
```

Uninstalling removes the app and startup item, but leaves saved notes in place.
