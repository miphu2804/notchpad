//! Two-way matching between Notchpad's tasks and the reminders of its own Reminders list, so the
//! tasks reach iPhone and Apple Watch through iCloud. Pure: [`crate::reminders`] fetches and
//! writes the reminders; this decides what changes.
//!
//! A task linked to a reminder carries `rem:<calendarItemIdentifier>` as its `ext`. Only local
//! tasks (no `ext`) that are not done get a reminder; tasks from Linear keep their own sync.
//! The snapshot holds each linked pair as it was after the last pass, which tells which side
//! changed since:
//!
//! - only the reminder changed: the task takes its title, day, time, priority and completion;
//! - the task changed: the reminder takes the task's fields (the task wins when both changed,
//!   except that a completion on the phone is kept);
//! - a linked reminder disappeared: the task is deleted, and a linked task deleted in
//!   Notchpad deletes its reminder;
//! - a new, open reminder becomes a task.
//!
//! When the list comes back empty while the snapshot is not (the list was deleted or recreated),
//! links are dropped instead of deleting tasks, so the tasks are put back into the new list.

use std::collections::{HashMap, HashSet};
use std::{fs, io, path::PathBuf};

use crate::store::Task;

pub const PREFIX: &str = "rem:";

/// A reminder's synced fields, in the task's terms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reminder {
    pub id: String,
    pub text: String,
    /// `YYYY-MM-DD`, or empty for no due date (the Inbox).
    pub day: String,
    /// `HH:MM`, or empty for a due date without a time.
    pub time: String,
    /// `low`, `medium`, `high` or empty.
    pub priority: String,
    pub done: bool,
}

/// Linked pairs as they were after the last pass, by reminder id.
pub type Snapshot = HashMap<String, Reminder>;

/// What the reminders side has to do after [`reconcile`] changed the tasks.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    /// Linked reminders to overwrite with these fields.
    pub update: Vec<Reminder>,
    /// Reminders whose task was deleted in Notchpad.
    pub delete: Vec<String>,
    /// Whether `tasks` changed and needs saving.
    pub tasks_changed: bool,
}

/// Brings `tasks` in line with `reminders` (every reminder of the list, done ones included) and
/// returns what the list needs. Tasks still without a link afterwards are new: see [`to_create`].
pub fn reconcile(tasks: &mut Vec<Task>, reminders: &[Reminder], snapshot: &Snapshot) -> Changes {
    let by_id: HashMap<&str, &Reminder> = reminders.iter().map(|r| (r.id.as_str(), r)).collect();
    let reset = reminders.is_empty() && !snapshot.is_empty();
    let mut changes = Changes::default();
    let mut kept = Vec::with_capacity(tasks.len());
    for mut task in tasks.drain(..) {
        let Some(id) = task.ext.strip_prefix(PREFIX).map(str::to_owned) else {
            kept.push(task);
            continue;
        };
        match by_id.get(id.as_str()) {
            Some(reminder) => {
                let base = snapshot.get(&id);
                let phone_changed = base != Some(*reminder);
                let task_changed = base != Some(&fields(&task, &id));
                if phone_changed && !task_changed {
                    pull(&mut task, reminder);
                    changes.tasks_changed = true;
                } else if task_changed {
                    if phone_changed && reminder.done && !task.done {
                        task.set_done(true);
                        changes.tasks_changed = true;
                    }
                    let mine = fields(&task, &id);
                    if mine != **reminder {
                        changes.update.push(mine);
                    }
                }
                kept.push(task);
            }
            // Deleted on the other side.
            None if snapshot.contains_key(&id) && !reset => changes.tasks_changed = true,
            None => {
                task.ext.clear();
                changes.tasks_changed = true;
                kept.push(task);
            }
        }
    }

    let linked: HashSet<String> = kept.iter().filter_map(|t| t.ext.strip_prefix(PREFIX).map(str::to_owned)).collect();
    for reminder in reminders.iter().filter(|r| !linked.contains(&r.id)) {
        if snapshot.contains_key(&reminder.id) {
            changes.delete.push(reminder.id.clone());
        } else if !reminder.done {
            let mut task = Task { ext: format!("{PREFIX}{}", reminder.id), ..Task::default() };
            pull(&mut task, reminder);
            kept.push(task);
            changes.tasks_changed = true;
        }
    }
    *tasks = kept;
    changes
}

