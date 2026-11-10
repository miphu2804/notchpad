//! Today's events from the macOS calendars (Google and iCloud accounts added in System Settings ›
//! Internet Accounts, so Notion Calendar's events arrive through the same Google calendar),
//! read through EventKit. Read-only: nothing here writes to a calendar.
//!
//! Access is asked for only by [`Calendar::request_access`], which Settings calls when the user
//! turns the switch on; every other call just reads the current authorization.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::msg_send;
use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore};
use objc2_foundation::{NSCalendar, NSCalendarOptions, NSCalendarUnit, NSDate, NSError};

use crate::days;

/// One calendar event of today that has not ended yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub title: String,
    /// `HH:MM` (24-hour), or empty for an all-day event.
    pub time: String,
    /// Seconds since the Cocoa reference date, to order events that share a day.
    start: f64,
    /// When it ends, in the same seconds; past this it leaves the list.
    end: f64,
    /// Minutes since today's midnight it covers, clamped to today; `None` for an all-day event.
    pub span: Option<(u32, u32)>,
}

impl Event {
    pub fn ended(&self) -> bool {
        self.end <= NSDate::now().timeIntervalSinceReferenceDate()
    }
}

/// What macOS lets NotchGo do with the user's calendars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Access {
    Undecided,
    Granted,
    Denied,
}

pub fn access() -> Access {
    access_for(EKEntityType::Event)
}

/// The same for events or reminders, which macOS asks about separately.
pub fn access_for(entity: EKEntityType) -> Access {
    let status = unsafe { EKEventStore::authorizationStatusForEntityType(entity) };
    if status == EKAuthorizationStatus::NotDetermined {
        Access::Undecided
    } else if status == EKAuthorizationStatus::FullAccess {
        Access::Granted
    } else {
        Access::Denied
    }
}

/// The event store, made on first use so that a Mac whose user never turns the calendar on never touches EventKit.
#[derive(Default)]
pub struct Calendar {
    store: Option<Retained<EKEventStore>>,
}

impl Calendar {
    /// Shows the system's permission prompt if the user has not answered it yet. The answer arrives on
    /// an EventKit queue, so `target` is told to call `selector` on the main thread.
    pub fn request_access(&mut self, target: &AnyObject, selector: Sel) {
        // Raw address: the controller lives for the process, and performSelector is thread-safe.
        let target = target as *const AnyObject as usize;
        let done = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
            let target = target as *const AnyObject;
            let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
        });
        unsafe { self.store().requestFullAccessToEventsWithCompletion(&*done as *const _ as *mut _) };
    }

    /// Today's events in display order; empty without access.
    pub fn today(&mut self) -> Vec<Event> {
        if access() != Access::Granted {
            return Vec::new();
        }
        let calendar = NSCalendar::currentCalendar();
        // From now, not midnight: EventKit returns what overlaps the range, so the event under
        // way stays and the finished ones drop out.
        let now = NSDate::now();
        let midnight = calendar.startOfDayForDate(&now);
        let Some(end) = calendar.dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, 1, &midnight, NSCalendarOptions::empty()) else {
            return Vec::new();
        };
        let day_start = midnight.timeIntervalSinceReferenceDate();
        let store = self.store();
        let events = unsafe {
            let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&now, &end, None);
            store.eventsMatchingPredicate(&predicate).iter().map(|e| {
                let time = if e.isAllDay() { String::new() } else { days::clock_of(&e.startDate()) };
                let (start, end) = (e.startDate().timeIntervalSinceReferenceDate(), e.endDate().timeIntervalSinceReferenceDate());
                let minute = |at: f64| ((at - day_start) / 60.0).clamp(0.0, 24.0 * 60.0) as u32;
                let span = (!e.isAllDay()).then(|| (minute(start), minute(end)));
                Event { title: e.title().to_string(), time, start, end, span }
            }).collect()
        };
        ordered(events)
    }

    fn store(&mut self) -> &EKEventStore {
        self.store.get_or_insert_with(|| unsafe { EKEventStore::new() })
    }
}

/// All-day events first, then by start time; EventKit promises no order.
fn ordered(mut events: Vec<Event>) -> Vec<Event> {
    events.sort_by(|a, b| a.time.is_empty().cmp(&b.time.is_empty()).reverse().then(a.start.total_cmp(&b.start)));
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, time: &str, start: f64) -> Event {
        Event { title: title.to_owned(), time: time.to_owned(), start, end: start + 1.0, span: None }
    }

    #[test]
    fn all_day_events_come_first_then_by_start() {
        let sorted = ordered(vec![event("lunch", "12:30", 30.0), event("holiday", "", 0.0), event("standup", "09:00", 10.0)]);
        let titles: Vec<_> = sorted.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["holiday", "standup", "lunch"]);
    }

    #[test]
    fn an_event_has_ended_once_its_end_is_past() {
        let now = NSDate::now().timeIntervalSinceReferenceDate();
        assert!(event("breakfast", "08:30", now - 3600.0).ended());
        assert!(!Event { end: now + 3600.0, ..event("exam", "12:30", now - 60.0) }.ended());
    }
}
