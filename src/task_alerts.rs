//! macOS notifications for tasks scheduled at a time of day.
//!
//! `due_alerts` decides which tasks need a notification (pure, unit-tested); `sync` replaces the
//! pending `notchpad.task.*` requests with them. Authorization is requested only once a timed task
//! actually needs it, never at launch. `reminders_between` picks the in-notch reminders the
//! controller pops open while the app runs.

use block2::RcBlock;
use objc2::{rc::Retained, runtime::Bool};
use objc2_foundation::{NSArray, NSBundle, NSDateComponents, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNCalendarNotificationTrigger, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSettings, UNNotificationSound, UNUserNotificationCenter,
};
use std::ptr::NonNull;

use crate::{days, store::Task};

const ID_PREFIX: &str = "notchpad.task.";
/// Only the next week is scheduled; every save re-syncs, so later tasks join the window as it moves.
const DAYS_AHEAD: i64 = 7;

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub id: String,
    pub day: String,
    pub hour: u32,
    pub minute: u32,
    pub title: String,
    pub body: String,
}

/// Notifications for undone tasks with a `day` and a `time`, from now until `days_ahead` days out:
/// one `lead` minutes before the time (none when `lead` is 0 or would fall on the day before) and
/// one at it, worded like the notch card. Only those still ahead of `now_minutes` are kept.
pub fn due_alerts(tasks: &[Task], now_day: &str, now_minutes: u32, days_ahead: i64, lead: u32) -> Vec<Alert> {
    let Some(today) = day_number(now_day) else { return Vec::new() };
    let mut alerts = Vec::new();
    for t in tasks.iter().filter(|t| !t.done) {
        let (Some(day), Some(at)) = (day_number(&t.day), minutes(&t.time)) else { continue };
        let offset = day - today;
        if !(0..=days_ahead).contains(&offset) {
            continue;
        }
        let key = format!("{}\t{}\t{}", t.day, t.time, t.text);
        let project = if t.project.is_empty() { String::new() } else { format!(" · #{}", t.project) };
        let time = clock_12h(&t.time).unwrap_or_default();
        let early = (lead > 0 && at >= lead).then(|| (at - lead, format!("{key}\tlead"), format!("in {lead} min")));
        for (fire, id, when) in early.into_iter().chain([(at, key.clone(), "now".to_owned())]) {
            if offset == 0 && fire <= now_minutes {
                continue;
            }
            alerts.push(Alert {
                id: format!("{ID_PREFIX}{:016x}", fnv1a(&id)),
                day: t.day.clone(),
                hour: fire / 60,
                minute: fire % 60,
                title: t.text.clone(),
                body: format!("{time} · {when}{project}"),
            });
        }
    }
    alerts
}

/// `HH:MM` on a 12-hour clock, such as `5:30 PM`; `None` for malformed text.
pub fn clock_12h(time: &str) -> Option<String> {
    let (h, m) = time.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    Some(format!("{}:{m} {}", (h + 11) % 12 + 1, if h < 12 { "AM" } else { "PM" }))
}

/// A timed task the notch reminds of: the task's text and `HH:MM`, and its minute of the day.
#[derive(Clone, Debug, PartialEq)]
pub struct Reminder {
    pub text: String,
    pub time: String,
    pub at: u32,
}

/// Open tasks of `today` with a reminder minute in `(after, upto]`: `lead` minutes before the
/// task's time and at it (once when `lead` is 0). A range rather than a minute, so a reminder whose
/// minute passed between two checks (a busy main thread, a short sleep) still shows; the caller
/// bounds it. Sorted by the task's time.
pub fn reminders_between(tasks: &[Task], today: &str, lead: u32, after: u32, upto: u32) -> Vec<Reminder> {
    let mut due: Vec<Reminder> = tasks
        .iter()
        .filter(|t| !t.done && t.day == today)
        .filter_map(|t| {
            let (hour, minute) = clock(&t.time)?;
            let at = hour * 60 + minute;
            let in_range = |minute: u32| after < minute && minute <= upto;
            let ahead = lead > 0 && at >= lead && in_range(at - lead);
            (in_range(at) || ahead).then(|| Reminder { text: t.text.clone(), time: t.time.clone(), at })
        })
        .collect();
    due.sort_by_key(|r| r.at);
    due
}

