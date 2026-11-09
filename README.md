# Notchpad

A small, native macOS app that lives in the notch. It holds your tasks, notes, a pomodoro timer and a terminal, and shows what's playing. Hover the notch (or press ⌥N) to open it.

[![Install for Mac](https://img.shields.io/badge/Install_for_Mac-Download-000000?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/miphu2804/notchpad/releases/latest/download/Notchpad.zip)

Apple Silicon, macOS 14 or later. It is written in Rust with AppKit, runs at 0% CPU while idle, and makes no network requests.

## Features

- **Workspace.** Today's tasks with quick add (`gym 6pm !high #health 45m tomorrow`). A pomodoro card shows today's tomatoes, a stopwatch, +5 min and weekly focus stats.
- **Week.** Tasks, calendar events and reminders laid out by day. Calendar and Reminders are only accessed after you click to allow them.
- **Collapsed notch.** Shows album art with a moving waveform while music plays, or the focus countdown.
- **Music.** Now playing from any app (Spotify, Music, YouTube in a browser), with playback controls.
- **Clips.** A short clipboard history.
- **Notes.** Saved automatically. Turn a line into a task with ⌘↩.
- **Terminal.** A real shell in the notch.
- **Notifications.** Alerts for timed tasks, a JSON backup export, and a dashboard window. Right-click the header to open the dashboard or export.

## Install

1. Download [Notchpad.zip](https://github.com/miphu2804/notchpad/releases/latest/download/Notchpad.zip), unzip it, and drag **Notchpad.app** into **Applications**.
2. Open Notchpad. The app is not notarized, so macOS blocks the first launch: open **System Settings › Privacy & Security** and click **Open Anyway**. Alternatively, run:

   ```bash
   xattr -dr com.apple.quarantine /Applications/Notchpad.app
   ```

3. To start Notchpad at login, add it under **System Settings › General › Login Items**.

Quit with the ✕ in the notch. Data lives in `~/Library/Application Support/notchpad/` as plain text.

## Build from source

Building needs Rust and the Xcode command line tools.

```bash
./macos/bundle.sh --install
```

This builds `Notchpad.app` (ad-hoc signed) and copies it to `/Applications`. `cargo build --release` alone produces `notchpad` (the app) and `notchpad-mcp` (a stdio MCP server over the same task file).

## Task sync (Notion, Linear, Google Calendar)

Sync runs through Claude: Claude uses the service's MCP connector together with the `notchpad` MCP server. The app itself holds no tokens and has no network code.

- **Store.** `~/Library/Application Support/notchpad/tasks.txt`, one task per line, tab separated. `ext` is the stable external id (`linear:ENG-12`, `notion:<page-id>`, `gcal:<event-id>`). The app reloads the file within about 2 s of an external write.
- **Server.** Register it with `claude mcp add --scope user notchpad -- $PWD/target/release/notchpad-mcp`. Tools: `list_tasks`, `add_task` (upserts when `ext` is given), `complete_task`, `move_task`, `update_task`, `plan_day`.
- **Conflict rule.** A task done on either side stays done. The `day` set in Notchpad wins over the service's date. The title comes from the service. Tasks are deduplicated by `ext`, so re-running never creates duplicates.
- **Run it.** Authorize the connectors in Claude Code (`/mcp`), then run `/sync-tasks` (`.claude/commands/sync-tasks.md`).
