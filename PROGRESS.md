### [2026-11-09 23:10 UTC+07:00] — [Feature] Insights page in the dashboard

**Done:**
- Insights tab in the dashboard window: stat cards (active days ring, streak, focus time, best day), a five-week heatmap with Focus/Pomodoros/Tasks tabs, month rings, 30-day pulse with Export PNG (save panel), moon calendar, and an empty state.
- Pure `insights` module with per-day aggregates, streaks, best day and intensity levels, 14 unit tests (empty data, month-crossing streak, garbage lines).

**Changed  files:**
- `src/insights.rs`, `src/insights_chart.rs`, `src/insights_view.rs` — created
- `src/controller.rs`, `src/widgets.rs`, `src/main.rs`, `README.md` — modified

**Flow explained:**
`refresh()` calls `render_insights()` only while the Insights tab is visible; it builds `Insights` from the pomodoro log and tasks and hands it to `InsightsView`. The heatmap and rings cover the last 36 days (`SPAN`, six week columns) instead of a year; the palette is purple (focus), green (tasks), orange (pomodoros), cream moons. `Chart` draws each plot with NSBezierPath and skips the redraw when the plot is unchanged, so there are no new timers. Verified with 85 tests, offscreen renders from a scratch HOME (60 and 300 days, empty), and 0.0% idle CPU.

### [2026-11-09 21:55 UTC+07:00] — [Feature] Notch alert cards, focus settings, quick-add hints

**Done:**
- Alerts now drop a small card out of the notch (`NotchBanner`, its own non-activating panel) instead of opening the whole panel. Task reminders carry Snooze 5 min and Done; pomodoro and break ends show for 8 s.
- Settings › Focus: focus length, short/long break, reminder lead (off, at the time, 5/10/15 min) and chime on/off, saved in `settings.txt`.
- Typing `!`, `#` or `/` in the add-task field shows completion pills (↑↓, Tab/Return, Esc). ▶ has a tooltip.

**Changed  files:**
- `src/notch_banner.rs` — created
- `src/controller.rs`, `src/settings.rs`, `src/settings_window.rs`, `src/task_alerts.rs`, `src/quick_add.rs`, `src/main.rs`, `README.md` — modified

**Flow explained:**
- The 2 s reload tick calls `remind_tasks`, which collects `reminders_between(lead)` plus snoozed reminders due, then `notify` rings the chime (if enabled) and shows the card under the collapsed notch; `bannerExpired:` hides it after 60 s (8 s for sessions).
- Snooze stores the reminder with now + 5 min; Done marks the matching task done and saves.

### [2026-11-09 21:35 UTC+07:00] — [Feature] Notch alerts, task reminders, undo delete, open at login

**Done:**
- Pomodoro and break endings and timed tasks (10 min before, and at the time) chime and open the notch on Workspace without taking keyboard focus. The message stays on the card until the panel closes; before, a pomodoro ending focused the task input.
- Deleting a task shows "Deleted …" with Undo for 6 s; Undo puts it back where it was.
- Header menu › Copy today as Markdown: a checklist in the quick-add syntax.
- Settings › General › Open at login (`SMAppService`).

**Changed  files:**
- `src/controller.rs`, `src/task_alerts.rs`, `src/chime.rs`, `src/settings_window.rs`, `Cargo.toml`, `Cargo.lock`, `README.md` — modified

**Flow explained:**
The 2 s `reload:` tick calls `remind_tasks`, which works only when the minute changes: `task_alerts::reminders_between` returns today's open timed tasks whose reminder minute fell since the last check, at most 5 min back. `alert` rings the chime and pins the panel open with `orderFrontRegardless`, not as key window. Collapsing clears the toasts. Verified by hook logs (reminder opens the panel with `key=false`; delete, Undo and collapse states), renders of the reminder, Undo and General pane, 64 tests, and 0.0% idle CPU.

### [2026-11-09 20:55 UTC+07:00] — [Feature] Task priorities, clear completed; Week and Notes removed

**Done:**
- Today's tasks sort by priority (high, medium, low, none), then by time. Right-click › Priority sets it. The coloured dots beside the input pick the priority of new tasks, and a typed `!high` still wins.
- Check circles take the priority colour; the priority word in the metadata line is tinted. Completed tasks are struck through.
- Completed › Clear takes today's completed tasks off the list. Their `done_on` stays, so the weekly count is unchanged.
- The Week and Notes tabs, the calendar and reminders reading and the drag-between-days columns are gone. `notes.txt` stays on disk and in backups. Tasks without a day (Inbox) stay in `tasks.txt` and are reached through the MCP server.
- Header icon pills are 24 pt like the text pills (`alignmentRectInsets` override). Clips is an icon.

**Changed  files:**
- `src/controller.rs`, `src/settings.rs`, `src/store.rs`, `src/list_row.rs`, `src/pill_button.rs`, `src/main.rs` — modified
- `src/calendar.rs`, `src/drop_column.rs`, `src/note_view.rs`, `src/task_row.rs` — deleted
- `Cargo.toml`, `macos/Info.plist`, `README.md` — modified

**Flow explained:**
`render_task_card` sorts open tasks by `(priority_rank, untimed, time)`. `setPriority:` (menu) and `pickPriority:` (dots) write the priority or the default for new tasks; `push_task` applies it when the typed line names none. `clearCompleted:` empties the `day` of today's done tasks. Verified by renders of the sort, tint, dots and strike, and by logs of the header tabs and the task menu; 59 tests pass.

### [2026-11-09 20:30 UTC+07:00] — [Feature] Phase-end bell and toast, Linear-like palette, smooth section resize

**Done:**
- When a focus or break phase ends, a short synthesised two-note bell plays and the task line shows a toast for 6 s, e.g. "Pomodoro 1 done · short break 5 min". This replaces the Glass system sound.
- The cards use one dark surface (24,25,27) with raised controls (40,41,45). The tasks card is no longer sage.
- The Music section is now "Playing", with the `play.circle` icon.
- Switching between a compact and an extra-tall section no longer strands a ghost of the old tab or sinks the header. The content grows with the panel each frame. The terminal grid is resized only once the panel settles.

**Changed  files:**
- `src/chime.rs` — created
- `src/main.rs` — modified
- `src/controller.rs` — modified
- `src/notch_motion.rs` — modified
- `src/terminal_view.rs` — modified
- `src/settings.rs`, `src/settings_window.rs`, `README.md` — modified

**Flow explained:**
`tick:` at the deadline → `announce(chime, toast)` → `Chime::sound()` builds a WAV in memory for `NSSound` → `render_focus` shows the toast until `toastExpired:`. For a resize, `NotchMotion::apply` sizes the content to the spring height before it sets the panel frame. Only the new body crossfades. `setFrameSize:` on the terminal debounces `fitGrid` by 0.12 s. Verified with frame captures of both directions and 73 passing tests.

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