/// `HH:MM` as minutes past midnight; `None` for empty or malformed text.
pub fn minutes(text: &str) -> Option<u32> {
    clock(text).map(|(h, m)| h * 60 + m)
}

/// Replaces the pending task notifications with those `tasks` need. `remind` is the reminder
/// lead from the settings: with it set, the notch shows its own card for each notification and
/// owns the next minute; with it off, only the at-the-time notification is kept. Silently does
/// nothing in an unbundled binary, where `UNUserNotificationCenter` would throw.
pub fn sync(tasks: &[Task], remind: Option<u32>) {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let now = days::stamp();
    let (now_day, clock_now) = now.split_once(' ').unwrap_or_default();
    let now_minutes = clock(clock_now).map_or(0, |(h, m)| h * 60 + m);
    // With notch cards on, the next minute belongs to the card (see `withdraw_next_minute`), so a
    // save in it does not put back the notification just withdrawn. Without them the system
    // notification is the only reminder and must stay.
    let from = now_minutes + u32::from(remind.is_some());
    let alerts = due_alerts(tasks, now_day, from, DAYS_AHEAD, remind.unwrap_or(0));

    let center = UNUserNotificationCenter::currentNotificationCenter();
    let handler = RcBlock::new({
        let center = center.clone();
        move |settings: NonNull<UNNotificationSettings>| {
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            if status == UNAuthorizationStatus::NotDetermined && !alerts.is_empty() {
                request_then_schedule(&center, alerts.clone());
            } else if matches!(status, UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Provisional | UNAuthorizationStatus::Ephemeral) {
                schedule(&center, &alerts);
            }
        }
    });
    center.getNotificationSettingsWithCompletionHandler(&handler);
}

/// Ids of the notifications for open tasks of `today` that fire in the minute after `now`.
pub fn next_minute_ids(tasks: &[Task], today: &str, now: u32, lead: u32) -> Vec<String> {
    due_alerts(tasks, today, now, 0, lead).into_iter().filter(|a| a.hour * 60 + a.minute == now + 1).map(|a| a.id).collect()
}

/// While the app runs, the notch card reminds of a task, so a system notification would only
/// repeat it with a second sound. Withdrawn a minute ahead, it still fires when the app is not
/// running, and stays scheduled when the notch reminders are off (the caller does not call this).
pub fn withdraw_next_minute(tasks: &[Task], today: &str, now: u32, lead: u32) {
    let ids = next_minute_ids(tasks, today, now, lead);
    if ids.is_empty() || NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let ids: Vec<Retained<NSString>> = ids.iter().map(|id| NSString::from_str(id)).collect();
    UNUserNotificationCenter::currentNotificationCenter().removePendingNotificationRequestsWithIdentifiers(&NSArray::from_retained_slice(&ids));
}

