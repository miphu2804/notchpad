### [2026-11-10 00:17 UTC+07:00] — [Integration] Real Linear OAuth acceptance

**Status:** Open

**Blocked by:** A registered public Linear OAuth client ID, with exact redirect URI `http://127.0.0.1:53682/oauth/callback`, has not been supplied.

**Impact:** The PKCE, callback/state validation, cancellation and refresh/disconnect synchronization are locally tested. This does not verify Linear's real authorization or token exchange. The unconfigured build reports the missing setup instead of starting a browser flow. Existing connected personal keys remain supported.

**Next action:** Register a public application in Linear, supply its public client ID (not a secret), rebuild with `NOTCHPAD_LINEAR_CLIENT_ID`, then authorize and verify assigned-issue import/completion with the user's consent.

### [2026-11-09 15:24 UTC+07:00] — [Feature] Live acceptance of the new features

**Status:** Open (narrowed 2026-11-09 18:37 UTC+07:00)

**Blocked by:**
- Testing rules: tests must not trigger the Calendar, Reminders or Notifications permission prompts, must not read or overwrite the user's clipboard, and must not switch the user's Spaces.

**Impact:**
- Calendar and Reminders are verified only with injected events.
- Notifications are verified only through the pure `due_alerts` output.
- ⌘C is verified only by the selection logic.
- Space-collapse is verified only by posting the notification.
- Already accepted with a scratch-HOME hook on 2026-11-09:
  - the header menu items and ⌘,
  - the Settings switches: hide, refuse the last one, wing, size, `settings.txt`
  - the dashboard window: open, close, content returned to the notch
  - the export JSON: tasks, notes, pomodoros

**Next action:** the user:
1. Clicks "Show calendar events" / "Show reminders" in Week, grants access, and checks that real items appear and that ticking one completes it in Reminders.app.
2. Adds `test <time 2 min ahead>` and checks that the notification fires.
3. Selects text in Terminal, presses ⌘C, and pastes it.
4. Right-clicks the header → Export backup and saves through the real save panel.
5. Swipes between Spaces with the panel open and checks that it folds into the notch.
