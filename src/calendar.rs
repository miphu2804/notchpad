//! Calendar events and due reminders for the Week tab, read through EventKit.

use std::sync::{Arc, Mutex};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSColorSpace};
use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore, EKEventStoreChangedNotification, EKReminder};
use objc2_foundation::{NSArray, NSCalendar, NSCalendarOptions, NSCalendarUnit, NSDate, NSDateComponentUndefined, NSNotificationCenter, NSString};

use crate::days;

/// One event on one day; a multi-day event yields one `Event` per day it covers.
pub struct Event {
    pub day_key: String,
    /// Minutes after midnight, or `None` for an all-day event.
    pub start_minutes: Option<u32>,
    pub title: String,
    pub color: (u8, u8, u8),
}

/// One incomplete reminder due today, tomorrow or earlier.
pub struct Reminder {
    /// `calendarItemIdentifier`, used to find the reminder again when it is completed.
    pub id: String,
    /// The column it belongs in; overdue reminders sit in today's.
    pub day_key: String,
    /// Minutes after midnight, or `None` when the reminder has a date only.
    pub due_minutes: Option<u32>,
    pub overdue: bool,
    pub title: String,
    /// The reminder list's colour.
    pub color: (u8, u8, u8),
}

/// Posted on the main queue once a reminders fetch has landed in `Calendar::take_fetched`.
pub const REMINDERS_FETCHED: &str = "NotchpadRemindersFetched";

#[derive(PartialEq)]
pub enum Access {
    /// The user has not been asked yet; only an explicit click may ask.
    NotDetermined,
    Granted,
    /// Denied, restricted or write-only: nothing calendar-related is shown.
    Unavailable,
}

/// Reads the calendar authorization status, which never prompts.
pub fn access() -> Access {
    status(EKEntityType::Event)
}

/// Reminders have their own authorization, separate from calendar events.
pub fn reminders_access() -> Access {
    status(EKEntityType::Reminder)
}

fn status(entity: EKEntityType) -> Access {
    match unsafe { EKEventStore::authorizationStatusForEntityType(entity) } {
        EKAuthorizationStatus::NotDetermined => Access::NotDetermined,
        EKAuthorizationStatus::FullAccess => Access::Granted,
        _ => Access::Unavailable,
    }
}

pub struct Calendar {
    store: Retained<EKEventStore>,
    /// Filled by the fetch completion on a background queue, drained on the main queue.
    fetched: Arc<Mutex<Option<Vec<Reminder>>>>,
}

impl Default for Calendar {
    fn default() -> Self {
        Self { store: unsafe { EKEventStore::new() }, fetched: Arc::default() }
    }
}

impl Calendar {
    /// Shows the system prompt. Called only from the "Show calendar events" pill.
    pub fn request_access(&self) {
        unsafe { self.store.requestFullAccessToEventsWithCompletion(&*Self::notify_changed() as *const _ as *mut _) };
    }

    /// Shows the system prompt. Called only from the "Show reminders" pill.
    pub fn request_reminders_access(&self) {
        unsafe { self.store.requestFullAccessToRemindersWithCompletion(&*Self::notify_changed() as *const _ as *mut _) };
    }

    /// The handler runs on a background queue, so it only posts the change notification
    /// (thread-safe); the observer re-fetches on the main queue.
    fn notify_changed() -> RcBlock<dyn Fn(objc2::runtime::Bool, *mut objc2_foundation::NSError)> {
        RcBlock::new(|_granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| unsafe {
            NSNotificationCenter::defaultCenter().postNotificationName_object(EKEventStoreChangedNotification, None);
        })
    }

    /// Starts an asynchronous fetch of incomplete reminders due today, tomorrow or earlier. The
    /// result is posted as `REMINDERS_FETCHED` and read with `take_fetched`; does nothing
    /// unless access is granted.
    pub fn fetch_reminders(&self) {
        if reminders_access() != Access::Granted {
            return;
        }
        let end_of_tomorrow = Self::day_start(2);
        // A nil start includes overdue reminders.
        let predicate = unsafe { self.store.predicateForIncompleteRemindersWithDueDateStarting_ending_calendars(None, Some(&end_of_tomorrow), None) };
        let fetched = self.fetched.clone();
        let done = RcBlock::new(move |found: *mut NSArray<EKReminder>| {
            // Runs off the main thread: convert to plain values here, then hop to the main queue.
            let reminders = unsafe { found.as_ref() }.map(|found| found.iter().filter_map(|r| to_reminder(&r)).collect()).unwrap_or_default();
            *fetched.lock().unwrap() = Some(reminders);
            DispatchQueue::main().exec_async(|| unsafe {
                NSNotificationCenter::defaultCenter().postNotificationName_object(&NSString::from_str(REMINDERS_FETCHED), None);
            });
        });
        unsafe { self.store.fetchRemindersMatchingPredicate_completion(&predicate, &done) };
    }

