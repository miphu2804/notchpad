### [2026-11-09 18:25 UTC+07:00] — [Feature] Settings window, live waveform, countdown wing, install README

**Done:**
- Settings window (⌘, or the header menu): show or hide each section, and turn the collapsed-notch music and focus wings on or off. Saved in `settings.txt`.
- The collapsed notch shows white animated bars while music plays. It is pure Core Animation, so it adds no CPU when idle and stops when Reduce Motion is on.
- The countdown wing is widened to 50 pt and its digits are right-aligned. Measured clearance from the hardware notch is 7.2 pt.
- The focus card's tomatoes sit in one row of 22 pt.
- The two always-on timers get tolerance so their wakeups can be coalesced.
- README rewritten with an Install for Mac button that points to the latest release zip.

**Changed  files:**
- `src/settings.rs` — created
- `src/settings_window.rs` — created
- `src/live_activity.rs` — modified
- `src/controller.rs` — modified
- `src/panel.rs` — modified
- `README.md` — modified
- `.gitignore` — modified

**Flow explained:**
`Settings::load()` runs at startup. `render_tabs` hides the pills of hidden sections and falls back to the first visible tab. `sync_activity` reads the wing flags. `SettingsWindow` saves each change and calls back into the controller, which applies it live. `LiveActivity::animate_bars` only adds or removes the CA animation when the shown state changes.

### [2026-11-09 15:18 UTC+07:00] — [Feature] Task alerts, timer +5/stopwatch, JSON export, dashboard window

**Done:**
Closed the remaining feature gaps, one Sonnet thread per feature, each accepted and merged with `--no-ff`. Tasks with a day and time get a local notification at that time; the next 7 days are synced after each save, and authorization is asked on the first timed save, only in the bundled app. Focus timer: +5 min (also while paused), and a stopwatch duration that counts up and logs its minutes on End. Completed tasks are stamped with a `done_on` day, which is an optional 9th column so old files still load. The insights strip shows tasks done this week. The header's right-click menu holds "Open dashboard window", which moves the same content view into a resizable window, and "Export backup (JSON)…", which writes tasks, notes and the pomodoro log through a save panel. Closing the window or pressing the header ✕ returns the view to the notch. Verified with 57 unit tests and a hook that opened the dashboard, pressed ✕ and expanded the notch: the window closed, the app kept running, and the notch PNG matched the original. Idle CPU 0.0% after the install.

**Changed  files:**
- `src/task_alerts.rs` — created
- `src/export.rs` — created
- `src/controller.rs` — modified
- `src/pomodoros.rs` — modified
- `src/store.rs` — modified
- `src/bin/notchpad-mcp.rs` — modified
- `src/main.rs` — modified
- `src/notch_motion.rs` — modified
- `Cargo.toml`, `Cargo.lock` — modified

**Flow explained:**
- Save tasks → `task_alerts::sync` turns the pure `due_alerts` into `UNCalendarNotificationTrigger` requests with stable FNV ids.
- Timer: the `STOPWATCH` index runs `run_stopwatch` from `since`; `next_duration` and `end_log_minutes` stay pure.
- Header menu → `openDashboard:` → `show_dashboard`, which reparents `ui.content`. Closing calls `windowWillClose:` → `hide_dashboard` → `attach_to_notch`.
- `exportBackup:` → `NSSavePanel` → `export::write`.

### [2026-11-09 14:56 UTC+07:00] — [Feature] Reminders in the Week tab and focus insights

**Done:**
Week tab: incomplete Apple Reminders due today or overdue sit in Today, those due tomorrow in Tomorrow, below the calendar events — a hollow circle in the list colour (click completes it), the title, and the due time (soft red when overdue); at most 3 then "+N more". Access is asked only from a "Show reminders" pill in Inbox. Workspace focus card: an insights strip with 7 daily bars of completed pomodoros (today coral), weekly focus minutes and the day streak; the card's spacing and countdown size were tightened to fit. Both built by Sonnet threads; accepted by expanded-panel PNGs (motion stepped by hand while the screen was locked) and unit tests; the merged main was rendered again. Idle CPU 0.0%.

**Changed  files:**
- `src/calendar.rs` — modified (reminders fetch/complete, pure `place`/`reminder_rows`)
- `src/week_bars.rs` — created
- `src/pomodoros.rs` — modified (`week`, `week_of`, `streak`; `done_today`/`count_done` removed)
- `src/days.rs` — modified (`initial`)
- `src/controller.rs`, `src/main.rs`, `Cargo.toml`, `macos/Info.plist` — modified

**Flow explained:**
`reload_events` also starts an async `fetchRemindersMatchingPredicate`; its completion converts to plain `Reminder`s off the main thread, hops to the main queue and posts `NotchpadRemindersFetched`, whose observer stores them and re-renders the Week tab. `refresh` reads `pomodoros.txt` once into `Week { done[7], minutes, streak }`; the count chip uses `done[6]` and `WeekBars` draws the array.

### [2026-11-09 14:50 UTC+07:00] — [Feature] Calendar events in the Week tab

