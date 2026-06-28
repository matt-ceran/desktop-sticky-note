# desktop sticky-note

A tiny native macOS sticky note app written in Rust.

## What it does

- Shows editable sticky notes on the desktop layer.
- Lets you create more notes from the menu bar icon.
- Keeps separate sticky note layouts for each macOS desktop/Space.
- Lets you change the selected note's note color, font color, font, and font size.
- Lets you resize notes by dragging their edges or corners.
- Turns typed dates like `06/10/2026` into readable dates and schedules reminders.
- Turns typed times like `9:30pm` or `09:30am` into readable times and schedules reminders.
- Supports multiple reminder lines in one note, so a sticky note can work as a reminder list.
- Saves note text, size, and position automatically.
- Starts on login through a user LaunchAgent and restarts after abnormal exits.
- Keeps data in `~/Library/Application Support/Desktop Sticky Note/notes.json`.

## Use

Click the `Sticky` item in the macOS menu bar.

- `New Note` creates another sticky note.
- New notes are created on the desktop/Space for the monitor where you opened the menu.
- `Note Color`, `Text Color`, `Font`, and `Font Size` apply to the note you last clicked or typed in.
- Choosing the black note color automatically switches the text to white.
- Type a date as `MM/DD/YYYY` to schedule that day; date-only reminders fire at 9:00 AM.
- Type a time as `H:MMam`, `HH:MMam`, `H:MMpm`, or `HH:MMpm` to schedule that time. Time-only reminders use today if the time is still ahead, otherwise tomorrow.
- Put a date and one or more times on the same line to schedule those exact date/time reminders.
- Put additional dates or times on separate lines to schedule multiple reminders in the same sticky note.
- Reminder notifications fire with sound 30 minutes before, 10 minutes before, 5 minutes before, and at the scheduled time.
- If macOS notification permission is denied, reminders show a native alert with sound instead.

## Install

```sh
./scripts/install.sh
```

The installed app lives at `~/Applications/Desktop Sticky Note.app`.

Existing saved notes that predate per-desktop layouts are assigned to the current desktop the first time this version runs.

## Remove

```sh
./scripts/uninstall.sh
```

Uninstalling removes the app and startup item, but leaves saved notes in place.
