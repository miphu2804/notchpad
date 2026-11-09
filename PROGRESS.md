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

### [2026-11-09 13:55 UTC+07:00] — [Feature] Smart quick-add and right-click task menu

**Done:**
Typing `gym 6pm !high #health 45m tomorrow` in any add-task field now fills the time (18:00), priority, project, estimate and day, and keeps "gym" as the title. Times: `17:30`, `5pm`, `5:30pm`, `9h`, `9h30` (a preceding "at"/"lúc" is dropped). Priority: `!high`/`!!!`, `!med`/`!!`, `!low`/`!`. Estimate: `30m`, `45min`, `~1.5h`. Day: `today`, `tomorrow`, `tmr`. Plain numbers stay in the title. Right-clicking a task in the Today card or the Week board offers Move to Inbox / Today / Tomorrow and Delete; deleting keeps the focus card on the same task.

**Changed  files:**
- `src/quick_add.rs` — created (`parse`, 7 unit tests)
- `src/controller.rs` — modified (`add_task` uses `quick_add::parse`; `task_menu`, `reschedule`, `deleteTask:`, `moveToInbox:`/`moveToToday:`/`moveToTomorrow:`)
- `src/main.rs` — modified (module)

**Flow explained:**
`add_task` picks the field's default day (Inbox empty, Tomorrow column, else today) and passes the text to `quick_add::parse`, which takes recognised tokens out of the title. Each rendered `ListRow` gets an `NSMenu` whose items carry the task index in their tag; a right-click on any part of the row climbs the responder chain to that menu. Verified with a temporary hook on a scratch HOME: the menu actions delete and reschedule, remap focus and save `tasks.txt`.

### [2026-11-09 13:20 UTC+07:00] — [Feature] Header in the bands beside the notch

**Done:**
The open panel no longer leaves an empty black strip under the hardware notch. The header now fills the menu-bar band: close sits at the far left, the Workspace/Week/Clips tabs sit right of the notch, and the cards start right below. The logo and app name were dropped from the panel (`src/logo.rs` deleted). The panel stays 720 pt wide (an 840 pt try covered menu-bar items). The panel also re-anchors when displays are plugged, unplugged or rearranged, and the header band and notch gap follow the new screen.

**Changed  files:**
- `src/controller.rs` — modified (header split into close button and tabs group, `header_band`, `notch_gap`, `screenParametersChanged:`)
- `src/notch_motion.rs` — modified (`set_rects` for display changes)
- `src/logo.rs` — deleted; `src/main.rs` — modified

**Flow explained:**
`build_ui` gives the header a band of `max(safe_top, 32)` at the top of the content. The tabs group has a required constraint keeping its leading edge at least half the notch width plus 8 pt right of the centre, because the panel is centred on the notch. `screenParametersChanged:` recomputes the notch rects, updates both constants and passes the new rects to `NotchMotion`.

### [2026-11-08 22:06 UTC+07:00] — [Feature] Clips: clipboard history in the notch

**Done:**
New Clips tab. Text, images and files you copy, new screenshots and screen recordings, and files dragged onto the notch hang as cards on a line, newest first, in a sideways-scrolling strip (12 kept). Clicking a card puts it back on the clipboard; dragging a card drops it into any app. Clear empties the list. Clips stay in memory only. Copies marked concealed or transient (password managers) are skipped. TIFF copies are stored as PNG, and previews are 240 pt thumbnails, so a full-screen screenshot doesn't stay decoded. Idle CPU stays at 0%, RSS 18 MB; the release binary grew from 390 KB to 423 KB.

**Changed  files:**
- `src/clips.rs` — created (`ClipHistory`: pasteboard and screenshot-folder polling, dedupe, copy back)
- `src/clip_card.rs` — created (`ClipCard`: click to copy, drag source)
- `src/panel.rs` — modified (`NotchShape` accepts file drags and reports `FileDrag` events)
- `src/controller.rs` — modified (`Tab` enum with Clips, `render_clips`, `file_drag`, 0.5 s `pollClips:` timer)
- `src/widgets.rs`, `src/task_row.rs` — modified (`snapshot` shared by both drag sources)
- `src/main.rs` — modified (modules)

**Flow explained:**
Every 0.5 s `pollClips:` compares the general pasteboard's `changeCount` and, only when the screenshot folder's mtime changed, lists that folder (`com.apple.screencapture location`, else Desktop) for new files tagged `kMDItemIsScreenCapture` or named like screenshots. A new clip equal to an existing one moves that one to the front, so a clicked card (re-copied) also jumps to the front on the next poll. Cards are rebuilt only when the history changes, so a refresh never tears down a card mid-drag. A file dragged over the notch opens it on Clips; dropping adds the files, and the panel closes once the pointer leaves.

### [2026-11-08 21:44 UTC+07:00] — [Feature] Notchpad.app bundle

**Done:**
`macos/bundle.sh` builds an ad-hoc signed `Notchpad.app` with the hexagon icon, and `--install` copies it to `/Applications`. It runs as an agent app: no Dock icon, no menu bar.

**Changed  files:**
- `macos/bundle.sh`, `macos/Info.plist`, `macos/AppIcon.icns` — created
- `README.md` — modified (install section)

