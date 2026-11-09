//! Local-calendar day keys (`YYYY-MM-DD`) and short labels (`8 Nov`), relative to today.

use objc2_foundation::{NSDate, NSDateFormatter, NSString};

pub fn key(offset_days: i64) -> String {
    format(offset_days, "yyyy-MM-dd")
}

pub fn label(offset_days: i64) -> String {
    format(offset_days, "d MMM")
}

/// `YYYY-MM-DD HH:MM` now.
pub fn stamp() -> String {
    format_at(0.0, "yyyy-MM-dd HH:mm")
}

/// `HH:MM` on the clock `seconds` from now.
pub fn clock_in(seconds: u64) -> String {
    format_at(seconds as f64, "HH:mm")
}

fn format(offset_days: i64, pattern: &str) -> String {
    format_at(offset_days as f64 * 86_400.0, pattern)
}

fn format_at(offset_seconds: f64, pattern: &str) -> String {
    let date = NSDate::dateWithTimeIntervalSinceNow(offset_seconds);
    let formatter = NSDateFormatter::new();
    formatter.setDateFormat(Some(&NSString::from_str(pattern)));
    formatter.stringFromDate(&date).to_string()
}