/// Indexes of the tasks that need a new reminder: local and still open.
pub fn to_create(tasks: &[Task]) -> Vec<usize> {
    tasks.iter().enumerate().filter(|(_, t)| t.ext.is_empty() && !t.done && !t.text.trim().is_empty()).map(|(i, _)| i).collect()
}

/// The snapshot after a pass: every linked task as it now stands.
pub fn snapshot_of(tasks: &[Task]) -> Snapshot {
    tasks.iter().filter_map(|t| t.ext.strip_prefix(PREFIX).map(|id| (id.to_owned(), fields(t, id)))).collect()
}

pub fn fields(task: &Task, id: &str) -> Reminder {
    Reminder {
        id: id.to_owned(),
        text: task.text.clone(),
        day: task.day.clone(),
        // A time means nothing without a day, and Reminders cannot hold one.
        time: if task.day.is_empty() { String::new() } else { task.time.clone() },
        priority: task.priority.clone(),
        done: task.done,
    }
}

fn pull(task: &mut Task, reminder: &Reminder) {
    task.text = reminder.text.clone();
    task.day = reminder.day.clone();
    task.time = reminder.time.clone();
    task.priority = reminder.priority.clone();
    task.set_done(reminder.done);
}

// ---- Snapshot file ------------------------------------------------------------------------

fn path() -> PathBuf {
    crate::store::path().with_file_name("reminders.txt")
}

/// One pair per line: `id<TAB>done<TAB>day<TAB>time<TAB>priority<TAB>text`.
pub fn load_snapshot() -> Snapshot {
    fs::read_to_string(path()).map(|text| parse_snapshot(&text)).unwrap_or_default()
}

pub fn save_snapshot(snapshot: &Snapshot) -> io::Result<()> {
    fs::write(path(), format_snapshot(snapshot))
}

fn parse_snapshot(text: &str) -> Snapshot {
    text.lines()
        .filter_map(|line| {
            let mut cols = line.splitn(6, '\t');
            let id = cols.next()?.to_owned();
            let done = cols.next()? == "1";
            let (day, time, priority, text) = (cols.next()?, cols.next()?, cols.next()?, cols.next()?);
            let reminder = Reminder { id: id.clone(), text: text.to_owned(), day: day.to_owned(), time: time.to_owned(), priority: priority.to_owned(), done };
            Some((id, reminder))
        })
        .collect()
}