**Done:**
The Today and Tomorrow columns list that day's calendar events above the tasks: a dot in the calendar's colour, `HH:mm` or "All day", and a one-line title, at most 4 then "+N more". Read-only through EventKit. Access is never requested at launch: while undetermined, the Inbox column shows a "Show calendar events" pill that asks; denied shows nothing. Events re-fetch on launch, tab switch, expand and `EKEventStoreChangedNotification` — no polling, idle CPU 0.0%. Built by a Sonnet thread; accepted by fake-event state logs and unit tests (the real fetch and prompt need the bundled app and the user's click).

**Changed  files:**
- `src/calendar.rs` — created
- `src/controller.rs` — modified
- `src/main.rs` — modified
- `src/notch_motion.rs` — modified
- `Cargo.toml`, `Cargo.lock` — modified (`objc2-event-kit`, `block2`)
- `macos/Info.plist` — modified (`NSCalendarsFullAccessUsageDescription`)

**Flow explained:**
`reload_events` asks `Calendar::events` (empty unless access is FullAccess) for today and tomorrow, splitting multi-day events per day, then `render_week` builds rows from the pure `day_rows` (all-day first, by start time, capped). The pill's click calls `requestFullAccessToEventsWithCompletion`, whose handler posts the change notification so the main-queue observer re-fetches.

### [2026-11-09 14:43 UTC+07:00] — [Feature] Live activity wings and terminal selection

**Done:**
The collapsed notch grows two 34 pt wings while something is live: the playing track's artwork on the left with a coral waveform on the right, or the focus/break countdown on the right (coral/green). They spring in and out with the notch motion and hide when expanded. The now-playing helper now starts at launch so the wings work without opening the Music tab; it blocks on notifications, idle CPU 0.0%. The Terminal tab gained mouse selection (drag, double-click word, triple-click line, Shift under mouse reporting) and ⌘C. Both built by Sonnet threads; accepted by state logs and collapsed PNGs (screen was locked, so the open animation and a real drag were not exercised). Vietnamese IME input to the PTY verified by the shell receiving `echo tiếng Việt`.

**Changed  files:**
- `src/live_activity.rs` — created
- `src/notch_motion.rs` — modified
- `src/controller.rs` — modified
- `src/main.rs` — modified
- `src/notch_motion.rs` — modified
- `src/terminal_view.rs` — modified
- `src/terminal_session.rs` — modified

**Flow explained:**
`sync_activity` runs on every focus render (the existing session tick), each now-playing change and each expand/collapse; it fills the wing view and tells `NotchMotion::set_wings` the extra width, which springs the collapsed frame wider. Terminal mouse events map to grid cells via the pure `cell_at`, update alacritty's `Selection`, and the draw pass highlights the selected cells; ⌘C writes `selection_to_string()` to the pasteboard.

### [2026-11-09 14:35 UTC+07:00] — [Feature] Real PTY terminal tab

**Done:**
The Terminal tab is now a real terminal: the user's login shell in a PTY, emulated by `alacritty_terminal`, so `claude`, vim and htop run as in a full terminal. Colors (16/256/truecolor), alternate screen, block cursor, bold/dim/inverse/underline, scrollback on the wheel, Ctrl/Option/arrow/function keys, ⌘V with bracketed paste, and an `NSTextInputClient` for IME input. The shell starts the first time the tab opens and survives collapse; Return restarts it after exit. Replaces the old command runner (`shell.rs`). Built by a Sonnet thread; accepted by renders of colored output, typed input, vi full-screen, and `claude --version`. Idle CPU 0.0%; footprint 28.2 MB before the shell starts, about 41 MB with it running.

**Changed  files:**
- `src/terminal_view.rs` — created
- `src/terminal_session.rs` — created
- `src/terminal_keys.rs` — created
- `src/terminal_palette.rs` — created
- `src/shell.rs` — deleted
- `src/controller.rs` — modified
- `Cargo.toml`, `Cargo.lock` — modified

**Flow explained:**
`TerminalSession` spawns the PTY and alacritty's event loop; its listener hops to the main queue via `dispatch2` to mark `TerminalView` dirty, so nothing polls. `TerminalView` draws the grid per row, maps keys through `terminal_keys`, and resizes the PTY to its bounds. While the view is first responder the panel stays open.

### [2026-11-09 14:05 UTC+07:00] — [Feature] System-wide Now Playing, pomodoro chime, terminal Escape fix

**Done:**
The Music tab now follows whatever macOS shows as Now Playing (YouTube in Chrome, Music, Spotify, …) with title, artist, artwork, position and prev/play-pause/next. macOS 15.4+ blocks MediaRemote for unentitled apps, so a small helper dylib runs inside Apple-signed `/usr/bin/perl` and streams JSON lines. Built by a Sonnet thread; accepted by a render with live YouTube data. Pomodoro end now plays the Glass chime instead of the alert beep and still opens the notch (checked with a 1 s session). The terminal prompt stays enabled while a command runs so Escape reaches it and kills the command (checked with synthetic key events). Idle CPU 0.0%, footprint 28.9 MB.

**Changed  files:**
- `build.rs` — created
- `macos/now_playing_helper.m` — created
- `src/now_playing.rs` — modified
- `src/controller.rs` — modified

**Flow explained:**
`build.rs` compiles the helper with clang; `now_playing.rs` embeds it, writes it to the app's support folder on first use of the Music tab, and runs `perl` with it. The helper registers for MediaRemote notifications and prints a JSON line per change; a reader thread queues parsed tracks and pokes the controller on the main thread, which re-renders. Controls are words written to the helper's stdin; it exits when stdin closes.

### [2026-11-09 13:48 UTC+07:00] — [Feature] Pomodoro tomatoes and Terminal tab

**Done:**
The focus card is now "Today's pomodoros": 8 pixel tomatoes (ripe red when done, green while running, grey ahead), an "N / 8" chip, a "Focus · n of 4" or "Break · short/long" badge, the countdown with "ends at", and Start/Pause/Resume plus End. Sessions log to `pomodoros.txt` (`YYYY-MM-DD HH:MM <min> done|ended`); breaks are 5 min, or 15 after the 4th focus. Clicking the idle countdown cycles the length, since the timer tile was dropped at the user's request. A new "Terminal" tab runs `zsh -lc` commands in the background with streamed output, stderr in red, `[exit N]`, in-process `cd`, ↑↓ history, Escape to kill, Clear and Open in Terminal. Built by two Sonnet threads; accepted by renders of both tabs on a scratch HOME. Idle CPU 0.0%, footprint 24.9 MB.

**Changed  files:**
- `src/tomato_view.rs` — created
- `src/pomodoros.rs` — created
- `src/shell.rs` — created
- `src/controller.rs` — modified
- `src/days.rs` — modified
- `src/main.rs` — modified
- `src/notch_motion.rs` — modified

**Flow explained:**
The focus timer ticks only while running; on finish it appends a `done` line and starts a break, and End appends `ended`. The count re-reads the log on each refresh. Terminal: Return calls `run_command`, which handles `cd` and the interactive-program hint in `Shell::submit`, or spawns the child in its own process group; two reader threads send chunks over mpsc, and a 0.1 s timer that exists only while a command runs drains them into the output view, trimmed to 2000 lines.

### [2026-11-09 14:45 UTC+07:00] — [Feature] Notes tab, and header tabs on both sides of the notch

**Done:**
A Notes tab (note icon) holds a free-text notepad that saves to `notes.txt` half a second after typing stops and whenever the panel closes. ⌘↩ turns the line under the cursor into a task for today, parsed by quick-add (`call mom 6pm` becomes "call mom" at 18:00), removes the line from the note and flashes "Added to Today". With five tabs, the header now splits them across the notch: Workspace and Week left (after ✕), Clips, ♪ and Notes right. Built by the np-notes thread. Accepted by renders of the Notes and Workspace tabs and by a scripted ⌘↩ on a scratch HOME.

**Changed  files:**
- `src/note_view.rs` — created (`NoteView` text view with ⌘↩, `take_line`, tests)
- `src/store.rs` — modified (`load_notes`, `save_notes`)
- `src/controller.rs` — modified (Notes tab; `push_task` shared by add-task fields and ⌘↩; `Header` with left and right tab groups and two `notch_gaps` constraints)
- `src/main.rs` — modified (module)

**Flow explained:**
`NoteView` reports edits and ⌘↩ to the controller. Edits reschedule a debounced `saveNotes:`; ⌘↩ calls `note_line_to_task`, which splits the text with `take_line` and passes the line to `push_task` (quick-add, save, refresh). In the header, one constraint keeps the left group's trailing edge clear of the notch and another keeps the right group's leading edge clear; `screenParametersChanged:` updates both.

### [2026-11-09 14:20 UTC+07:00] — [Feature] Now Playing music tab

**Done:**
A ♪ tab shows what Apple Music or Spotify is playing: artwork (Music cover via AppleScript, else the app icon), title, artist · album, a progress bar with elapsed/total, and ⏮ ⏯ ⏭ controls that send AppleScript to the player. With nothing playing it says "Nothing playing. Start Music or Spotify." The 1 s progress timer runs only while the panel is open on the Music tab and a track plays. The private MediaRemote framework isn't used. Built by the np-music thread; accepted by renders of the empty and playing states, plus a real `com.apple.Music.playerInfo` notification posted from another process, which updated the tab.

**Changed  files:**
- `src/now_playing.rs` — created (`NowPlaying`, `parse`, `info_map`, AppleScript `send`, 4 tests)
- `src/controller.rs` — modified (Music tab, `musicChanged:`/`spotifyChanged:` observers, `render_music`, progress timer)
- `macos/Info.plist` — modified (`NSAppleEventsUsageDescription`)
- `src/main.rs` — modified (module)

**Flow explained:**
`NSDistributedNotificationCenter` delivers the players' state notifications. `info_map` turns userInfo into strings, `parse` builds a `Track` (Stopped means none), and `apply` keeps the artwork when the track is unchanged. AppleScript runs only when the player app is already running, so a closed app is never launched.
