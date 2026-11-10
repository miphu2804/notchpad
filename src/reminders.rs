//! EventKit side of the Reminders sync: the "NotchGo" list, fetching its reminders and writing
//! back what [`crate::reminders_sync`] decided. The fetch completes on an EventKit queue, so the
//! result waits here until the controller takes it on the main thread.
//!
//! Access is asked for only by [`Reminders::request_access`], which runs when the user turns the
//! sync on; every other call just reads the current authorization.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use block2::RcBlock;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2_event_kit::{EKCalendar, EKEntityType, EKEventStore, EKReminder};
use objc2_foundation::{NSArray, NSDate, NSDateComponents, NSError, NSString};

use crate::calendar::{self, Access};
use crate::reminders_sync::{self, Changes, PREFIX, Reminder};
use crate::store::Task;

const LIST: &str = "NotchGo";
/// How often the phone's edits are picked up when nothing changed here and no
/// `EKEventStoreChangedNotification` arrived (see [`Reminders::store_changed`]).
const SYNC_EVERY: Duration = Duration::from_secs(30);
/// `NSDateComponentUndefined`: a component the reminder does not set.
const UNDEFINED: isize = isize::MAX;

pub fn access() -> Access {
    calendar::access_for(EKEntityType::Reminder)
}

#[derive(Default)]
pub struct Reminders {
    store: Option<Retained<EKEventStore>>,
    /// Filled by the fetch's completion, each reminder with its last modification; `None` until
    /// then, or when EventKit returned nothing.
    fetched: Arc<Mutex<Option<Vec<(Reminder, Option<SystemTime>)>>>>,
    pending: bool,
    /// The list was created by the fetch in flight, so an empty result is not a deletion.
    list_created: bool,
    last: Option<Instant>,
    /// The task file's modification time after the last pass; a change means local edits to push.
    synced: Option<SystemTime>,
}

impl Reminders {
    /// Shows the system's permission prompt if the user has not answered it yet; `selector` runs
    /// on the main thread once they have.
    pub fn request_access(&mut self, target: &AnyObject, selector: Sel) {
        let target = target as *const AnyObject as usize;
        let done = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
            let target = target as *const AnyObject;
            let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
        });
        unsafe { self.store().requestFullAccessToRemindersWithCompletion(&*done as *const _ as *mut _) };
    }

    /// Whether a pass is due: none in flight, and the tasks changed or the phone may have.
    pub fn due(&self, tasks_modified: Option<SystemTime>) -> bool {
        !self.pending && (tasks_modified != self.synced || self.last.is_none_or(|at| at.elapsed() >= SYNC_EVERY))
    }

    /// The Reminders database changed (the phone's edit arrived through iCloud, or this app
    /// wrote): the next [`due`](Self::due) check runs a pass without waiting for the interval.
    /// A pass after this app's own write finds nothing to change, so it does not loop.
    pub fn store_changed(&mut self) {
        self.last = None;
    }

    /// Reads every reminder of the list (creating the list on first use); `selector` runs on the
    /// main thread when they are ready to [`take`](Self::take).
    pub fn fetch(&mut self, target: &AnyObject, selector: Sel) {
        self.last = Some(Instant::now());
        let list = match self.list() {
            Ok(list) => list,
            Err(err) => return eprintln!("notchpad: no Reminders list: {err}"),
        };
        self.pending = true;
        let fetched = Arc::clone(&self.fetched);
        let target = target as *const AnyObject as usize;
        let done = RcBlock::new(move |found: *mut NSArray<EKReminder>| {
            if let Some(found) = unsafe { found.as_ref() } {
                *fetched.lock().unwrap() = Some(found.iter().map(|r| read(&r)).collect());
            }
            let target = target as *const AnyObject;
            let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
        });
        let store = self.store();
        unsafe {
            let predicate = store.predicateForRemindersInCalendars(Some(&NSArray::from_retained_slice(&[list])));
            store.fetchRemindersMatchingPredicate_completion(&predicate, &done);
        }
    }

    /// The finished fetch, each reminder's last modification by id, and whether the list was
    /// new; `None` when EventKit failed.
    pub fn take(&mut self) -> Option<(Vec<Reminder>, HashMap<String, SystemTime>, bool)> {
        self.pending = false;
        let list_created = std::mem::take(&mut self.list_created);
        let found = self.fetched.lock().unwrap().take()?;
        let modified = found.iter().filter_map(|(r, at)| Some((r.id.clone(), (*at)?))).collect();
        Some((found.into_iter().map(|(r, _)| r).collect(), modified, list_created))
    }

    /// Writes `changes` to the list and creates a reminder for each new local task, linking it.
    /// Returns whether a task got a link.
    pub fn apply(&mut self, changes: &Changes, tasks: &mut [Task]) -> bool {
        let Ok(list) = self.list() else { return false };
        let store = self.store().clone();
        for fields in &changes.update {
            if let Some(reminder) = find(&store, &fields.id) {
                write(&reminder, fields);
                if let Err(err) = unsafe { store.saveReminder_commit_error(&reminder, true) } {
                    eprintln!("notchpad: could not update a reminder: {err}");
                }
            }
        }
        for id in &changes.delete {
            if let Some(reminder) = find(&store, id)
                && let Err(err) = unsafe { store.removeReminder_commit_error(&reminder, true) }
            {
                eprintln!("notchpad: could not delete a reminder: {err}");
            }
        }
        let mut linked = false;
        for i in reminders_sync::to_create(tasks) {
            let reminder = unsafe { EKReminder::reminderWithEventStore(&store) };
            unsafe { reminder.setCalendar(Some(&list)) };
            write(&reminder, &reminders_sync::fields(&tasks[i], ""));
            match unsafe { store.saveReminder_commit_error(&reminder, true) } {
                Ok(()) => {
                    tasks[i].ext = format!("{PREFIX}{}", unsafe { reminder.calendarItemIdentifier() });
                    linked = true;
                }
                Err(err) => eprintln!("notchpad: could not add a reminder: {err}"),
            }
        }
        linked
    }

    /// Records the task file as synced, so only later edits make the next pass due.
    pub fn synced(&mut self, tasks_modified: Option<SystemTime>) {
        self.synced = tasks_modified;
    }

    /// The list named "NotchGo", made in the account that holds new reminders if it is missing.
    fn list(&mut self) -> Result<Retained<EKCalendar>, String> {
        let store = self.store().clone();
        let lists = unsafe { store.calendarsForEntityType(EKEntityType::Reminder) };
        if let Some(list) = lists.iter().find(|l| unsafe { l.title() }.to_string() == LIST) {
            return Ok(list);
        }
        let source = unsafe { store.defaultCalendarForNewReminders() }.and_then(|c| unsafe { c.source() }).ok_or("no Reminders account")?;
        let list = unsafe { EKCalendar::calendarForEntityType_eventStore(EKEntityType::Reminder, &store) };
        unsafe {
            list.setTitle(&NSString::from_str(LIST));
            list.setSource(Some(&source));
            store.saveCalendar_commit_error(&list, true).map_err(|e| e.to_string())?;
        }
        self.list_created = true;
        Ok(list)
    }

    fn store(&mut self) -> &Retained<EKEventStore> {
        self.store.get_or_insert_with(|| unsafe { EKEventStore::new() })
    }
}

