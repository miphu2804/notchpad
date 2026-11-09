### [2026-11-09 19:45 UTC+07:00] — [Feature] Current pomodoro tomato ripens as the session runs

**Done:**
- The tomato for the running focus session blends from green through orange to red with the time elapsed, instead of jumping from green to red at the end. The stopwatch has no end, so its tomato stays green.

**Changed  files:**
- `src/tomato_view.rs` — modified
- `src/controller.rs` — modified

**Flow explained:**
`render_focus` (every timer tick) → ripeness = 1 − remaining ÷ session length → `Tomato::Growing(ripeness)` → `ripen` blends body, highlight and shade between the green, orange and red stops. Verified by a scratch render at 0, ¼, ½, ¾ and 1.

### [2026-11-09 19:35 UTC+07:00] — [Fix] Signed builds keep permissions; block glyphs fill the cell

**Done:**
- `bundle.sh` signs with the first Apple Development identity in the keychain (ad-hoc when there is none). The designated requirement is now the identifier plus the certificate instead of the cdhash, so macOS keeps granted privacy permissions across rebuilds.
- The terminal draws block elements (U+2580–U+259F) as rectangles snapped to the cell instead of font glyphs, which leave gaps in a cell taller than the font. Claude Code's logo now renders solid.

**Changed  files:**
- `macos/bundle.sh` — modified
- `src/terminal_view.rs` — modified

**Flow explained:**
`paint` → `block_shape(ch)` gives cell-fraction rects and an alpha (shades) → filled in the foreground colour, edges rounded to whole points so neighbouring cells meet. Verified by a scratch render of the logo and every block group.

### [2026-11-09 19:25 UTC+07:00] — [Fix] Terminal wheel scrolls programs that track the mouse

**Done:**
- When a program has turned on mouse tracking (e.g. Claude Code full screen, vim with `mouse=a`), the scroll wheel sends it wheel reports at the cell under the pointer, in SGR form when asked, else the legacy form. Before, the alternate screen always got arrow keys, which only walked the prompt history. Programs without mouse tracking still get arrows, and the normal screen still scrolls the scrollback.

**Changed  files:**
- `src/terminal_view.rs` — modified

**Flow explained:**
`scrollWheel:` → `scroll_by` turns the delta into whole lines → with `MOUSE_MODE`, `pointer_cell` + `wheel_report` (button 64 up / 65 down) per line; otherwise the arrow keys or scrollback path as before. Verified by a run against a probe program: mouse mode got `ESC[<64;41;1M` ×2 and `ESC[<65;41;1M`, plain alternate screen got `ESC[A` ×6 and `ESC[B` ×3.

### [2026-11-09 18:35 UTC+07:00] — [Feature] Per-section panel size and collapse on Space change

**Done:**
- Each section has a size in Settings › Sections: Compact, Tall (1.6×) or Extra tall (2.5×). It is clamped to the screen's visible height minus 40 pt. Switching tabs springs the panel to that tab's height, and the content stays level with the panel top while it moves. The terminal is told its new size once per switch.
- Switching Spaces or full-screen apps folds the expanded panel into the notch. A latch stops a pointer resting on the notch from reopening it until the pointer leaves and comes back.

**Changed  files:**
- `src/settings.rs` — modified
- `src/settings_window.rs` — modified
- `src/controller.rs` — modified
- `src/notch_motion.rs` — modified

**Flow explained:**
`render_tabs` calls `fit_panel`, which computes `body_height(size, visible_height)` and hands the new rects to `NotchMotion::set_rects`. `apply` offsets the content by the height still to travel. `activeSpaceChanged:` comes from the NSWorkspace notification centre: it sets `space_collapsed` and collapses the panel. `mouseExited:` clears the latch.

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

