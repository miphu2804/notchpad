//! Session log in `pomodoros.txt`, beside `tasks.txt`.
//!
//! One line per finished session: `YYYY-MM-DD HH:MM <minutes> done|ended`. Only `done`
//! lines count toward the daily goal; `ended` ones record a session stopped early.

use std::{fs, io::Write, path::PathBuf};

use crate::store;

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

/// Completed sessions logged for `today` (`YYYY-MM-DD`).
pub fn done_today(today: &str) -> usize {
    count_done(&fs::read_to_string(path()).unwrap_or_default(), today)
}

fn count_done(log: &str, today: &str) -> usize {
    log.lines()
        .filter(|line| {
            let mut cols = line.split_whitespace();
            cols.next() == Some(today) && cols.nth(2) == Some("done")
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::count_done;

    #[test]
    fn counts_only_todays_done_lines() {
        let log = "2026-11-08 09:00 25 done\n\
                   2026-11-09 09:00 25 done\n\
                   2026-11-09 09:30 10 ended\n\
                   2026-11-09 10:00 25 done\n";
        assert_eq!(count_done(log, "2026-11-09"), 2);
        assert_eq!(count_done(log, "2026-11-08"), 1);
    }

    #[test]
    fn skips_malformed_and_blank_lines() {
        let log = "\ngarbage\n2026-11-09 09:00 done\n2026-11-09 09:00 25 done extra\n";
        assert_eq!(count_done(log, "2026-11-09"), 1);
        assert_eq!(count_done("", "2026-11-09"), 0);
    }
}
