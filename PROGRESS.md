### [2026-11-10 00:06 UTC+07:00] — [UI] Pomodoro ring and four pixel tomatoes

**Done:**
- Large elapsed-progress ring around the timer, four unchanged pixel sprites for the current cycle, separate daily count and focus minutes. The current tomato ripens green → orange → red; completed cycles stay fully red during the long break.
- The ring uses the existing controller timer, changes to green on breaks, and stays neutral in the unbounded stopwatch. Its preferred 164 pt size fits the compact card.

**Changed  files:**
- `src/focus_ring.rs` — created
- `src/controller.rs`, `src/main.rs`, `src/tomato_view.rs`, `README.md` — modified

**Flow explained:**
`render_focus` updates the ring from elapsed/session time and builds four cycle states; daily focus minutes are read with the existing log refresh, not every tick. Verified: 100 tests pass, cycle rollover regression test, scratch-HOME AppKit Start/tick/Pause/+5/End assertions, and real-window renders for idle, quarter, half, near-end, long break, stopwatch and tall. Screenshots inspected under `/tmp/notchpad-pomo/shots/`; temporary hooks removed.

### [2026-11-09 23:47 UTC+07:00] — [Fix] Direct entry points for Insights

**Done:**
- Settings › Sections now includes an Insights row with Open, and the notch header menu includes Open Insights. Both open the dashboard and select Insights directly; Settings closes out of the way.

**Changed  files:**
- `src/controller.rs`, `src/settings_window.rs`, `README.md` — modified

**Flow explained:**
The Settings window receives an `on_insights` callback backed by a weak controller reference. Both entry points use `show_insights_dashboard`, which opens the host window before selecting the dashboard-only tab. Verified: 99 tests pass; a temporary scratch-HOME hook clicked the actual Settings Open button and invoked the header action, asserting dashboard visibility and the Insights tab. Real window screenshots checked at `/tmp/notchpad-entry/shots/`; hooks removed.

### [2026-11-09 23:09 UTC+07:00] — [UI] Rounded Insights petals and stacked Pulse

**Done:**
- Rounded, pastel petals replace thin month-ring strokes; idle days remain dots. Pulse uses full-height columns split by contribution, with faint columns on idle days. The week strip remains in Insights, not the pomodoro card.
- Chart contribution is a daily estimate: pomodoro minutes are capped at session count × 25; remaining focus minutes are deep focus, and each task weighs 10 equivalent minutes. The tooltip explains this; it does not claim measured task time.

**Changed  files:**
- `src/insights.rs`, `src/insights_chart.rs`, `src/insights_view.rs`, `src/week_bars.rs`, `README.md` — modified

**Flow explained:**
`Day::effort` builds contribution weights, `shares` normalizes Pulse segments, and `dominant` selects petal colour. Charts still redraw only when their plot changes; no timer was added. Verified: 80 tests pass and real-window screenshots inspected with populated, empty and one-task scratch HOME fixtures under `/tmp/notchpad-finish/insights/`. Temporary rendering hooks removed. Idle CPU not yet remeasured.

### [2026-11-09 23:55 UTC+07:00] — [Refactor] Week strip moved from the focus card to Insights

**Done:**
- The pomodoro card no longer shows the week bars, "15m · 4 tasks" and the streak line; its column stays centred.
- Insights' Focus time card now says "this week · N tasks"; streak and week bars were already there.
- Removed `pomodoros::Week`, `week()` and the controller's `week` state; today's count comes from `Insights::build`.

**Changed  files:**
- `src/controller.rs`, `src/pomodoros.rs`, `src/insights_view.rs`, `README.md` — modified

**Flow explained:**
`reload_done_today` builds `Insights` from the log on each refresh and keeps today's pomodoro count for the goal row and break messages. Verified with 80 tests (the five `pomodoros` week tests went with the code; `insights` tests cover the same parsing) and renders of Workspace and Insights.

