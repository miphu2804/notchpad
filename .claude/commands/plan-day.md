---
description: Plan a day (or part of one) into notchpad's Today card through the notchpad MCP
argument-hint: "<what to plan, e.g. my afternoon: call mom at 5pm, gym after work !high>"
---
Plan this into notchpad: $ARGUMENTS

1. Split the request into one task per item with a short title ("Call mom", "Gym").
2. For each, set only what the user said or clearly implied: `time` as 24-hour HH:MM
   ("5pm" -> "17:00"), `priority` from !high / !med / !low, `project` from a #tag or an obvious
   area (Work, Home, Errands), `minutes` from a duration ("45m", "1h"). Leave the rest empty.
   Use today's date unless the request names another day.
3. Call the notchpad `plan_day` tool once with all items. Do not call `add_task` per item.
4. Reply with the planned tasks in time order, one line each, in the form
   `5:00 PM · Call mom · Home`.
