//! Task model and persistence in `~/Library/Application Support/notchpad/tasks.txt`.
//!
//! One task per line: `done<TAB>day<TAB>text[<TAB>ext[<TAB>time<TAB>priority<TAB>project<TAB>minutes]]`,
//! where `done` is `0`/`1`, `day` is `YYYY-MM-DD` or empty for the Inbox, and the optional `ext`
//! is the stable id of the task in an external service (`notion:<page>`, `linear:<ENG-1>`,
//! `gcal:<event>`). The planning columns are `time` (`HH:MM`, 24-hour), `priority` (`low`,
//! `medium`, `high`), `project` (short label) and `minutes` (estimated focus length), followed by
//! `done_on` (`YYYY-MM-DD` the task was completed). Every
//! optional column may be empty, and trailing empty columns are not written, so lines from
//! before `ext` (three columns) or before the planning columns (four) still parse.
//!
//! Shared by the app and the `notchpad-mcp` binary (which includes this file by path).

use std::{fs, io, path::PathBuf, time::SystemTime};

#[derive(Default)]
pub struct Task {
    pub text: String,
    pub done: bool,
    /// `YYYY-MM-DD`, or empty when the task sits in the Inbox.
    pub day: String,
    /// External id used to dedupe synced tasks; empty for local-only tasks.
    pub ext: String,
    /// `HH:MM` (24-hour) when the task is scheduled at a time of day, else empty.
    pub time: String,
    /// `low`, `medium` or `high`; empty means no priority.
    pub priority: String,
    /// Short project or area label such as `Work` or `Home`; may be empty.
    pub project: String,
    /// Estimated focus length in minutes; 0 when unknown.
    pub minutes: u32,
    /// `YYYY-MM-DD` the task was marked done; empty when not done or completed before this was recorded.
    pub done_on: String,
}

impl Task {
    /// Marks the task done or not done, keeping `done_on` in step. Re-marking a done task keeps its date.
    pub fn set_done(&mut self, done: bool) {
        if done && !self.done {
            self.done_on = crate::days::key(0);
        } else if !done {
            self.done_on.clear();
        }
        self.done = done;
    }
}

pub fn path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library/Application Support/notchpad/tasks.txt")
}

pub fn load() -> Vec<Task> {
    let Ok(contents) = fs::read_to_string(path()) else { return Vec::new() };
    contents.lines().filter(|l| !l.trim().is_empty()).map(parse).collect()
}

/// The scratch note lives beside `tasks.txt`.
pub fn notes_path() -> PathBuf {
    path().with_file_name("notes.txt")
}

pub fn load_notes() -> String {
    fs::read_to_string(notes_path()).unwrap_or_default()
}

pub fn save_notes(text: &str) -> io::Result<()> {
    let path = notes_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)
}

/// Last-modified time of the task file, used to notice writes by other processes.
pub fn modified() -> Option<SystemTime> {
    fs::metadata(path()).and_then(|m| m.modified()).ok()
}

/// Writes via a temp file and rename so a concurrent reader never sees a half-written file.
pub fn save(tasks: &[Task]) -> io::Result<()> {
    let path = path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let lines: Vec<String> = tasks
        .iter()
        .map(|t| {
            let minutes = if t.minutes == 0 { String::new() } else { t.minutes.to_string() };
            let done = if t.done { "1" } else { "0" };
            let mut cols = vec![done, &t.day, &t.text, &t.ext, &t.time, &t.priority, &t.project, &minutes, &t.done_on];
            while cols.len() > 3 && cols.last() == Some(&"") {
                cols.pop();
            }
            cols.join("\t")
        })
        .collect();
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, lines.join("\n"))?;
    fs::rename(tmp, path)
}

fn parse(line: &str) -> Task {
    let cols: Vec<&str> = line.split('\t').collect();
    if cols.len() < 3 {
        // A line without tabs (hand-edited file) becomes an Inbox task.
        return Task { text: line.to_owned(), ..Task::default() };
    }
    let col = |i: usize| cols.get(i).copied().unwrap_or("").to_owned();
    Task {
        done: cols[0] == "1",
        day: col(1),
        text: col(2),
        ext: col(3),
        time: col(4),
        priority: col(5),
        project: col(6),
        minutes: col(7).parse().unwrap_or(0),
        done_on: col(8),
    }
}
