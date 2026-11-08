# notchpad

Tiny native macOS notch task app (Rust, AppKit). `cargo build --release` produces two binaries:
`notchpad` (the app) and `notchpad-mcp` (a stdio MCP server over the same task file).

## Task sync (Notion, Linear, Google Calendar)

Sync is Claude-driven: Claude uses the service's MCP connector plus the `notchpad` MCP server.
The app has no tokens and no network code.

- **Store.** `~/Library/Application Support/notchpad/tasks.txt`, `done<TAB>day<TAB>text[<TAB>ext]`.
  `ext` is the stable external id (`linear:ENG-12`, `notion:<page-id>`, `gcal:<event-id>`);
  three-column lines from older versions still load. The app reloads the file within ~2s of an
  external write.
- **Server.** `claude mcp add --scope user notchpad -- $PWD/target/release/notchpad-mcp`.
  Tools: `list_tasks`, `add_task` (upsert when `ext` is given), `complete_task`, `move_task`.
- **Mapping.** Linear issues assigned to me, Notion database rows, and Calendar events for today
  and tomorrow each become a task keyed by `ext`; their date becomes `day`.
- **Conflict rule.** Done on either side wins; the `day` set in notchpad wins over the service's
  date; the title comes from the service. Dedupe is by `ext`, so re-running never duplicates.
- **Run it.** Authorize the connectors in Claude Code (`/mcp`), then run `/sync-tasks`
  (`.claude/commands/sync-tasks.md`). Schedule it if you want it periodic.