fn request_then_schedule(center: &Retained<UNUserNotificationCenter>, alerts: Vec<Alert>) {
    let done = RcBlock::new({
        let center = center.clone();
        move |granted: Bool, _error: *mut NSError| {
            if granted.as_bool() {
                schedule(&center, &alerts);
            }
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &done);
}

/// Drops every pending task request, then adds `alerts`; ids are stable, so this is idempotent.
fn schedule(center: &Retained<UNUserNotificationCenter>, alerts: &[Alert]) {
    let alerts = alerts.to_vec();
    let pending = RcBlock::new({
        let center = center.clone();
        move |requests: NonNull<NSArray<UNNotificationRequest>>| {
            let stale: Vec<Retained<NSString>> = unsafe { requests.as_ref() }
                .iter()
                .map(|r| r.identifier())
                .filter(|id| id.to_string().starts_with(ID_PREFIX))
                .collect();
            center.removePendingNotificationRequestsWithIdentifiers(&NSArray::from_retained_slice(&stale));
            for alert in &alerts {
                add(&center, alert);
            }
        }
    });
    center.getPendingNotificationRequestsWithCompletionHandler(&pending);
}

fn add(center: &UNUserNotificationCenter, alert: &Alert) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&alert.title));
    content.setBody(&NSString::from_str(&alert.body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));

    // Calendar components, not a time interval, so a time-zone change still fires at wall-clock time.
    let when = NSDateComponents::new();
    let mut ymd = alert.day.split('-').filter_map(|p| p.parse::<isize>().ok());
    when.setYear(ymd.next().unwrap_or(0));
    when.setMonth(ymd.next().unwrap_or(0));
    when.setDay(ymd.next().unwrap_or(0));
    when.setHour(alert.hour as isize);
    when.setMinute(alert.minute as isize);
    let trigger = UNCalendarNotificationTrigger::triggerWithDateMatchingComponents_repeats(&when, false);

    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&alert.id), &content, Some(&trigger));
    center.addNotificationRequest_withCompletionHandler(&request, None);
}

