# Notchpad

A small, native macOS app that lives in the notch. It holds today's tasks, a pomodoro timer and a terminal, and shows what's playing. Hover the notch (or press ⌥N) to open it.

[![Install for Mac](https://img.shields.io/badge/Install_for_Mac-Download-000000?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/miphu2804/notchpad/releases/latest/download/Notchpad.zip)

Apple Silicon, macOS 14 or later. It is written in Rust with AppKit, runs at 0% CPU while idle, and makes no network requests.

## Features

- **Workspace.** Today's tasks with quick add (`gym 6pm !high #health 45m tomorrow`), sorted by priority. Typing `!`, `#` or `/` shows completions for priorities, projects and words; ↑↓ choose and Tab takes one. ▶ makes a task the pomodoro's focus. Pick a priority from the coloured dots beside the input or by right-clicking a task. A deleted task can be brought back with Undo for a few seconds. A pomodoro card shows today's tomatoes, a stopwatch, +5 min and weekly focus stats.
- **Collapsed notch.** Shows album art with a moving waveform while music plays, or the focus countdown.
- **Playing.** Now playing from any app (Spotify, Music, YouTube in a browser), with playback controls.
- **Clips.** A short clipboard history.
- **Terminal.** A real shell in the notch.
- **Alerts.** When a focus session or a break ends, and 10 minutes before and at a timed task's time, the notch chimes and opens with the news. It does not take the keyboard from the app you're in, and it stays open until you hover it and leave. Timed tasks also get a system notification.
- **More.** A JSON backup export, a dashboard window, and today's tasks copied as a Markdown checklist. Right-click the header for these.

## Install

1. Download [Notchpad.zip](https://github.com/miphu2804/notchpad/releases/latest/download/Notchpad.zip), unzip it, and drag **Notchpad.app** into **Applications**.
2. Open Notchpad. The app is not notarized, so macOS blocks the first launch: open **System Settings › Privacy & Security** and click **Open Anyway**. Alternatively, run:

   ```bash
   xattr -dr com.apple.quarantine /Applications/Notchpad.app
   ```

3. To start Notchpad at login, turn on **Settings › General › Open at login**.

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
- **Inbox.** A task without a day sits in the Inbox, which the notch does not show. `add_task` puts tasks there unless given a day; `move_task` or `plan_day` brings them into today.
- **Conflict rule.** A task done on either side stays done. The `day` set in Notchpad wins over the service's date. The title comes from the service. Tasks are deduplicated by `ext`, so re-running never creates duplicates.
- **Run it.** Authorize the connectors in Claude Code (`/mcp`), then run `/sync-tasks` (`.claude/commands/sync-tasks.md`).
