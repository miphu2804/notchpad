//! Read-only calendar events for the Week tab, read through EventKit.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSColorSpace};
use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore, EKEventStoreChangedNotification};
use objc2_foundation::{NSCalendar, NSCalendarOptions, NSCalendarUnit, NSDate, NSNotificationCenter};

use crate::days;

/// One event on one day; a multi-day event yields one `Event` per day it covers.
pub struct Event {
    pub day_key: String,
    /// Minutes after midnight, or `None` for an all-day event.
    pub start_minutes: Option<u32>,
    pub title: String,
    pub color: (u8, u8, u8),
}

#[derive(PartialEq)]
pub enum Access {
    /// The user has not been asked yet; only an explicit click may ask.
    NotDetermined,
    Granted,
    /// Denied, restricted or write-only: nothing calendar-related is shown.
    Unavailable,
}

/// Reads the authorization status, which never prompts.
pub fn access() -> Access {
    match unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) } {
        EKAuthorizationStatus::NotDetermined => Access::NotDetermined,
        EKAuthorizationStatus::FullAccess => Access::Granted,
        _ => Access::Unavailable,
    }
}

pub struct Calendar {
    store: Retained<EKEventStore>,
}

impl Default for Calendar {
    fn default() -> Self {
        Self { store: unsafe { EKEventStore::new() } }
    }
}

impl Calendar {
    /// Shows the system prompt. Called only from the "Show calendar events" pill.
    pub fn request_access(&self) {
        // The handler runs on a background queue, so it only posts the change notification
        // (thread-safe); the observer re-fetches on the main queue.
        let done = RcBlock::new(|_granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| unsafe {
            NSNotificationCenter::defaultCenter().postNotificationName_object(EKEventStoreChangedNotification, None);
        });
        unsafe { self.store.requestFullAccessToEventsWithCompletion(&*done as *const _ as *mut _) };
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
}