fn format_snapshot(snapshot: &Snapshot) -> String {
    let mut ids: Vec<&String> = snapshot.keys().collect();
    ids.sort();
    ids.into_iter()
        .map(|id| {
            let r = &snapshot[id];
            // Tabs and newlines would break the line format; titles never need them.
            let text = r.text.replace(['\t', '\n'], " ");
            format!("{id}\t{}\t{}\t{}\t{}\t{text}\n", u8::from(r.done), r.day, r.time, r.priority)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(text: &str, ext: &str) -> Task {
        Task { text: text.to_owned(), day: "2026-11-10".to_owned(), ext: ext.to_owned(), ..Task::default() }
    }

    fn reminder(id: &str, text: &str) -> Reminder {
        Reminder { id: id.to_owned(), text: text.to_owned(), day: "2026-11-10".to_owned(), ..Reminder::default() }
    }

    fn snapshot(reminders: &[Reminder]) -> Snapshot {
        reminders.iter().map(|r| (r.id.clone(), r.clone())).collect()
    }

    #[test]
    fn a_change_on_the_phone_reaches_the_task() {
        let base = reminder("a", "Call mom");
        let mut tasks = vec![task("Call mom", "rem:a")];
        let phone = Reminder { text: "Call mom back".to_owned(), time: "17:00".to_owned(), done: true, ..base.clone() };
        let changes = reconcile(&mut tasks, &[phone], &snapshot(&[base]));
        assert!(changes.tasks_changed && changes.update.is_empty());
        assert_eq!((tasks[0].text.as_str(), tasks[0].time.as_str(), tasks[0].done), ("Call mom back", "17:00", true));
        assert_eq!(tasks[0].done_on, crate::days::key(0));
    }

    #[test]
    fn a_change_in_notchpad_reaches_the_reminder_and_wins_a_conflict() {
        let base = reminder("a", "Call mom");
        let mut tasks = vec![Task { priority: "high".to_owned(), ..task("Call dad", "rem:a") }];
        let phone = Reminder { text: "Call mom later".to_owned(), ..base.clone() };
        let changes = reconcile(&mut tasks, &[phone], &snapshot(&[base]));
        assert_eq!(changes.update, [Reminder { text: "Call dad".to_owned(), priority: "high".to_owned(), ..reminder("a", "") }]);
        assert!(!changes.tasks_changed);
    }

    #[test]
    fn a_completion_on_the_phone_survives_a_conflict() {
        let base = reminder("a", "Call mom");
        let mut tasks = vec![task("Call mom now", "rem:a")];
        let phone = Reminder { done: true, ..base.clone() };
        let changes = reconcile(&mut tasks, &[phone], &snapshot(&[base]));
        assert!(tasks[0].done);
        assert_eq!(changes.update, [Reminder { text: "Call mom now".to_owned(), done: true, ..reminder("a", "") }]);
    }

    #[test]
    fn unchanged_pairs_need_nothing() {
        let base = reminder("a", "Call mom");
        let mut tasks = vec![task("Call mom", "rem:a"), task("Local", ""), task("ENG-1", "linear:ENG-1")];
        let changes = reconcile(&mut tasks, &[base.clone()], &snapshot(&[base]));
        assert_eq!(changes, Changes::default());
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn deletions_cross_over_both_ways() {
        let (gone, kept) = (reminder("a", "Deleted on phone"), reminder("b", "Deleted in Notchpad"));
        let mut tasks = vec![task("Deleted on phone", "rem:a"), task("Stays", "")];
        let changes = reconcile(&mut tasks, &[kept.clone()], &snapshot(&[gone, kept]));
        assert_eq!(tasks.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["Stays"]);
        assert_eq!(changes.delete, ["b"]);
        assert!(changes.tasks_changed);
    }

    #[test]
    fn new_open_reminders_become_tasks_and_done_ones_do_not() {
        let mut tasks = Vec::new();
        let open = Reminder { priority: "medium".to_owned(), ..reminder("n", "Buy milk") };
        let done = Reminder { done: true, ..reminder("d", "Old") };
        let changes = reconcile(&mut tasks, &[open, done], &Snapshot::new());
        assert!(changes.tasks_changed);
        assert_eq!(tasks.len(), 1);
        assert_eq!((tasks[0].ext.as_str(), tasks[0].text.as_str(), tasks[0].priority.as_str()), ("rem:n", "Buy milk", "medium"));
    }

    #[test]
    fn an_emptied_list_unlinks_instead_of_deleting() {
        let mut tasks = vec![task("Call mom", "rem:a")];
        let changes = reconcile(&mut tasks, &[], &snapshot(&[reminder("a", "Call mom")]));
        assert!(changes.tasks_changed);
        assert_eq!((tasks.len(), tasks[0].ext.as_str()), (1, ""));
        assert_eq!(to_create(&tasks), [0]);
    }

    #[test]
    fn only_open_local_tasks_get_a_reminder() {
        let done = Task { done: true, ..task("Done", "") };
        let tasks = [task("Local", ""), done, task("Linked", "rem:a"), task("ENG-1", "linear:ENG-1"), task(" ", "")];
        assert_eq!(to_create(&tasks), [0]);
    }

    #[test]
    fn inbox_tasks_drop_their_time() {
        let inbox = Task { day: String::new(), time: "09:00".to_owned(), ..task("Someday", "rem:a") };
        assert_eq!(fields(&inbox, "a").time, "");
    }

    #[test]
    fn the_snapshot_file_round_trips() {
        let a = Reminder { time: "17:00".to_owned(), priority: "high".to_owned(), done: true, ..reminder("a", "Call\tmom") };
        let b = Reminder { day: String::new(), ..reminder("b", "Inbox thing") };
        let parsed = parse_snapshot(&format_snapshot(&snapshot(&[a.clone(), b.clone()])));
        assert_eq!(parsed["a"], Reminder { text: "Call mom".to_owned(), ..a });
        assert_eq!(parsed["b"], b);
    }
}
