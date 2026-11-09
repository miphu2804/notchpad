//! Session log in `pomodoros.txt`, beside `tasks.txt`.
//!
//! One line per finished session: `YYYY-MM-DD HH:MM <minutes> done|ended`. Only `done`
//! lines count toward the daily goal and the weekly insights; `ended` ones record a session stopped early.

use std::{fs, io::Write, path::PathBuf};

use crate::store::{self, Task};

pub const DAILY_GOAL: usize = 8;

fn path() -> PathBuf {
    store::path().with_file_name("pomodoros.txt")
}

/// Appends one session; `stamp` is `YYYY-MM-DD HH:MM`.
pub fn append(stamp: &str, minutes: u64, done: bool) {
    let result = (|| {
        let path = path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{stamp} {minutes} {}", if done { "done" } else { "ended" })
    })();
    if let Err(err) = result {
        eprintln!("notchpad: could not log pomodoro: {err}");
    }
}

/// Everything the insights strip shows, from one read of the log.
#[derive(Clone, Copy, Default)]
pub struct Week {
    /// Completed sessions per day, oldest first, today last.
    pub done: [u32; 7],
    /// Minutes focused in those seven days.
    pub minutes: u64,
    pub streak: u32,
    /// Tasks completed in those seven days.
    pub tasks_done: u32,
}

/// Insights for the seven days in `days` (`YYYY-MM-DD`, oldest first, today last).
pub fn week(days: &[String; 7], tasks: &[Task]) -> Week {
    Week { tasks_done: tasks_done(tasks, days), ..week_of(&load_log(), days) }
}

/// The raw log text, empty when no session has been logged yet.
pub fn load_log() -> String {
    fs::read_to_string(path()).unwrap_or_default()
}

fn tasks_done(tasks: &[Task], days: &[String; 7]) -> u32 {
    tasks.iter().filter(|t| t.done && days.contains(&t.done_on)).count() as u32
}

fn week_of(log: &str, days: &[String; 7]) -> Week {
    let mut done = [0u32; 7];
    let mut minutes = 0;
    for line in log.lines() {
        let mut cols = line.split_whitespace();
        let (Some(day), Some(_time), Some(mins), Some("done"), None) =
            (cols.next(), cols.next(), cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        if let Some(i) = days.iter().position(|d| d == day) {
            done[i] += 1;
            minutes += mins.parse().unwrap_or(0);
        }
    }
    Week { done, minutes, streak: streak(&done), ..Week::default() }
}

/// Consecutive days with a done session, ending today or, while today is empty, yesterday.
fn streak(done: &[u32; 7]) -> u32 {
    let end = if done[6] > 0 { 7 } else { 6 };
    done[..end].iter().rev().take_while(|&&n| n > 0).count() as u32
}

#[cfg(test)]
mod tests {
    use super::{streak, tasks_done, week_of};
    use crate::store::Task;

    fn days() -> [String; 7] {
        std::array::from_fn(|i| format!("2026-11-{:02}", 3 + i))
    }

    #[test]
    fn skips_malformed_and_blank_lines() {
        let log = "\ngarbage\n2026-11-09 09:00 done\n2026-11-09 09:00 25 done extra\n2026-11-09 09:00 25 done\n";
        assert_eq!(week_of(log, &days()).done[6], 1);
        assert_eq!(week_of("", &days()).done, [0; 7]);
    }

    #[test]
    fn week_counts_done_per_day_and_sums_minutes() {
        let log = "2026-11-02 09:00 25 done\n\
                   2026-11-03 09:00 25 done\n\
                   2026-11-03 10:00 45 done\n\
                   2026-11-04 09:00 10 ended\n\
                   2026-11-09 09:00 15 done\n\
                   2026-11-10 09:00 25 done\n\
                   bad line\n";
        let w = week_of(log, &days());
        assert_eq!(w.done, [2, 0, 0, 0, 0, 0, 1]);
        assert_eq!(w.minutes, 85);
        assert_eq!(w.streak, 1);
    }

    #[test]
    fn tasks_done_counts_completions_within_the_week() {
        let task = |done, done_on: &str| Task { done, done_on: done_on.to_owned(), ..Task::default() };
        let tasks = [
            task(true, "2026-11-03"),
            task(true, "2026-11-09"),
            task(true, "2026-11-02"), // before the window
            task(true, ""),           // done before dates were recorded
            task(false, "2026-11-05"),
        ];
        assert_eq!(tasks_done(&tasks, &days()), 2);
    }

    #[test]
    fn streak_counts_back_from_today() {
        assert_eq!(streak(&[0, 1, 0, 2, 1, 3, 1]), 4);
        assert_eq!(streak(&[1, 1, 1, 1, 1, 1, 1]), 7);
        assert_eq!(streak(&[0; 7]), 0);
    }

    #[test]
    fn streak_without_today_counts_from_yesterday() {
        assert_eq!(streak(&[0, 0, 1, 1, 1, 1, 0]), 4);
        assert_eq!(streak(&[1, 1, 1, 1, 1, 1, 0]), 6);
        assert_eq!(streak(&[1, 1, 0, 0, 0, 0, 0]), 0);
    }
}