fn find(store: &EKEventStore, id: &str) -> Option<Retained<EKReminder>> {
    unsafe { store.calendarItemWithIdentifier(&NSString::from_str(id)) }?.downcast::<EKReminder>().ok()
}

fn read(reminder: &EKReminder) -> (Reminder, Option<SystemTime>) {
    let (day, time) = unsafe { reminder.dueDateComponents() }.map_or_else(Default::default, |due| due_strings(&due));
    let fields = Reminder {
        id: unsafe { reminder.calendarItemIdentifier() }.to_string(),
        text: unsafe { reminder.title() }.to_string(),
        day,
        time,
        priority: priority_name(unsafe { reminder.priority() }).to_owned(),
        project: reminders_sync::project_of(&notes(reminder)),
        done: unsafe { reminder.isCompleted() },
    };
    (fields, unsafe { reminder.lastModifiedDate() }.map(|date| system_time(&date)))
}

fn write(reminder: &EKReminder, fields: &Reminder) {
    let notes = notes(reminder);
    let tagged = reminders_sync::with_project(&notes, &fields.project);
    unsafe {
        reminder.setTitle(Some(&NSString::from_str(&reminders_sync::clean(&fields.text))));
        reminder.setDueDateComponents(due_components(&fields.day, &fields.time).as_deref());
        if tagged != notes {
            reminder.setNotes((!tagged.is_empty()).then(|| NSString::from_str(&tagged)).as_deref());
        }
        // Keep the phone's exact 1-9 when it already means the same level.
        if priority_name(reminder.priority()) != fields.priority {
            reminder.setPriority(priority_value(&fields.priority));
        }
        if reminder.isCompleted() != fields.done {
            reminder.setCompleted(fields.done);
        }
    }
}

fn notes(reminder: &EKReminder) -> String {
    unsafe { reminder.notes() }.map(|n| n.to_string()).unwrap_or_default()
}

fn system_time(date: &NSDate) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs_f64(date.timeIntervalSince1970().max(0.0))
}

fn due_strings(due: &NSDateComponents) -> (String, String) {
    let (year, month, day) = (due.year(), due.month(), due.day());
    if [year, month, day].contains(&UNDEFINED) {
        return Default::default();
    }
    let (hour, minute) = (due.hour(), due.minute());
    let time = if hour == UNDEFINED { String::new() } else { format!("{hour:02}:{:02}", if minute == UNDEFINED { 0 } else { minute }) };
    (format!("{year:04}-{month:02}-{day:02}"), time)
}

fn due_components(day: &str, time: &str) -> Option<Retained<NSDateComponents>> {
    let mut date = day.splitn(3, '-').map(|n| n.parse::<isize>());
    let (Some(Ok(year)), Some(Ok(month)), Some(Ok(d))) = (date.next(), date.next(), date.next()) else { return None };
    let due = NSDateComponents::new();
    due.setYear(year);
    due.setMonth(month);
    due.setDay(d);
    if let Some((Ok(hour), Ok(minute))) = time.split_once(':').map(|(h, m)| (h.parse(), m.parse())) {
        due.setHour(hour);
        due.setMinute(minute);
    }
    Some(due)
}

/// Reminders' 1-9 scale, as the levels its own menu offers: 1 high, 5 medium, 9 low.
fn priority_name(value: usize) -> &'static str {
    match value {
        1..=4 => "high",
        5 => "medium",
        6..=9 => "low",
        _ => "",
    }
}

fn priority_value(name: &str) -> usize {
    match name {
        "high" => 1,
        "medium" => 5,
        "low" => 9,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_round_trip_through_the_one_to_nine_scale() {
        for name in ["high", "medium", "low", ""] {
            assert_eq!(priority_name(priority_value(name)), name);
        }
        assert_eq!(priority_name(3), "high");
    }

    #[test]
    fn due_dates_round_trip_with_and_without_a_time() {
        for (day, time) in [("2026-11-10", "17:05"), ("2026-01-02", ""), ("", "")] {
            let due = due_components(day, time);
            assert_eq!(due.map_or_else(Default::default, |d| due_strings(&d)), (day.to_owned(), time.to_owned()));
        }
    }
}
