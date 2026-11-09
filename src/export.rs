//! JSON backup of the whole planner: tasks, notes and the pomodoro log.
//!
//! Clips are not exported because they live only in memory.

use std::{fs, io};

use serde_json::{Value, json};

use crate::{days, pomodoros, store};

/// Reads the planner from disk and writes the backup to `path`.
pub fn write(path: &str) -> io::Result<()> {
    let mut backup = build(&store::load(), &store::load_notes(), &pomodoros::load_log());
    backup["exported_at"] = json!(days::stamp());
    fs::write(path, serde_json::to_string_pretty(&backup)?)
}

/// `pomodoros_log` is the raw `pomodoros.txt` text; malformed lines are skipped.
fn build(tasks: &[store::Task], notes: &str, pomodoros_log: &str) -> Value {
    let tasks: Vec<Value> = tasks
        .iter()
        .map(|t| {
            json!({"text": t.text, "done": t.done, "done_on": t.done_on, "day": t.day, "time": t.time,
                   "priority": t.priority, "project": t.project, "minutes": t.minutes})
        })
        .collect();
    let notes: Vec<Value> = (!notes.trim().is_empty()).then(|| json!({"name": "scratch", "text": notes})).into_iter().collect();
    let sessions: Vec<Value> = pomodoros_log
        .lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let [day, time, minutes, state] = cols[..] else { return None };
            Some(json!({"at": format!("{day} {time}"), "minutes": minutes.parse::<u64>().ok()?, "done": state == "done"}))
        })
        .collect();
    json!({"version": 1, "tasks": tasks, "notes": notes, "pomodoros": sessions})
}

#[cfg(test)]
mod tests {
    use super::build;
    use crate::store::Task;

    #[test]
    fn builds_tasks_notes_and_sessions() {
        let task = Task { text: "Write".into(), done: true, done_on: "2026-11-09".into(), minutes: 25, ..Task::default() };
        let log = "2026-11-09 09:00 25 done\n2026-11-09 10:00 10 ended\nbad line\n";
        let v = build(&[task], "remember milk", log);
        assert_eq!(v["version"], 1);
        assert_eq!(v["tasks"][0]["text"], "Write");
        assert_eq!(v["tasks"][0]["done_on"], "2026-11-09");
        assert_eq!(v["tasks"][0]["minutes"], 25);
        assert_eq!(v["notes"][0]["text"], "remember milk");
        assert_eq!(v["pomodoros"].as_array().unwrap().len(), 2);
        assert_eq!(v["pomodoros"][0]["at"], "2026-11-09 09:00");
        assert_eq!(v["pomodoros"][1]["done"], false);
        assert_eq!(build(&[], " \n", "")["notes"].as_array().unwrap().len(), 0);
    }
}
