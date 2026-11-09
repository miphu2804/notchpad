# Notchday

A small native macOS app that lives in the MacBook notch: today's tasks, a focus timer and quick capture, one hover away.

![Platform: macOS 14+](https://img.shields.io/badge/platform-macOS%2014%2B-lightgrey) [![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

## Features

- Today's tasks with a one-line quick add, sorted by priority, with undo for deletes.
- Focus timer (pomodoro) tied to a task, with a progress ring and daily totals.
- Alerts that drop out of the notch when a focus session or break ends, and before a timed task, with Snooze and Done.
- Insights dashboard: streaks and focus totals, month rings of completed tasks, and a moon calendar of focus days.
- Clips (clipboard history and fresh screenshots), a built-in terminal, and Now Playing with playback controls.
- Integrations: Claude (MCP), Calendar, Linear, Apple Reminders and Shortcuts. See [Integrations](#integrations).

## Requirements

- macOS 14 or later.
- A MacBook with a notch. On a display without one, Notchday draws its own notch at the top of the screen.
- To build from source: a Rust toolchain (edition 2024) and the Xcode command line tools.

## Install

**Release.** Download the zip from the [latest release](https://github.com/miphu2804/notchday/releases/latest), unzip it, and drag **Notchday.app** into **Applications**. The app is not notarized, so macOS blocks the first launch: open **System Settings › Privacy & Security** and click **Open Anyway**, or run:

```bash
xattr -dr com.apple.quarantine /Applications/Notchday.app
```

**From source.**

```bash
./macos/bundle.sh --install
```

This builds `Notchday.app` and replaces `/Applications/Notchday.app`. It is signed with your first Apple Development identity if one exists, otherwise ad hoc; ad-hoc builds make macOS ask for permissions again after each rebuild.

To start at login, turn on **Settings › General › Open at login**.

## Usage

Hover the notch, or press Option-N, to open it. Quit with the ✕ in the notch. Right-click the header for the JSON backup export, Insights and copying today's tasks as a Markdown checklist.

Quick add parses one line; recognised words set fields and are removed from the title:

| Syntax | Meaning |
|---|---|
| `5pm`, `17:30`, `5:30pm`, `9h30` | time |
| `!high` `!med` `!low` (or `!!!` `!!` `!`) | priority |
| `#project` | project |
| `30m`, `45min`, `~1h`, `~1.5h` | time estimate |
| `today`, `tomorrow`, `tmr` | day (default: today) |

Example: `gym 6pm !high #health 45m tomorrow`. Typing `!`, `#` or `/` shows completions; Up/Down choose and Tab accepts.

## Integrations

### Claude (Code and Desktop)

The bundled MCP server `notchpad-mcp` lets Claude read and manage your tasks (`list_tasks`, `add_task`, `complete_task`, `move_task`, `update_task`, `plan_day`). In **Settings › Integrations › Claude**, use the Add button for Claude Code (runs `claude mcp add --scope user`) or Claude Desktop (merges into `claude_desktop_config.json`, keeping a `.bak`; restart Claude Desktop afterwards). A config that is not valid JSON is never overwritten.

Tasks live in `~/Library/Application Support/notchpad/tasks.txt`, one tab-separated line per task; the app reloads it within about 2 seconds of an external write. Tasks added through MCP without a day go to the Inbox, which the notch does not show; `move_task` or `plan_day` brings them into today.

### Calendar

Turn on **Show calendar events** to see today's events, read-only, above your tasks. Events come from the macOS calendars (EventKit), so add Google or other accounts in System Settings › Internet Accounts. macOS asks for access only when you enable the switch; Notchday never writes to a calendar.

### Linear

Under **Settings › Integrations › Linear**, either:

- **Connect** with OAuth (browser authorization), or
- paste a personal API key from Linear › Settings › Security & access (shown when the build has no OAuth client ID).

Your open assigned issues become tasks on their due date (or today), refreshed when the app opens and every 10 minutes. Completing one in Notchday moves the issue to its team's completed state. Credentials are stored in the macOS Keychain.

### Apple Reminders

Opt-in: turn on **Sync tasks with Reminders** in **Settings › Integrations**. Tasks sync two-way with a Reminders list named "Notchday", so they reach iPhone and Apple Watch through iCloud.

- Title, day, time, priority and completion sync; Inbox tasks become reminders without a due date.
- Only local tasks sync; Linear tasks keep their own sync.
- Deleting an open task on either side deletes it on the other. Clearing completed reminders on the phone keeps the done tasks in Notchday's history.
- When both sides changed, Notchday's edit wins, except that a completion made on the phone is kept.
- macOS asks for Reminders access only when you turn the switch on. Changes on the phone arrive within about 30 seconds.

### Shortcuts and automation

`notchpad-mcp add` adds a task from the command line using the quick-add syntax. It goes to today unless the line names another day.

```bash
/Applications/Notchday.app/Contents/MacOS/notchpad-mcp add "Pay rent 5pm !high #home"
```

In the Shortcuts app, turn on **Settings › Advanced › Allow Running Scripts**, then add a **Run Shell Script** action with that command, and pass your text as an argument (or build the string with a Text action first). Combined with a personal automation, this turns incoming messages into tasks. On macOS Tahoe, create an automation triggered by **Email** or **Message**, filter on the sender or subject (for example, your bank's transaction emails), then run a shell script that adds a task such as `Review transaction #finance`. macOS has no generic "any notification" trigger, so automations are limited to the triggers Shortcuts offers.

## Privacy

Everything stays on your Mac, in `~/Library/Application Support/notchpad/` as plain text. Notchday makes network requests only to Linear, and only if you connect it. Calendar, Reminders and Music/Spotify access are requested from macOS only when the feature is used.

## Development

```bash
cargo build --release   # produces notchpad (app) and notchpad-mcp (MCP server)
cargo test
```

Layout of `src/`:

- `main.rs`, `controller.rs`, `panel.rs`: app entry, notch window and event handling.
- `store.rs`, `days.rs`, `quick_add.rs`: task file, day keys and the quick-add parser.
- `settings*.rs`, `insights*.rs`, `calendar.rs`, `linear.rs`, `claude_mcp.rs`: settings UI and integrations.
- `terminal_*.rs`, `now_playing.rs`, `clips.rs`: terminal, Now Playing and clips.
- `bin/notchpad-mcp.rs`: MCP server and the `add` command.

Internal identifiers keep the project's first name, so existing data and permissions carry over: the binaries `notchpad` and `notchpad-mcp`, the bundle ID `app.notchpad`, the data folder and the MCP server name `notchpad`.

To embed a Linear OAuth client ID, register a public application in Linear with the redirect URI `http://127.0.0.1:53682/oauth/callback` and build with `NOTCHPAD_LINEAR_CLIENT_ID='<client-id>' ./macos/bundle.sh --install`. Never embed a client secret.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE).
