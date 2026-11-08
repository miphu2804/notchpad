//! Local-calendar day keys (`YYYY-MM-DD`) and short labels (`8 Nov`), relative to today.

use objc2_foundation::{NSDate, NSDateFormatter, NSString};

pub fn key(offset_days: i64) -> String {
    format(offset_days, "yyyy-MM-dd")
}

pub fn label(offset_days: i64) -> String {
    format(offset_days, "d MMM")
}

fn format(offset_days: i64, pattern: &str) -> String {
    let date = NSDate::dateWithTimeIntervalSinceNow(offset_days as f64 * 86_400.0);
    let formatter = NSDateFormatter::new();
    formatter.setDateFormat(Some(&NSString::from_str(pattern)));
    formatter.stringFromDate(&date).to_string()
}
