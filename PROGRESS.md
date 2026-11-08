### [2026-11-08 21:05 UTC+07:00] — [Feature] MCP server and task-sync groundwork

**Done:**
`notchpad-mcp` stdio server (list_tasks, add_task upsert-by-ext, complete_task, move_task), registered in the user-scope Claude Code config. Store gained an optional 4th `ext` column (old lines still load) and atomic saves. The app now reloads `tasks.txt` on external change (2s timer). `/sync-tasks` command and README section describe the Claude-driven sync. Not yet run end-to-end: Notion and Linear need authorization, Google Calendar connector has no URL.

**Changed  files:**
- `src/bin/notchpad-mcp.rs` — created
- `src/store.rs` — modified (`ext`, `modified()`, atomic save)
- `src/controller.rs` — modified (`ext` on new tasks, `reload:` timer, `seen_mtime`)
- `Cargo.toml` — modified (serde_json, explicit bins)
- `.claude/commands/sync-tasks.md`, `README.md` — created

**Flow explained:**
Claude pulls items via a service connector and calls `add_task` with `ext`; the server rewrites `tasks.txt`; the app sees the new mtime and re-renders.

### [2026-11-08 20:45 UTC+07:00] — [Fix] Hover flicker at top edge; M4 Pro notch shape

**Done:**
Panel no longer closes/reopens while the pointer rests on the notch. Notch drawn with concave top ears and rounded bottom corners (MacBook Pro M4 Pro proportions; 185×32 default on displays without a notch).

**Changed  files:**
- `src/panel.rs` — modified (`NotchShape` view, `ear()` sizing)
- `src/controller.rs` — modified (`TOP_SLOP`, exit confirmation + recheck, ear-aware rects)

**Flow explained:**
The pointer at the top edge sits at the screen's max Y, outside a window ending there, so AppKit reported exits and the panel looped. The window now extends 1pt above the screen, and every exit is confirmed against `NSEvent::mouseLocation()`; a spurious exit starts a 0.25s recheck until the pointer truly leaves.

### [2026-11-08 20:32 UTC+07:00] — [Feature] Workspace and Week UI

**Done:**
Rebuilt the expanded notch panel to a new layout: header with Workspace/Week tabs, lavender Focus card (task title, timer, Start/✓, Set time), sage "Today's tasks" card (To do/Completed, circle rows, ▶ to focus, add field), and a Week tab with Inbox/Today/Tomorrow columns. Tasks now carry a done flag and a day.

**Changed  files:**
- `src/controller.rs` — modified (state, rendering, builders)
- `src/store.rs` — modified (`Task { text, done, day }`, tab-separated file)
- `src/widgets.rs` — created (view factories)
- `src/days.rs` — created (day keys/labels)
- `src/panel.rs` — modified (removed custom background view)
- `src/main.rs`, `Cargo.toml` — modified

**Flow explained:**
Hover on the notch (or Option-N) expands the panel; every action mutates `State`, saves `tasks.txt`, then `refresh()` re-renders both tabs from state. Open tasks dated today or earlier show under Today; empty day = Inbox.
