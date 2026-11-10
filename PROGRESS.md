### [2026-11-10 10:15 UTC+07:00] — [Fix] Bar shape drawn as a smaller notch

**Done:** The Bar shape was a flat rectangle with square top corners, which looked out of place beside the hardware notch. It now uses the notch silhouette (concave ears, rounded bottom corners) at the smaller Bar size (160×24).

**Changed  files:**
- `src/panel.rs` — modified: one outline for every shape; `bar` look and `bar_radius` removed
- `src/notch_banner.rs`, `src/controller.rs` — modified: `set_look(glass)`

**Flow explained:** `Shape::is_bar` still picks the size in `notch_rects`; `NotchShape` draws the same outline at whatever size its window has.

### [2026-11-10 10:11 UTC+07:00] — [Feature] Plan the rest of today

**Done:** Header menu item that gives today's untimed open tasks a start time in the free gaps until 22:00, high priority first, around calendar events and timed tasks.

**Changed  files:**
- `src/day_plan.rs` — created: the deterministic planner and its tests
- `src/calendar.rs` — modified: `Event::span` in minutes since midnight
- `src/controller.rs`, `src/main.rs`, `README.md` — modified: menu action, module, usage line

**Flow explained:** `planDay:` collects event spans as busy time, `day_plan::plan` ranks tasks and places each in the earliest gap from the next quarter hour, then the tasks are saved and the notch refreshes.

### [2026-11-10 10:03 UTC+07:00] — [Fix] Notch stays still during Desktop swipes

**Done:**
- The notch panel and banner are `Stationary` and `Transient` together: the drawn notch no longer slides off the hardware notch on a Desktop swipe, and Mission Control still hides it (confirmed on the user's Mac).

**Changed  files:**
- `src/controller.rs`, `src/notch_banner.rs` — modified

**Flow explained:**
- `Transient` alone (set to hide the panel in Mission Control) let the window move with the Space transition; adding `Stationary` pins it while keeping the Mission Control hiding.

### [2026-11-10 09:49 UTC+07:00] — [Docs] README layout

**Done:**
- README opens with Workspace and Insights screenshots (demo data), a contents line, and a Features table by section.
- Integrations start with a summary table; Reminders now describes instant sync, last-edit-wins and `#tag` projects. Release links point to `miphu2804/notchpad`.

**Changed  files:**
- `README.md` — modified
- `assets/screenshots/workspace.png`, `assets/screenshots/insights.png` — created

**Flow explained:**
- Screenshots are window-only captures (`screencapture -o -l`) of a probe build with a scratch HOME.

### [2026-11-10 09:45 UTC+07:00] — [UI] Stat notes clear of charts, larger Playing tab, instant Reminders sync

**Done:**
- Insights stat cards: fixed ring card width, slimmer week bars and a wrapping note, so "this week · N tasks" no longer runs under the bars.
- Playing: 220 pt artwork sized to the image's aspect (16:9 stream thumbnails have no bars), 24 pt title, larger controls.
- Merged `feat/reminders-sync-gaps`: projects travel as a `#tag` in the notes, the side changed last wins, and `EKEventStoreChangedNotification` triggers a pass.

**Changed  files:**
- `src/insights_view.rs`, `src/week_bars.rs`, `src/controller.rs`, `src/reminders*.rs` — modified

**Flow explained:**
- `render_music` sets the artwork's width and height constraints from the image size inside a 220 pt box.

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
