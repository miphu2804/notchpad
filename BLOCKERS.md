### [2026-11-09 15:24 UTC+07:00] — [Feature] Live acceptance of the new features

**Status:** Open

**Blocked by:**
- The Mac's screen is locked (`ioreg` reports `CGSSessionScreenIsLocked = true`), so no real UI can be driven: hover, the live spring animation, the right-click header menu, the export `NSSavePanel`, and the dashboard window on a real display.
- Testing rules: tests must not trigger the Calendar, Reminders or Notifications permission prompts, and must not read or overwrite the user's clipboard. That excludes testing real EventKit fetch/complete, notification delivery and the ⌘C pasteboard write automatically. The current TCC grants can't be read without Full Disk Access.

**Impact:**
- Calendar and Reminders are verified only with injected events, plus unit tests on the pure placement code.
- Notifications are verified only through the pure `due_alerts` output.
- Everything else is verified with hand-stepped PNG renders and unit tests.

**Next action:**
After unlocking, the user:
1. Clicks "Show calendar" / "Show reminders" in the Week tab and grants access, then checks that real events and reminders appear and that ticking one completes it in Reminders.app.
2. Adds `test <time 2 min ahead>` and checks that the notification fires.
3. Right-clicks the header → Export backup and checks the saved JSON.
4. Selects text in the Terminal and presses ⌘C, then pastes it.
5. Opens the dashboard window and closes it with ✕.
