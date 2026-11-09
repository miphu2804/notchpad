### [2026-11-10 00:20 UTC+07:00] — [Integration] Linear browser authorization with PKCE

**Done:**
- Replaced the new-key form with browser OAuth, cancellable callback handling, Keychain token storage and serialized refresh. Disconnect prevents late writes and blocks reconnect until pending credential deletion finishes.
- Preserved existing personal-key reads, stable `linear:<identifier>` tasks and the direct Insights callback. Removed unused key-entry code and made config-escaping tests exercise the production helper.
- Documented the registered public client ID and exact loopback redirect. No client secret is embedded; an unconfigured Connect reports setup missing without opening a browser.

**Changed  files:**
- `src/linear.rs`, `src/settings_window.rs`, `macos/bundle.sh`, `README.md`, `BLOCKERS.md` — modified

**Flow explained:**
Settings starts a worker and polls its result on the main thread; PKCE/state-validated callbacks exchange tokens, then enable the existing sync. Refresh and disconnect use generation/transition guards to prevent stale credential writes. Verified: 110 tests pass (including callback rejection, timeout/cancel, PKCE, token rotation and disconnect/reconnect gating), release build without warnings, shell syntax and diff checks. Actual scratch-HOME Connect action stays disconnected with the explicit configuration error; `/tmp/notchpad-oauth/unconfigured.png` inspected. Temporary hooks removed. Real provider authorization remains unverified and blocked on registration; see `BLOCKERS.md`.

### [2026-11-10 00:12 UTC+07:00] — [Fix] Preserve text height in the compact focus card

**Done:**
- The ring now shrinks before text rows. Its minimum allows the full phase, task and daily-total labels in the real 330 pt notch, while taller views retain the larger ring.

**Changed  files:**
- `src/controller.rs` — modified

**Flow explained:**
The preferred ring size previously competed with text compression, producing phase/count label heights of 8/7 pt instead of their 14 pt intrinsic heights and a zero-height task row. Lowering the ring preference to 200 and its minimum to 120 yields a 123 pt ring and restores every text row to its intrinsic height. Verified: reproduced before/after on the actual expanded notch, asserted all seven label heights, inspected `/tmp/notchpad-clipping/before.png` and `after.png`, and ran 100 passing tests. Temporary measurement/render hooks removed.

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
