//! The numbers behind the Insights view: per-day totals from the pomodoro log and the completed
//! tasks, streaks, the month's active days, the best day and heatmap intensity buckets.
//!
//! Pure: days are numbered (days since 1970-01-01) so ranges, streaks and month borders are plain
//! arithmetic, and nothing here touches AppKit or the disk.

use std::collections::HashMap;

use crate::store::Task;

/// A day counts for the heatmaps and rings by this many of its pomodoros-and-tasks points.
const POMODORO_POINTS: u32 = 2;
/// Intensity buckets above empty: a day is 0 (nothing) to `LEVELS`.
pub const LEVELS: u8 = 4;
/// The heatmap and the rings look back this many days, today included: five weeks and a day, so
/// the grid always spans 6 week columns.
pub const SPAN: i64 = 36;

/// What one calendar day held.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Day {
    pub focus_min: u32,
    pub pomodoros: u32,
    pub tasks: u32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Metric {
    Focus,
    Pomodoros,
    Tasks,
    /// Pomodoros and tasks together, for views that show effort of any kind.
    Score,
}

impl Day {
    pub fn active(&self) -> bool {
        self.focus_min > 0 || self.pomodoros > 0 || self.tasks > 0
    }

    pub fn score(&self) -> u32 {
        self.pomodoros * POMODORO_POINTS + self.tasks
    }

    pub fn value(&self, metric: Metric) -> u32 {
        match metric {
            Metric::Focus => self.focus_min,
            Metric::Pomodoros => self.pomodoros,
            Metric::Tasks => self.tasks,
            Metric::Score => self.score(),
        }
    }

    /// True when focus sessions, not finished tasks, carried the day.
    pub fn focus_led(&self) -> bool {
        self.pomodoros * POMODORO_POINTS >= self.tasks
    }
}

/// Every day that has anything in it, plus today for the "this month" and "current streak" maths.
#[derive(Default)]
pub struct Insights {
    days: HashMap<i64, Day>,
    today: i64,
}

impl Insights {
    /// `log` is `pomodoros.txt`; only `done` lines count and malformed lines are skipped.
    /// `today` is `YYYY-MM-DD`.
    pub fn build(log: &str, tasks: &[Task], today: &str) -> Self {
        let mut days: HashMap<i64, Day> = HashMap::new();
        for line in log.lines() {
            let mut cols = line.split_whitespace();
            let (Some(day), Some(_time), Some(mins), Some("done"), None) =
                (cols.next(), cols.next(), cols.next(), cols.next(), cols.next())
            else {
                continue;
            };
            let (Some(n), Ok(mins)) = (parse_day(day), mins.parse::<u32>()) else { continue };
            let entry = days.entry(n).or_default();
            entry.focus_min += mins;
            entry.pomodoros += 1;
        }
        for task in tasks.iter().filter(|t| t.done) {
            if let Some(n) = parse_day(&task.done_on) {
                days.entry(n).or_default().tasks += 1;
            }
        }
        Self { days, today: parse_day(today).unwrap_or(0) }
    }

    pub fn today(&self) -> i64 {
        self.today
    }

    pub fn day(&self, n: i64) -> Day {
        self.days.get(&n).copied().unwrap_or_default()
    }

    /// No active day at all: the view shows its empty state.
    pub fn is_empty(&self) -> bool {
        !self.days.values().any(Day::active)
    }

    /// Consecutive active days ending today, or yesterday while today is still empty.
    pub fn current_streak(&self) -> u32 {
        let mut n = if self.day(self.today).active() { self.today } else { self.today - 1 };
        let mut streak = 0;
        while self.day(n).active() {
            streak += 1;
            n -= 1;
        }
        streak
    }

    pub fn longest_streak(&self) -> u32 {
        let mut active: Vec<i64> = self.days.iter().filter(|(_, d)| d.active()).map(|(n, _)| *n).collect();
        active.sort_unstable();
        let (mut longest, mut run) = (0, 0);
        for (i, n) in active.iter().enumerate() {
            run = if i > 0 && active[i - 1] + 1 == *n { run + 1 } else { 1 };
            longest = longest.max(run);
        }
        longest
    }