### [2026-11-09 23:10 UTC+07:00] — [Feature] Insights page in the dashboard

**Done:**
- Insights tab in the dashboard window: stat cards (active days ring, streak, focus time, best day), a five-week heatmap with Focus/Pomodoros/Tasks tabs, month rings, 30-day pulse with Export PNG (save panel), moon calendar, and an empty state.
- Pure `insights` module with per-day aggregates, streaks, best day and intensity levels, 14 unit tests (empty data, month-crossing streak, garbage lines).

**Changed  files:**
- `src/insights.rs`, `src/insights_chart.rs`, `src/insights_view.rs` — created
- `src/controller.rs`, `src/widgets.rs`, `src/main.rs`, `README.md` — modified

**Flow explained:**
`refresh()` calls `render_insights()` only while the Insights tab is visible; it builds `Insights` from the pomodoro log and tasks and hands it to `InsightsView`. The heatmap and rings cover the last 36 days (`SPAN`, six week columns) instead of a year; the palette is purple (focus), green (tasks), orange (pomodoros), cream moons. `Chart` draws each plot with NSBezierPath and skips the redraw when the plot is unchanged, so there are no new timers. Verified with 85 tests, offscreen renders from a scratch HOME (60 and 300 days, empty), and 0.0% idle CPU.

### [2026-11-09 22:58 UTC+07:00] — [UI] Vendor icons in Integrations

**Done:**
- Embedded the official Notion, Linear and Google Calendar PNG icons from their vendors; calendar marks sit side by side, without tint or redrawing. Asset sources are in `assets/integrations/README.md`.
- Verified: 89 tests pass; real Settings window screenshot inspected at `/tmp/notchpad-finish/logos/vendor-icons.png`. Both Add buttons dispatched through AppKit in a scratch HOME and changed to Added; Claude Code's CLI was stubbed, while Desktop wrote its real JSON format in the scratch directory. Temporary render/action hooks removed.
- Still unverified: real Claude configuration, Calendar permission/events, and Linear sync with a real key.

### [2026-11-09 22:16 UTC+07:00] — [Feature] Settings › Integrations: Claude, calendars, Linear

**Done:**
- New Integrations pane (`puzzlepiece.extension`) in three groups, one commit each.
- Claude: **Add** for Claude Code (`claude mcp add --scope user`, `claude` found on PATH, `~/.claude/local`, `~/.local/bin`, Homebrew) and for Claude Desktop (JSON merge with `.bak`; invalid JSON is refused). Status reads `~/.claude.json` and the Desktop config.
- Calendars: `calendar=1` setting; EventKit read of today's events, shown read-only above the tasks. Access is requested only when the switch is turned on.
- Linear: key in the Keychain (`notchpad.linear`), `linear=1` flag in settings, `curl` with the config on stdin, import every 10 min on the existing 2 s tick (no new timer), completion calls `issueUpdate`.

**Changed  files:**
- `src/claude_mcp.rs`, `src/calendar.rs`, `src/linear.rs` — created
- `src/settings_window.rs`, `src/settings.rs`, `src/controller.rs`, `src/days.rs`, `src/main.rs`, `Cargo.toml`, `Cargo.lock`, `macos/Info.plist`, `README.md` — modified

**Flow explained:**
- Switch on → `apply_settings` asks EventKit once, then `reloadEvents:` reads today's events (also on `EKEventStoreChanged` and at midnight); `render_task_card` prepends up to 3 event rows.
- The reload tick starts a Linear fetch on a thread when due; the thread sends the parsed issues over a channel and pings `linearSynced:`, which dedupes by `ext` and saves. Ticking a `linear:` task done spawns a thread that looks up the team's completed state and updates the issue.
- Verified: 89 tests (JSON merge, command args, claude lookup, GraphQL parsing, dedupe, curl request against a loopback server), pane rendered with a scratch HOME, 0.0% idle CPU. Not verified: a real Linear sync, a real calendar prompt and events, clicking Add.

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