/// `HH:MM` as (hour, minute); `None` for empty or malformed text.
fn clock(text: &str) -> Option<(u32, u32)> {
    let (h, m) = text.split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` key (Howard Hinnant's civil-date algorithm); `None` if malformed.
fn day_number(key: &str) -> Option<i64> {
    let mut parts = key.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// FNV-1a: unlike `DefaultHasher` it is specified, so ids survive toolchain upgrades.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(day: &str, time: &str, text: &str) -> Task {
        Task { day: day.into(), time: time.into(), text: text.into(), ..Task::default() }
    }

    #[test]
    fn next_minute_ids_pick_only_open_tasks_of_today_due_in_the_next_minute() {
        let mut done = task("2026-11-09", "10:01", "done");
        done.done = true;
        let tasks = [task("2026-11-09", "10:01", "soon"), task("2026-11-09", "10:02", "later"), task("2026-11-10", "10:01", "tomorrow"), done];
        let soon = due_alerts(&tasks[..1], "2026-11-09", 0, 0, 0)[0].id.clone();
        assert_eq!(next_minute_ids(&tasks, "2026-11-09", 600, 0), vec![soon]);
        assert!(next_minute_ids(&tasks, "2026-11-09", 602, 0).is_empty());
        // With a lead, the early notification of "later" is the one due at 09:52.
        let early = due_alerts(&tasks[1..2], "2026-11-09", 0, 0, 10)[0].id.clone();
        assert_eq!(next_minute_ids(&tasks, "2026-11-09", 591, 10), vec![early]);
    }

    fn ids(tasks: &[Task], now_minutes: u32) -> Vec<String> {
        due_alerts(tasks, "2026-11-09", now_minutes, 7, 0).into_iter().map(|a| a.title).collect()
    }

    #[test]
    fn skips_past_done_inbox_and_untimed() {
        let mut done = task("2026-11-10", "09:00", "done");
        done.done = true;
        let tasks = [
            task("2026-11-08", "09:00", "yesterday"),
            task("2026-11-09", "08:00", "earlier today"),
            task("2026-11-09", "10:00", "exactly now"),
            task("", "09:00", "inbox"),
            task("2026-11-10", "", "no time"),
            done,
        ];
        assert!(ids(&tasks, 600).is_empty());
    }

    #[test]
    fn keeps_later_today_and_window_edge() {
        let tasks = [
            task("2026-11-09", "18:00", "today"),
            task("2026-11-16", "00:00", "day seven"),
            task("2026-11-17", "09:00", "day eight"),
        ];
        assert_eq!(ids(&tasks, 600), ["today", "day seven"]);
    }

    #[test]
    fn window_crosses_month_and_year() {
        let alerts = due_alerts(&[task("2027-01-03", "09:00", "x")], "2026-12-30", 0, 7, 0);
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        let a = due_alerts(&[task("2026-11-10", "09:00", "gym")], "2026-11-09", 0, 7, 10);
        let b = due_alerts(&[task("2026-11-10", "09:00", "gym")], "2026-11-09", 0, 7, 10);
        let c = due_alerts(&[task("2026-11-10", "09:01", "gym")], "2026-11-09", 0, 7, 10);
        assert_eq!(a[0].id, b[0].id);
        assert_ne!(a[0].id, c[0].id);
        assert!(a[0].id.starts_with(ID_PREFIX));
    }

    fn texts(reminders: Vec<Reminder>) -> Vec<String> {
        reminders.into_iter().map(|r| r.text).collect()
    }

    #[test]
    fn reminds_ahead_and_at_the_time_once() {
        let mut done = task("2026-11-09", "18:00", "done");
        done.done = true;
        let tasks = [
            task("2026-11-09", "18:00", "gym"),
            task("2026-11-10", "18:00", "tomorrow"),
            task("2026-11-09", "", "untimed"),
            task("", "18:00", "inbox"),
            done,
        ];
        let at = |lead, after, upto| texts(reminders_between(&tasks, "2026-11-09", lead, after, upto));
        assert!(at(10, 17 * 60 + 48, 17 * 60 + 49).is_empty());
        assert_eq!(at(10, 17 * 60 + 49, 17 * 60 + 50), ["gym"]);
        assert!(at(10, 17 * 60 + 50, 17 * 60 + 59).is_empty());
        assert_eq!(at(10, 17 * 60 + 59, 18 * 60), ["gym"]);
        assert!(at(10, 18 * 60, 18 * 60 + 1).is_empty());
        // Only at the time.
        assert!(at(0, 17 * 60 + 49, 17 * 60 + 50).is_empty());
        assert_eq!(at(0, 17 * 60 + 59, 18 * 60), ["gym"]);
        assert_eq!(at(5, 17 * 60 + 54, 17 * 60 + 55), ["gym"]);
    }

    #[test]
    fn a_gap_between_checks_keeps_only_the_latest_per_task_in_time_order() {
        let tasks = [task("2026-11-09", "09:05", "b"), task("2026-11-09", "09:00", "a")];
        let due = reminders_between(&tasks, "2026-11-09", 10, 8 * 60 + 49, 9 * 60);
        assert_eq!(texts(due.clone()), ["a", "b"]);
        assert_eq!((due[0].time.as_str(), due[0].at), ("09:00", 540));
    }

    #[test]
    fn early_morning_task_has_no_lead_reminder_from_yesterday() {
        let tasks = [task("2026-11-09", "00:05", "early")];
        assert_eq!(texts(reminders_between(&tasks, "2026-11-09", 10, 0, 5)), ["early"]);
        assert_eq!(minutes("00:05"), Some(5));
        assert_eq!(minutes("bad"), None);
    }

    #[test]
    fn reminds_ahead_and_at_the_time_worded_like_the_notch_card() {
        let mut t = task("2026-11-10", "18:00", "gym");
        t.project = "Health".into();
        let alerts = due_alerts(&[t], "2026-11-09", 0, 7, 10);
        let got: Vec<_> = alerts.iter().map(|a| (a.hour, a.minute, a.body.as_str())).collect();
        assert_eq!(got, [(17, 50, "6:00 PM · in 10 min · #Health"), (18, 0, "6:00 PM · now · #Health")]);
        assert_ne!(alerts[0].id, alerts[1].id);
        // Once the early minute has passed only the at-the-time notification is left.
        assert_eq!(due_alerts(&[task("2026-11-09", "18:00", "gym")], "2026-11-09", 17 * 60 + 55, 7, 10).len(), 1);
        // No early notification on the day before.
        assert_eq!(due_alerts(&[task("2026-11-10", "00:05", "early")], "2026-11-09", 0, 7, 10).len(), 1);
    }
}
