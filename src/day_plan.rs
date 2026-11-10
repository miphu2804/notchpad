//! "Plan my day": rearranges what is left of today. Open tasks without a time, timed tasks whose
//! slot has passed and tasks overdue from earlier days get a start in the free gaps between now and
//! the evening, most important first; whatever does not fit moves to tomorrow. Local and
//! deterministic; Claude's `plan_day` tool is the assistant-driven alternative.

use crate::store::Task;
use crate::task_alerts::minutes;

/// Planning starts on the next quarter hour.
const STEP: u32 = 15;
/// Every planned task ends by this time, in minutes since midnight.
const DAY_END: u32 = 22 * 60;
/// Length assumed for a task without an estimate.
const DEFAULT_MINUTES: u32 = 30;
/// Breathing room kept after each planned task.
const BREAK: u32 = 5;

/// Where a planned task goes.
#[derive(Debug, PartialEq)]
pub enum Slot {
    /// Today at `HH:MM`.
    Today(String),
    /// Tomorrow, without a time, because nothing was free today.
    Tomorrow,
}

/// New slots for the tasks the plan moves, as (index into `tasks`, slot). `busy` holds calendar
/// spans in minutes since midnight; today's open timed tasks that have not ended count as busy too.
/// Tasks are taken by priority (high, medium, none, low), in list order within one priority, and
/// each gets the earliest gap it fits, so a short task can fill a gap a longer one skipped.
pub fn plan(tasks: &[Task], today: &str, now: u32, busy: &[(u32, u32)]) -> Vec<(usize, Slot)> {
    let span = |t: &Task| minutes(&t.time).map(|start| (start, start + length(t)));
    let open = |t: &Task| !t.done && !t.day.is_empty();
    let mut busy = busy.to_vec();
    busy.extend(tasks.iter().filter(|t| open(t) && t.day == today).filter_map(span).filter(|&(_, end)| end > now));
    let movable = |t: &Task| {
        let overdue = t.day.as_str() < today;
        let unplanned = t.day == today && span(t).is_none_or(|(_, end)| end <= now);
        open(t) && (overdue || unplanned)
    };
    let mut todo: Vec<usize> = (0..tasks.len()).filter(|&i| movable(&tasks[i])).collect();
    todo.sort_by_key(|&i| rank(&tasks[i].priority));

    let from = now.div_ceil(STEP) * STEP;
    todo.into_iter()
        .map(|i| {
            let need = length(&tasks[i]);
            let slot = match earliest_gap(&busy, from, need) {
                Some(start) => {
                    busy.push((start, start + need + BREAK));
                    Slot::Today(format!("{:02}:{:02}", start / 60, start % 60))
                }
                None => Slot::Tomorrow,
            };
            (i, slot)
        })
        .collect()
}

/// First start at or after `from`, on a five-minute mark, where `need` minutes overlap no busy span.
fn earliest_gap(busy: &[(u32, u32)], from: u32, need: u32) -> Option<u32> {
    let mut start = from;
    while start + need <= DAY_END {
        match busy.iter().filter(|&&(s, e)| s < start + need && start < e).map(|&(_, e)| e).max() {
            Some(end) => start = end.div_ceil(5) * 5,
            None => return Some(start),
        }
    }
    None
}

fn length(task: &Task) -> u32 {
    if task.minutes > 0 { task.minutes } else { DEFAULT_MINUTES }
}

fn rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 3,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-10";

    fn task(text: &str, priority: &str, minutes: u32, time: &str) -> Task {
        Task { text: text.into(), day: TODAY.into(), priority: priority.into(), minutes, time: time.into(), ..Task::default() }
    }

    fn slots(tasks: &[Task], now: u32, busy: &[(u32, u32)]) -> Vec<(String, Slot)> {
        let mut out: Vec<_> = plan(tasks, TODAY, now, busy).into_iter().map(|(i, s)| (tasks[i].text.clone(), s)).collect();
        out.sort_by(|a, b| format!("{:?}", a.1).cmp(&format!("{:?}", b.1)));
        out
    }

    fn at(list: &[(&str, &str)]) -> Vec<(String, Slot)> {
        list.iter().map(|(a, b)| (a.to_string(), Slot::Today(b.to_string()))).collect()
    }

    #[test]
    fn high_priority_goes_first_from_the_next_quarter_hour() {
        let tasks = [task("low", "low", 30, ""), task("none", "", 0, ""), task("high", "high", 60, "")];
        assert_eq!(slots(&tasks, 9 * 60 + 2, &[]), at(&[("high", "09:15"), ("none", "10:20"), ("low", "10:55")]));
    }

    #[test]
    fn events_and_timed_tasks_are_skipped_and_short_tasks_fill_gaps() {
        let tasks = [task("long", "high", 90, ""), task("short", "medium", 20, ""), task("call", "", 30, "10:30")];
        // Free 9:00–10:30 is a 90-minute gap, so "long" takes it; "short" lands after the call.
        assert_eq!(slots(&tasks, 9 * 60, &[(11 * 60, 12 * 60)]), at(&[("long", "09:00"), ("short", "12:00")]));
        let tasks = [task("long", "high", 120, ""), task("short", "medium", 20, "")];
        assert_eq!(slots(&tasks, 9 * 60, &[(10 * 60, 11 * 60)]), at(&[("short", "09:00"), ("long", "11:00")]));
    }

    #[test]
    fn missed_and_overdue_tasks_are_planned_again_but_running_ones_stay() {
        let mut overdue = task("overdue", "", 30, "16:00");
        overdue.day = "2026-11-08".into();
        let tasks = [task("missed", "", 30, "08:00"), task("running", "", 60, "09:30"), overdue];
        assert_eq!(slots(&tasks, 10 * 60, &[]), at(&[("missed", "10:30"), ("overdue", "11:05")]));
    }

    #[test]
    fn done_inbox_future_and_upcoming_tasks_are_left_alone() {
        let mut done = task("done", "high", 30, "");
        done.done = true;
        let mut tomorrow = task("tomorrow", "high", 30, "");
        tomorrow.day = "2026-11-11".into();
        let mut inbox = task("inbox", "high", 30, "");
        inbox.day.clear();
        let tasks = [done, tomorrow, inbox, task("timed", "high", 30, "15:00")];
        assert!(plan(&tasks, TODAY, 9 * 60, &[]).is_empty());
    }

    #[test]
    fn what_cannot_end_by_the_evening_moves_to_tomorrow() {
        let tasks = [task("late", "", 60, ""), task("tiny", "", 15, "")];
        let mut want = at(&[("tiny", "21:30")]);
        want.push(("late".into(), Slot::Tomorrow));
        assert_eq!(slots(&tasks, 21 * 60 + 20, &[]), want);
    }
}
