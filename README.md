# desktop sticky-note

A tiny native macOS sticky note app written in Rust.

## What it does

- Shows editable sticky notes on the desktop layer.
- Lets you create more notes from the menu bar icon.
- Lets you change the selected note's note color, font color, font, and font size.
- Lets you resize notes by dragging their edges or corners.
- Saves note text, size, and position automatically.
- Starts on login through a user LaunchAgent and restarts after abnormal exits.
- Keeps data in `~/Library/Application Support/Desktop Sticky Note/notes.json`.

## Use

Click the `Sticky` item in the macOS menu bar.

- `New Note` creates another sticky note.
- `Note Color`, `Text Color`, `Font`, and `Font Size` apply to the note you last clicked or typed in.
- Choosing the black note color automatically switches the text to white.

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