**Flow explained:**
The script runs `cargo build --release`, lays out `Contents/{MacOS,Resources}` with both binaries, the plist and the icon, signs with `codesign --sign -`, then optionally replaces the copy in `/Applications`. The icon is the `LogoMark` view rendered at 824 pt on a 1024 pt canvas and converted with `iconutil`.

### [2026-11-08 21:33 UTC+07:00] — [Feature] Smooth animations: notch spring, tabs, rows, drops

**Done:**
The notch panel springs open (slight overshoot) and closed (no overshoot) instead of the old fixed `setFrame:display:animate:`. Content slides out from under the notch, fading and scaling (0.97 → 1) in once the frame is 55% open, and fades out before the frame closes, so it never squeezes. Tab switches crossfade. Task rows (Today card and Week columns) tint under the pointer, fade in where they land while other rows slide, and fade out before their gap closes. Pill buttons dim while pressed. Kanban columns ease their drop highlight in and out. Reduce Motion drops the spring, scale and row sliding but keeps short fades. Idle CPU stays 0% (the display link pauses at rest); RSS 54.8 → 55.1 MB; release binary 372 → 390 KB.

**Changed  files:**
- `src/notch_motion.rs` — created (`NotchMotion`, display-link spring for the panel frame and content fade)
- `src/list_row.rs` — created (`ListRow` hover tint and hairline; `snapshot`/`animate_changes` for row in/out)
- `src/pill_button.rs` — created (`PillButton`, press dim)
- `src/controller.rs` — modified (`set_expanded` delegates to `NotchMotion`, hover check counts the expanded frame while opening, tab crossfade, rows wrapped in `ListRow`, Today separators became row hairlines, drop lookup goes through `ListRow`)
- `src/drop_column.rs` — modified (highlight is a fading layer behind the content)
- `src/widgets.rs` — modified (`fade`, `reduce_motion`, `pill` builds a `PillButton`)
- `src/main.rs` — modified (modules)

**Flow explained:**
Hover still calls `set_expanded`, which now only sets the target on `NotchMotion`. A `CADisplayLink` on the panel wakes the spring and steps it each frame: it sets the window frame between the collapsed and expanded rects, keeps the content centred and pinned to the bottom edge, and sets the content's alpha and scale. When the frame and fade are at rest, the link pauses. Exit events are still checked against the real pointer, and while open the full expanded rect counts, so an exit during the spring polls `recheckHover:` rather than closing. Rows are rebuilt on every render as before. `list_row::snapshot` records the old rows by task text first, and `animate_changes` then puts the removed rows back as inert ghosts that fade out and then hide inside an implicit-animation group, and fades new rows in at their final frames while the stack animates the rest of the layout.

### [2026-11-08 21:20 UTC+07:00] — [Feature] Let Claude plan it

**Done:**
Tasks gained optional `time` (HH:MM), `priority` (low/medium/high), `project` and `minutes`, stored as columns 5-8 of `tasks.txt`; 3- and 4-column lines still load, and trailing empty columns are not written. The MCP server has a new `plan_day` tool (several tasks, one atomic save, upsert by `ext` or by same open title on the same day) and `update_task`; `add_task` and `list_tasks` carry the new fields, and day/time/priority are validated. Today and Week rows show a dim line such as `5:00 PM · High · Home · 45 min`; Today's open tasks sort by time, untimed after. Lists now count two-line rows as 1.5 rows so they never overflow the card. `/plan-day` command added.

**Changed  files:**
- `src/store.rs` — modified (planning fields, line format, `Default`)
- `src/bin/notchpad-mcp.rs` — modified (`plan_day`, `update_task`, field validation)
- `src/controller.rs` — modified (`with_meta`, `meta_line`, `rows_that_fit`, Today time sort)
- `.claude/commands/plan-day.md` — created

**Flow explained:**
"plan my afternoon: call mom at 5pm, gym after work !high" → Claude splits it and calls `plan_day` once → the server validates every item, upserts them in memory and saves `tasks.txt` once (any invalid item aborts with nothing written) → the app sees the new mtime within 2s and re-renders, with timed tasks first in the Today card.

### [2026-11-08 21:11 UTC+07:00] — [Feature] Hexagon logo; Week kanban drag-and-drop

**Done:**
Header owl replaced by a vector logo (two interlinked rounded-hexagon rings, violet and amber, on a lavender tile). Week rows can be dragged onto any column: the drop reschedules the task (Inbox = no day, Today, Tomorrow) and places it above the row under the pointer. Project is now under git (`main`, Conventional Commits, one branch per feature).

**Changed  files:**
- `src/logo.rs` — created (`LogoMark` view)
- `src/task_row.rs` — created (`TaskRow`, drag source)
- `src/drop_column.rs` — created (`DropColumn`, drop target)
- `src/controller.rs` — modified (logo in header, `drop_task`, `save`, `redraw:`)
- `src/main.rs` — modified (modules)

**Flow explained:**
Dragging a row puts its store index on the pasteboard. The column under the pointer highlights, and on drop calls `Controller::drop_task`, which moves the task in the list, sets its day, keeps the focus index pointing at the same task, saves, and re-renders on the next run-loop pass so the drag source is not torn down mid-session.
\n