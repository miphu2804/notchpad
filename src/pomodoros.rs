//! Session log in `pomodoros.txt`, beside `tasks.txt`.
//!
//! One line per finished session: `YYYY-MM-DD HH:MM <minutes> done|ended`. Only `done`
//! lines count toward the daily goal and the insights; `ended` ones record a session stopped early.

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

/// The raw log text, empty when no session has been logged yet.
pub fn load_log() -> String {
    fs::read_to_string(path()).unwrap_or_default()
}