    /// Active days so far this month, and the days the month has had up to and including today.
    pub fn active_this_month(&self) -> (u32, u32) {
        let (_, _, elapsed) = civil(self.today);
        let first = self.today - (elapsed as i64 - 1);
        let active = (first..=self.today).filter(|n| self.day(*n).active()).count();
        (active as u32, elapsed)
    }

    /// The day with the highest score, the latest one when scores tie; none while no day scored.
    pub fn best_day(&self) -> Option<(i64, Day)> {
        self.days.iter().map(|(n, d)| (*n, *d)).filter(|(_, d)| d.score() > 0).max_by_key(|(n, d)| (d.score(), *n))
    }

    /// The last `count` days, oldest first, today last.
    pub fn recent(&self, count: i64) -> Vec<Day> {
        (self.today - count + 1..=self.today).map(|n| self.day(n)).collect()
    }

    /// The largest value of `metric` in the last [`SPAN`] days: the top of the intensity scale.
    pub fn peak(&self, metric: Metric) -> u32 {
        self.recent(SPAN).iter().map(|d| d.value(metric)).max().unwrap_or(0)
    }
}

/// Intensity 0 to [`LEVELS`] of `value` on a scale that tops out at `peak`: 0 only for nothing,
/// otherwise at least 1, and `peak` itself is the top level.
pub fn level(value: u32, peak: u32) -> u8 {
    if value == 0 || peak == 0 {
        return 0;
    }
    (value * LEVELS as u32).div_ceil(peak).clamp(1, LEVELS as u32) as u8
}

/// `YYYY-MM-DD` as a day number; anything else, including 30 February, is `None`.
pub fn parse_day(text: &str) -> Option<i64> {
    let mut parts = text.split('-');
    let (Some(year), Some(month), Some(day), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let (year, month, day) = (year.parse::<i64>().ok()?, month.parse::<u32>().ok()?, day.parse::<u32>().ok()?);
    ((1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)).then(|| days_from_civil(year, month, day))
}

pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Year, month (1–12) and day of month of day number `n`.
pub fn civil(n: i64) -> (i64, u32, u32) {
    let z = n + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month as i64 + 9) % 12) + 2) / 5 + day as i64 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

pub const MONTH_NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/// Columns of the heatmap: [`SPAN`] days that end on any weekday touch 6 weeks.
pub const HEAT_COLS: usize = 6;

/// Column (week) and row (weekday, Monday on top) of day `n` in a heatmap of the [`SPAN`] days
/// ending `today`. Column 0 starts on the Monday on or before the first day shown.
pub fn heat_cell(today: i64, n: i64) -> (usize, u32) {
    let first = today - (SPAN - 1);
    let first_monday = first - i64::from(weekday(first));
    (((n - first_monday) / 7) as usize, weekday(n))
}

/// A label for every week shown, as (the Monday's day number, text): the day of the month,
/// with the month name in front of the first and of each week that starts in a new month.
pub fn week_labels(today: i64) -> Vec<(i64, String)> {
    let first_monday = today - (SPAN - 1) - i64::from(weekday(today - (SPAN - 1)));
    let mut previous = 0;
    (0..HEAT_COLS as i64)
        .map(|col| {
            let monday = first_monday + col * 7;
            let (_, month, day) = civil(monday);
            let text = if month == previous { day.to_string() } else { format!("{} {day}", MONTH_NAMES[month as usize - 1]) };
            previous = month;
            (monday, text)
        })
        .collect()
}

