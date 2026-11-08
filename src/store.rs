//! Task model and persistence in `~/Library/Application Support/notchpad/tasks.txt`.
//!
//! One task per line: `done<TAB>day<TAB>text[<TAB>ext]`, where `done` is `0`/`1`, `day` is
//! `YYYY-MM-DD` or empty for the Inbox, and the optional `ext` is the stable id of the task in
//! an external service (`notion:<page>`, `linear:<ENG-1>`, `gcal:<event>`). Lines written
//! before `ext` existed have three columns and parse with an empty `ext`.
//!
//! Shared by the app and the `notchpad-mcp` binary (which includes this file by path).

use std::{fs, io, path::PathBuf, time::SystemTime};

pub struct Task {
    pub text: String,
    pub done: bool,
    /// `YYYY-MM-DD`, or empty when the task sits in the Inbox.
    pub day: String,
    /// External id used to dedupe synced tasks; empty for local-only tasks.
    pub ext: String,
}

pub fn path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library/Application Support/notchpad/tasks.txt")
}

pub fn load() -> Vec<Task> {
    let Ok(contents) = fs::read_to_string(path()) else { return Vec::new() };
    contents.lines().filter(|l| !l.trim().is_empty()).map(parse).collect()
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
            let line = format!("{}\t{}\t{}", u8::from(t.done), t.day, t.text);
            if t.ext.is_empty() { line } else { format!("{line}\t{}", t.ext) }
        })
        .collect();
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, lines.join("\n"))?;
    fs::rename(tmp, path)
}

fn parse(line: &str) -> Task {
    match line.splitn(4, '\t').collect::<Vec<_>>()[..] {
        [done, day, text, ext] => {
            Task { text: text.to_owned(), done: done == "1", day: day.to_owned(), ext: ext.to_owned() }
        }
        [done, day, text] => {
            Task { text: text.to_owned(), done: done == "1", day: day.to_owned(), ext: String::new() }
        }
        // A line without tabs (hand-edited file) becomes an Inbox task.
        _ => Task { text: line.to_owned(), done: false, day: String::new(), ext: String::new() },
    }
}
