### [2026-11-10 09:36 UTC+07:00] — [Feature] Insights as a notch section

**Done:**
- Insights is a fifth notch section (chart icon after Terminal), shown by default and switchable in Settings › Sections; Settings' "Insights window" still opens it in the dashboard.
- The page fits the compact notch: month rings beside a 2×2 grid of the stat cards. The moon calendar and its intensity buckets are gone; the ring dropped its legend (now in the tooltip), names "today"/"start" at the top and uses the card's full height.

**Changed  files:**
- `src/settings.rs`, `src/controller.rs`, `src/settings_window.rs`, `src/insights.rs`, `src/insights_chart.rs`, `src/insights_view.rs` — modified

**Flow explained:**
- `Tab::Insights` indexes `SECTIONS[4]`, so hiding and sizing go through the same settings arrays as the other sections; the dashboard shows Insights even while the notch hides it.

### [2026-11-10 09:14 UTC+07:00] — [Fix] Hide in Mission Control, drop finished events

**Done:**
- The notch panel and banner are `Transient` instead of `Stationary`, so Mission Control hides them (no second notch over the shrunken desktops) while they still float on every Desktop.
- Today's events are fetched from now instead of midnight, so finished ones leave the task card; the list reloads as soon as a shown event ends.

**Changed  files:**
- `src/controller.rs`, `src/notch_banner.rs`, `src/calendar.rs` — modified

**Flow explained:**
EventKit returns events overlapping the range, so an event under way stays until its end; `reload:` sees `Event::ended` and refetches.

### [2026-11-10 02:01 UTC+07:00] — [Feature] NotchGo rename, notch shape and glass, dot-matrix countdown, scrolling title

**Done:**
- Renamed the app to NotchGo (bundle, Info.plist, README, Reminders list); internal ids stay `notchpad`. MIT license.
- Settings › Notch: Shape (Auto/Notch/Bar, a flat bar for Macs without a notch) and Background (Black/Liquid Glass, `NSGlassEffectView` on macOS 26, a dark blur before).
- Settings › Focus › Notch countdown: Plain or Dot matrix, `mm:ss` in 5×7 LED dots with a moving ripple beside the collapsed notch; the card keeps its ring.
- Music tab: a title or artist too long for its width scrolls (rest, one pass, seamless repeat) on a `CATextLayer`.
- Linear: Connect with an empty key field opens Linear's API-key page.

**Changed  files:**
- `src/dot_matrix.rs`, `src/marquee.rs` — created
- `src/ascii_timer.rs` — deleted
- `src/panel.rs`, `src/controller.rs`, `src/live_activity.rs`, `src/notch_banner.rs`, `src/settings.rs`, `src/settings_window.rs`, `src/linear.rs`, `README.md` — modified

**Flow explained:**
- The dot countdown runs a 10 fps timer only while a session runs, the panel is collapsed and the wing is on; each tick sets the opacity of 175 dot layers.
- The scrolling title is one Core Animation keyframe animation, so the app stays idle while it moves; the same text again never restarts it.

### [2026-11-10 01:19 UTC+07:00] — [Feature] Reminders sync, Shortcuts capture and OSS docs

**Done:**
- Tasks sync two-way with a Reminders list named "Notchpad" (opt-in switch; access asked only when turned on). Never deletes on a reset or a lost task file; clearing completed reminders keeps done tasks.
- `notchpad-mcp add "<quick-add line>"` adds a task, for Shortcuts Run Shell Script and Mail/Message automations.
- README restructured to a standard open-source layout; CONTRIBUTING.md and SECURITY.md added.

**Changed  files:**
- `src/reminders_sync.rs`, `src/reminders.rs` — created
- `src/controller.rs`, `src/settings.rs`, `src/settings_window.rs`, `src/calendar.rs`, `src/brand_logos.rs`, `src/main.rs`, `src/bin/notchpad-mcp.rs`, `Cargo.toml`, `macos/Info.plist` — modified
- `README.md` — modified; `CONTRIBUTING.md`, `SECURITY.md` — created

**Flow explained:**
- The 2 s reload tick starts a Reminders fetch when the switch is on, access is granted, and the task file changed or 30 s passed. The fetch completes on an EventKit queue; on the main thread `reminders_sync::reconcile` compares tasks, reminders and the last snapshot (`reminders.txt`) to decide pulls, pushes, deletes and new links, then `Reminders::apply` writes them and the snapshot is saved. A pass is skipped if `tasks.txt` changed on disk meanwhile.

### [2026-11-10 00:51 UTC+07:00] — [Fix] Verify the integrations, alerts and Insights pass

**Done:**
- Linear connects with a personal API key when no OAuth client ID is built in; the key is checked against Linear before it is stored.
- A timed task no longer gets both a system notification and the notch card in the same minute; the scheduler leaves the next minute to the card.
- Integration logos share one rounded tile; Claude Desktop shows the installed app's icon.
- Insights keeps the stat cards, month rings and moon calendar; the heatmap and the 30-day pulse with PNG export are gone.

**Changed  files:**
- `src/linear.rs`, `src/settings_window.rs`, `src/task_alerts.rs`, `src/controller.rs`, `src/brand_logos.rs` — modified
- `src/insights.rs`, `src/insights_chart.rs`, `src/insights_view.rs`, `README.md` — modified

**Flow explained:**
- `remind_tasks` withdraws pending system requests for tasks due next minute, and `task_alerts::sync` schedules from the minute after; the notch card covers the gap.

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

