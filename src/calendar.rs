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

/// One calendar event of today.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub title: String,
    /// `HH:MM` (24-hour), or empty for an all-day event.
    pub time: String,
    /// Seconds since the Cocoa reference date, to order events that share a day.
    start: f64,
}

/// What macOS lets Notchpad do with the user's calendars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Access {
    Undecided,
    Granted,
    Denied,
}

pub fn access() -> Access {
    let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
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
        let start = calendar.startOfDayForDate(&NSDate::now());
        let Some(end) = calendar.dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, 1, &start, NSCalendarOptions::empty()) else {
            return Vec::new();
        };
        let store = self.store();
        let events = unsafe {
            let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);
            store.eventsMatchingPredicate(&predicate).iter().map(|e| {
                let time = if e.isAllDay() { String::new() } else { days::clock_of(&e.startDate()) };
                Event { title: e.title().to_string(), time, start: e.startDate().timeIntervalSinceReferenceDate() }
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
        Event { title: title.to_owned(), time: time.to_owned(), start }
    }

    #[test]
    fn all_day_events_come_first_then_by_start() {
        let sorted = ordered(vec![event("lunch", "12:30", 30.0), event("holiday", "", 0.0), event("standup", "09:00", 10.0)]);
        let titles: Vec<_> = sorted.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["holiday", "standup", "lunch"]);
    }
}
