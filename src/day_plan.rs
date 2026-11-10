//! "Plan the rest of today": gives today's open tasks without a time a start time in the free
//! gaps between now and the evening, most important first. Local and deterministic; Claude's
//! `plan_day` tool is the assistant-driven alternative.

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

/// Start times for the tasks that fit, as (index into `tasks`, `HH:MM`). `busy` holds calendar
/// spans in minutes since midnight; today's open timed tasks count as busy too. Tasks are taken
/// by priority (high, medium, none, low), in list order within one priority, and each gets the
/// earliest gap it fits, so a short task can fill a gap a longer one skipped. A task that fits
/// nowhere keeps no time.
pub fn plan(tasks: &[Task], today: &str, now: u32, busy: &[(u32, u32)]) -> Vec<(usize, String)> {
    let open_today = |t: &Task| !t.done && t.day == today;
    let mut busy = busy.to_vec();
    busy.extend(tasks.iter().filter(|t| open_today(t)).filter_map(|t| {
        let start = minutes(&t.time)?;
        Some((start, start + length(t)))
    }));
    let mut todo: Vec<usize> = (0..tasks.len()).filter(|&i| open_today(&tasks[i]) && tasks[i].time.is_empty()).collect();
    todo.sort_by_key(|&i| rank(&tasks[i].priority));

    let from = now.div_ceil(STEP) * STEP;
    let mut planned = Vec::new();
    for i in todo {
        let need = length(&tasks[i]);
        if let Some(start) = earliest_gap(&busy, from, need) {
            busy.push((start, start + need + BREAK));
            planned.push((i, format!("{:02}:{:02}", start / 60, start % 60)));
        }
    }
    planned
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

    fn times(tasks: &[Task], now: u32, busy: &[(u32, u32)]) -> Vec<(String, String)> {
        let mut out: Vec<_> = plan(tasks, TODAY, now, busy).into_iter().map(|(i, t)| (tasks[i].text.clone(), t)).collect();
        out.sort_by(|a, b| a.1.cmp(&b.1));
        out
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn high_priority_goes_first_from_the_next_quarter_hour() {
        let tasks = [task("low", "low", 30, ""), task("none", "", 0, ""), task("high", "high", 60, "")];
        assert_eq!(times(&tasks, 9 * 60 + 2, &[]), pairs(&[("high", "09:15"), ("none", "10:20"), ("low", "10:55")]));
    }

    #[test]
    fn events_and_timed_tasks_are_skipped_and_short_tasks_fill_gaps() {
        let tasks = [task("long", "high", 90, ""), task("short", "medium", 20, ""), task("call", "", 30, "10:30")];
        // Free 9:00–10:30 is a 90-minute gap, so "long" takes it; "short" lands after the call.
        assert_eq!(times(&tasks, 9 * 60, &[(11 * 60, 12 * 60)]), pairs(&[("long", "09:00"), ("short", "12:00")]));
        let tasks = [task("long", "high", 120, ""), task("short", "medium", 20, "")];
        assert_eq!(times(&tasks, 9 * 60, &[(10 * 60, 11 * 60)]), pairs(&[("short", "09:00"), ("long", "11:00")]));
    }

    #[test]
    fn done_other_days_and_already_timed_tasks_are_left_alone() {
        let mut done = task("done", "high", 30, "");
        done.done = true;
        let mut tomorrow = task("tomorrow", "high", 30, "");
        tomorrow.day = "2026-11-11".into();
        let tasks = [done, tomorrow, task("timed", "high", 30, "15:00")];
        assert!(plan(&tasks, TODAY, 9 * 60, &[]).is_empty());
    }

    #[test]
    fn a_task_that_cannot_end_by_the_evening_keeps_no_time() {
        let tasks = [task("late", "", 60, ""), task("tiny", "", 15, "")];
        assert_eq!(times(&tasks, 21 * 60 + 20, &[]), pairs(&[("tiny", "21:30")]));
    }
}