/// Monday is 0, Sunday 6.
pub fn weekday(n: i64) -> u32 {
    (n + 3).rem_euclid(7) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: &str) -> i64 {
        parse_day(day).unwrap()
    }

    fn done(day: &str) -> Task {
        Task { done: true, done_on: day.to_owned(), ..Task::default() }
    }

    fn build(log: &str, tasks: &[Task], today: &str) -> Insights {
        Insights::build(log, tasks, today)
    }

    #[test]
    fn empty_data_is_empty_with_zero_stats() {
        let data = build("", &[], "2026-11-09");
        assert!(data.is_empty());
        assert_eq!(data.current_streak(), 0);
        assert_eq!(data.longest_streak(), 0);
        assert_eq!(data.active_this_month(), (0, 9));
        assert!(data.best_day().is_none());
        assert_eq!(data.peak(Metric::Score), 0);
        assert_eq!(level(0, 0), 0);
    }

    #[test]
    fn totals_per_day_from_sessions_and_tasks() {
        let log = "2026-11-09 09:00 25 done\n2026-11-09 10:00 45 done\n2026-11-09 11:00 10 ended\n2026-11-08 09:00 25 done\n";
        let tasks = [done("2026-11-09"), done("2026-11-09"), done("2026-11-07"), Task::default()];
        let data = build(log, &tasks, "2026-11-09");
        assert_eq!(data.day(at("2026-11-09")), Day { focus_min: 70, pomodoros: 2, tasks: 2 });
        assert_eq!(data.day(at("2026-11-08")), Day { focus_min: 25, pomodoros: 1, tasks: 0 });
        assert_eq!(data.day(at("2026-11-07")), Day { focus_min: 0, pomodoros: 0, tasks: 1 });
        assert!(!data.day(at("2026-11-06")).active());
        assert!(!data.is_empty());
    }

    #[test]
    fn garbage_lines_and_dates_are_ignored() {
        let log = "\nnonsense\n2026-11-09 09:00 done\n2026-11-09 09:00 25 done extra\n2026-13-01 09:00 25 done\n\
                   2026-02-30 09:00 25 done\n2026-11-09 09:00 abc done\n2026-11-09 09:00 -5 done\n2026-11-09 09:00 25 done\n";
        let tasks = [done(""), done("someday"), Task { done_on: "2026-11-09".into(), ..Task::default() }];
        let data = build(log, &tasks, "2026-11-09");
        assert_eq!(data.day(at("2026-11-09")), Day { focus_min: 25, pomodoros: 1, tasks: 0 });
        assert_eq!(build("x", &[], "garbage").today(), 0);
    }

    #[test]
    fn streak_counts_back_from_today_or_yesterday() {
        let log = "2026-11-07 09:00 25 done\n2026-11-08 09:00 25 done\n2026-11-09 09:00 25 done\n2026-11-04 09:00 25 done\n";
        assert_eq!(build(log, &[], "2026-11-09").current_streak(), 3);
        // Today still empty: the streak is alive until midnight.
        assert_eq!(build(log, &[], "2026-11-10").current_streak(), 3);
        assert_eq!(build(log, &[], "2026-11-11").current_streak(), 0);
    }

    #[test]
    fn streak_crosses_a_month_and_year_boundary() {
        let log = "2026-12-30 09:00 25 done\n2027-01-01 09:00 25 done\n";
        let tasks = [done("2026-12-31"), done("2027-01-02")];
        let data = build(log, &tasks, "2027-01-02");
        assert_eq!(data.current_streak(), 4);
        assert_eq!(data.longest_streak(), 4);
        // February in a leap year has a 29th.
        let leap = build("2028-02-28 09:00 25 done\n2028-02-29 09:00 25 done\n2028-03-01 09:00 25 done\n", &[], "2028-03-01");
        assert_eq!(leap.current_streak(), 3);
    }

    #[test]
    fn longest_streak_may_be_older_than_the_current_one() {
        let log = "2026-10-01 09:00 25 done\n2026-10-02 09:00 25 done\n2026-10-03 09:00 25 done\n2026-10-04 09:00 25 done\n2026-11-09 09:00 25 done\n";
        let data = build(log, &[], "2026-11-09");
        assert_eq!(data.longest_streak(), 4);
        assert_eq!(data.current_streak(), 1);
    }

    #[test]
    fn active_days_this_month_ignore_other_months() {
        let log = "2026-10-31 09:00 25 done\n2026-11-01 09:00 25 done\n2026-11-01 10:00 25 done\n2026-11-05 09:00 25 done\n2026-11-12 09:00 25 done\n";
        let data = build(log, &[done("2026-11-05"), done("2026-11-08")], "2026-11-09");
        // The 1st, 5th and 8th; the 12th is in the future and 31 Oct in the past.
        assert_eq!(data.active_this_month(), (3, 9));
    }

    #[test]
    fn best_day_has_the_top_score_and_the_latest_wins_a_tie() {
        // 2 pomodoros (4) + 1 task = 5; the other days score 4 and 5.
        let log = "2026-11-01 09:00 25 done\n2026-11-01 10:00 25 done\n2026-11-02 09:00 25 done\n2026-11-02 10:00 25 done\n2026-11-03 09:00 25 done\n";
        let tasks = [done("2026-11-01"), done("2026-11-03"), done("2026-11-03"), done("2026-11-03")];
        let data = build(log, &tasks, "2026-11-09");
        let (n, day) = data.best_day().unwrap();
        assert_eq!(n, at("2026-11-03"));
        assert_eq!(day.score(), 5);
        // Focus minutes alone score nothing.
        assert!(build("", &[], "2026-11-09").best_day().is_none());
    }

    #[test]
    fn intensity_buckets_run_from_empty_to_the_top_level() {
        assert_eq!(level(0, 8), 0);
        assert_eq!(level(1, 8), 1);
        assert_eq!(level(2, 8), 1);
        assert_eq!(level(3, 8), 2);
        assert_eq!(level(4, 8), 2);
        assert_eq!(level(5, 8), 3);
        assert_eq!(level(6, 8), 3);
        assert_eq!(level(7, 8), 4);
        assert_eq!(level(8, 8), 4);
        assert_eq!(level(9, 8), 4);
        assert_eq!(level(1, 1000), 1);
    }

    #[test]
    fn peak_looks_back_one_span_only() {
        let log = "2024-01-01 09:00 90 done\n2026-11-01 09:00 25 done\n2026-11-02 09:00 50 done\n";
        let data = build(log, &[], "2026-11-09");
        assert_eq!(data.peak(Metric::Focus), 50);
        assert_eq!(data.peak(Metric::Pomodoros), 1);
        assert_eq!(data.recent(7).len(), 7);
        assert_eq!(data.recent(7)[6], Day::default());
    }

    #[test]
    fn dominant_effort_follows_points() {
        assert!(Day { focus_min: 25, pomodoros: 1, tasks: 2 }.focus_led());
        assert!(!Day { focus_min: 25, pomodoros: 1, tasks: 3 }.focus_led());
    }

    #[test]
    fn heatmap_puts_today_in_the_last_column_on_its_weekday() {
        for today in ["2026-11-09", "2026-11-15", "2027-01-01"] {
            let today = at(today);
            assert_eq!(heat_cell(today, today), (HEAT_COLS - 1, weekday(today)));
            assert_eq!(heat_cell(today, today - 6).0, HEAT_COLS - 2 + usize::from(weekday(today) == 6));
            assert_eq!(heat_cell(today, today - (SPAN - 1)).0, 0);
        }
        // A Monday is the top of its column and Sunday the bottom of the same one.
        let today = at("2026-11-09");
        assert_eq!(heat_cell(today, today).1, 0);
        assert_eq!(heat_cell(today, today - 1), (HEAT_COLS - 2, 6));
    }

    #[test]
    fn week_labels_name_the_month_when_it_changes() {
        let labels = week_labels(at("2026-11-09"));
        let text: Vec<_> = labels.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(text, ["Oct 5", "12", "19", "26", "Nov 2", "9"]);
        // Each label sits on the Monday that starts its column.
        assert!(labels.iter().enumerate().all(|(col, (monday, _))| heat_cell(at("2026-11-09"), *monday).0 == col && weekday(*monday) == 0));
    }

    #[test]
    fn calendar_maths_round_trips() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        for date in ["2026-11-09", "2028-02-29", "2000-03-01", "2026-01-01"] {
            let (y, m, d) = civil(at(date));
            assert_eq!(format!("{y:04}-{m:02}-{d:02}"), date);
        }
        // 9 November 2026 is a Monday; 1 January 1970 a Thursday.
        assert_eq!(weekday(at("2026-11-09")), 0);
        assert_eq!(weekday(0), 3);
        assert_eq!(weekday(at("2026-11-08")), 6);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert!(parse_day("2026-02-29").is_none());
        assert!(parse_day("2026-1-1").is_none());
    }
}