    /// The latest completed fetch, once.
    pub fn take_fetched(&self) -> Option<Vec<Reminder>> {
        self.fetched.lock().unwrap().take()
    }

    /// Ticks a reminder off; the resulting `EKEventStoreChanged` triggers the re-fetch.
    pub fn complete(&self, id: &str) {
        let item = unsafe { self.store.calendarItemWithIdentifier(&NSString::from_str(id)) };
        if let Some(reminder) = item.and_then(|item| item.downcast::<EKReminder>().ok()) {
            unsafe {
                reminder.setCompleted(true);
                let _ = self.store.saveReminder_commit_error(&reminder, true);
            }
        }
    }

    fn day_start(offset: i64) -> Retained<NSDate> {
        let calendar = NSCalendar::currentCalendar();
        let today = calendar.startOfDayForDate(&NSDate::now());
        calendar.dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, offset as isize, &today, NSCalendarOptions::empty()).unwrap_or(today)
    }

    /// Events for today through `last_offset` days ahead; empty unless access is granted.
    pub fn events(&self, last_offset: i64) -> Vec<Event> {
        if access() != Access::Granted {
            return Vec::new();
        }
        let calendar = NSCalendar::currentCalendar();
        let day_start = |offset: i64| {
            let today = calendar.startOfDayForDate(&NSDate::now());
            calendar.dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, offset as isize, &today, NSCalendarOptions::empty()).unwrap_or(today)
        };
        let found = unsafe {
            let predicate = self.store.predicateForEventsWithStartDate_endDate_calendars(&day_start(0), &day_start(last_offset + 1), None);
            self.store.eventsMatchingPredicate(&predicate)
        };
        let mut events = Vec::new();
        for offset in 0..=last_offset {
            let (start, end) = (day_start(offset), day_start(offset + 1));
            for event in found.iter() {
                let (from, to) = unsafe { (event.startDate(), event.endDate()) };
                if from.timeIntervalSinceDate(&end) >= 0.0 || to.timeIntervalSinceDate(&start) <= 0.0 {
                    continue;
                }
                let minutes = (from.timeIntervalSinceDate(&start).max(0.0) / 60.0) as u32;
                let color = unsafe { event.calendar() }.map(|c| rgb(&c)).unwrap_or((150, 150, 150));
                events.push(Event {
                    day_key: days::key(offset),
                    start_minutes: if unsafe { event.isAllDay() } { None } else { Some(minutes) },
                    title: unsafe { event.title() }.to_string(),
                    color,
                });
            }
        }
        events
    }
}

/// Plain values of one reminder; `None` when it has no due date or is due after tomorrow.
fn to_reminder(reminder: &EKReminder) -> Option<Reminder> {
    let due = unsafe { reminder.dueDateComponents() }?;
    let calendar = NSCalendar::currentCalendar();
    let date = calendar.dateFromComponents(&due)?;
    let offset = calendar.components_fromDate_toDate_options(NSCalendarUnit::Day, &Calendar::day_start(0), &calendar.startOfDayForDate(&date), NSCalendarOptions::empty()).day() as i64;
    let due_minutes = (due.hour() != NSDateComponentUndefined).then(|| (due.hour() * 60 + due.minute().max(0)) as u32);
    let now = calendar.components_fromDate(NSCalendarUnit::Hour | NSCalendarUnit::Minute, &NSDate::now());
    let (column, overdue) = place(offset, due_minutes, (now.hour() * 60 + now.minute()) as u32)?;
    let color = unsafe { reminder.calendar() }.map(|c| rgb(&c)).unwrap_or((150, 150, 150));
    Some(Reminder { id: unsafe { reminder.calendarItemIdentifier() }.to_string(), day_key: days::key(column), due_minutes, overdue, title: unsafe { reminder.title() }.to_string(), color })
}

/// The Week column (0 today, 1 tomorrow) a reminder due `offset` days from today belongs in, and
/// whether it is overdue; `None` when it is not due yet. Earlier days collapse into today.
fn place(offset: i64, due_minutes: Option<u32>, now_minutes: u32) -> Option<(i64, bool)> {
    match offset {
        ..0 => Some((0, true)),
        0 => Some((0, due_minutes.is_some_and(|m| m < now_minutes))),
        1 => Some((1, false)),
        _ => None,
    }
}

