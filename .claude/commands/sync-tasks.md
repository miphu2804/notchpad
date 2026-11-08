---
description: Reconcile notchpad tasks with Notion, Linear and Google Calendar through MCP
---
Sync notchpad with the external services that are connected in this session. Skip any service
whose MCP tools are unavailable and say which were skipped. Never delete data in a service.

1. `list_tasks` (notchpad MCP) to get the current tasks and their `ext` ids.
2. Pull, using `add_task` with `ext` (it upserts):
   - Linear: open issues assigned to me -> `ext=linear:<identifier>`, text `<identifier> <title>`,
     day = due date, else "" (Inbox).
   - Notion: rows of my task database not marked done -> `ext=notion:<page-id>`, day = its date property.
   - Google Calendar: events for today and tomorrow -> `ext=gcal:<event-id>`, day = event date.
3. Push done state back: for each notchpad task with an `ext` whose `done` is true and whose
   service item is still open, mark it complete in that service (Linear: move to a done state;
   Notion: tick the status property). Calendar events are read-only.
4. Reconcile: a task done on either side stays done; the day set in notchpad wins over the
   service's date; title comes from the service.
5. Report counts per service: added, updated, pushed, skipped.