/// The calendar's colour as sRGB bytes.
fn rgb(calendar: &objc2_event_kit::EKCalendar) -> (u8, u8, u8) {
    let srgb = unsafe { calendar.CGColor() }
        .map(|c| NSColor::colorWithCGColor(&c))
        .and_then(|c| c.and_then(|c| c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())));
    match srgb {
        Some(c) => ((c.redComponent() * 255.0) as u8, (c.greenComponent() * 255.0) as u8, (c.blueComponent() * 255.0) as u8),
        None => (150, 150, 150),
    }
}

/// The first `max` events of `day_key` (all-day first, then by start time) and how many more there are.
pub fn day_rows<'a>(events: &'a [Event], day_key: &str, max: usize) -> (Vec<&'a Event>, usize) {
    let mut day: Vec<&Event> = events.iter().filter(|e| e.day_key == day_key).collect();
    // `None` (all day) sorts before any `Some` start time; the sort is stable.
    day.sort_by_key(|e| e.start_minutes);
    let more = day.len().saturating_sub(max);
    day.truncate(max);
    (day, more)
}

/// The first `max` reminders of `day_key` (overdue first, then by due time, date-only last) and how many more there are.
pub fn reminder_rows<'a>(reminders: &'a [Reminder], day_key: &str, max: usize) -> (Vec<&'a Reminder>, usize) {
    let mut day: Vec<&Reminder> = reminders.iter().filter(|r| r.day_key == day_key).collect();
    day.sort_by_key(|r| (!r.overdue, r.due_minutes.is_none(), r.due_minutes));
    let more = day.len().saturating_sub(max);
    day.truncate(max);
    (day, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(day: &str, start: Option<u32>, title: &str) -> Event {
        Event { day_key: day.into(), start_minutes: start, title: title.into(), color: (0, 0, 0) }
    }

    fn titles(rows: &[&Event]) -> Vec<String> {
        rows.iter().map(|e| e.title.clone()).collect()
    }

    #[test]
    fn all_day_first_then_by_start_time() {
        let events = [event("d1", Some(600), "ten"), event("d1", None, "holiday"), event("d1", Some(540), "nine")];
        let (rows, more) = day_rows(&events, "d1", 4);
        assert_eq!(titles(&rows), ["holiday", "nine", "ten"]);
        assert_eq!(more, 0);
    }

    #[test]
    fn caps_rows_and_counts_the_rest() {
        let events: Vec<Event> = (0..7).map(|i| event("d1", Some(i * 60), &format!("e{i}"))).collect();
        let (rows, more) = day_rows(&events, "d1", 4);
        assert_eq!(titles(&rows), ["e0", "e1", "e2", "e3"]);
        assert_eq!(more, 3);
    }

    #[test]
    fn multi_day_event_shows_on_each_day_it_covers() {
        let events = [event("d1", Some(1320), "trip"), event("d2", Some(0), "trip"), event("d3", Some(60), "other")];
        assert_eq!(titles(&day_rows(&events, "d1", 4).0), ["trip"]);
        assert_eq!(titles(&day_rows(&events, "d2", 4).0), ["trip"]);
        assert!(day_rows(&events, "d4", 4).0.is_empty());
    }

    fn reminder(day: &str, due: Option<u32>, overdue: bool, title: &str) -> Reminder {
        Reminder { id: title.into(), day_key: day.into(), due_minutes: due, overdue, title: title.into(), color: (0, 0, 0) }
    }

    #[test]
    fn earlier_days_are_overdue_in_today() {
        assert_eq!(place(-3, None, 600), Some((0, true)));
        assert_eq!(place(-1, Some(900), 600), Some((0, true)));
    }

    #[test]
    fn today_is_overdue_only_once_the_time_has_passed() {
        assert_eq!(place(0, Some(540), 600), Some((0, true)));
        assert_eq!(place(0, Some(900), 600), Some((0, false)));
        assert_eq!(place(0, None, 600), Some((0, false)));
    }

    #[test]
    fn tomorrow_is_placed_and_later_is_not() {
        assert_eq!(place(1, Some(0), 600), Some((1, false)));
        assert_eq!(place(2, None, 600), None);
    }

    #[test]
    fn reminders_sort_overdue_then_timed_then_date_only() {
        let all = [reminder("d1", None, false, "plain"), reminder("d1", Some(900), false, "three"), reminder("d1", Some(540), true, "late"), reminder("d2", Some(60), false, "other day")];
        let (rows, more) = reminder_rows(&all, "d1", 5);
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["late", "three", "plain"]);
        assert_eq!(more, 0);
    }

    #[test]
    fn reminders_are_capped_with_a_remainder() {
        let all: Vec<Reminder> = (0..5).map(|i| reminder("d1", Some(i * 60), false, &format!("r{i}"))).collect();
        let (rows, more) = reminder_rows(&all, "d1", 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(more, 2);
    }
}
