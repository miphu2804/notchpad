//! The Insights section: month rings beside a two-by-two grid of stat cards, sized to fit the
//! compact notch, in a scroll view for anything shorter. All numbers come from [`Insights`]; the charts are drawn by
//! [`Chart`]. The page redraws only when `show` is given different data, so it costs nothing while idle.

use std::cell::{OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular,
    NSFontWeightSemibold, NSImageView, NSLayoutAttribute, NSLineBreakMode, NSScrollView, NSStackView, NSStackViewDistribution, NSTextAlignment, NSTextField,
    NSView,
};
use objc2_foundation::{NSObject, NSString};

use crate::days;
use crate::insights::{Day, Insights, MONTH_NAMES, SPAN, civil};
use crate::insights_chart::{AMBER, Chart, PINK, PURPLE, Plot, Rgb, SAGE};
use crate::week_bars::{self, WeekBars};
use crate::widgets::{self as w, SURFACE, rgba};

const GAP: f64 = 12.0;
const RING_CARD_WIDTH: f64 = 316.0;
/// Room for a note beside a stat card's chart; longer notes wrap onto a second line.
const NOTE_BESIDE_CHART: f64 = 66.0;
const CARD_INSET: f64 = 16.0;

struct Parts {
    body: Retained<NSView>,
    scroll: Retained<NSScrollView>,
    empty: Retained<NSStackView>,
    ring: Retained<Chart>,
    active_pct: Retained<NSTextField>,
    streak: Retained<NSTextField>,
    streak_note: Retained<NSTextField>,
    focus_time: Retained<NSTextField>,
    focus_note: Retained<NSTextField>,
    focus_bars: Retained<WeekBars>,
    best: Retained<NSTextField>,
    best_note: Retained<NSTextField>,
    petals: Retained<Chart>,
}

pub struct Ivars {
    parts: OnceCell<Parts>,
    data: RefCell<Insights>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "InsightsView"]
    pub struct InsightsView;

    impl InsightsView {}
);

impl InsightsView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let ivars = Ivars { parts: OnceCell::new(), data: RefCell::new(Insights::default()) };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let parts = this.build(mtm);
        this.ivars().parts.set(parts).ok().expect("built once");
        this
    }

    pub fn body(&self) -> &NSView {
        &self.parts().body
    }

    /// Redraws the page from `data`.
    pub fn show(&self, data: Insights) {
        *self.ivars().data.borrow_mut() = data;
        self.render();
    }

    fn parts(&self) -> &Parts {
        self.ivars().parts.get().expect("built in new()")
    }

    fn render(&self) {
        let parts = self.parts();
        let data = self.ivars().data.borrow();
        let empty = data.is_empty();
        parts.scroll.setHidden(empty);
        parts.empty.setHidden(!empty);
        if empty {
            return;
        }
        let today = data.today();

        let (active, total) = data.active_this_month();
        parts.ring.set(Plot::Ring { active, total });
        parts.active_pct.setStringValue(&NSString::from_str(&format!("{}%", 100 * active / total.max(1))));

        parts.streak.setStringValue(&NSString::from_str(&plural(data.current_streak(), "day")));
        parts.streak_note.setStringValue(&NSString::from_str(&format!("Longest {}", plural(data.longest_streak(), "day"))));

        let week = data.recent(7);
        parts.focus_time.setStringValue(&NSString::from_str(&duration(week.iter().map(|d| d.focus_min).sum())));
        parts.focus_note.setStringValue(&NSString::from_str(&format!("this week · {}", plural(week.iter().map(|d| d.tasks).sum(), "task"))));
        parts.focus_bars.set(
            std::array::from_fn(|i| week[i].focus_min),
            std::array::from_fn(|i| days::initial(i as i64 - 6)),
        );

        match data.best_day() {
            Some((n, day)) => {
                parts.best.setStringValue(&NSString::from_str(&date_label(n, today)));
                parts.best_note.setStringValue(&NSString::from_str(&format!(
                    "Score {}\n{} · {}", day.score(), plural(day.pomodoros, "pomodoro"), plural(day.tasks, "task")
                )));
            }
            None => {
                parts.best.setStringValue(&NSString::from_str("None yet"));
                parts.best_note.setStringValue(&NSString::from_str("Finish a pomodoro or a task"));
            }
        }

        let span = data.recent(SPAN);
        let effort = |d: &Day| d.effort().iter().sum::<u32>();
        let busiest = span.iter().map(effort).max().unwrap_or(0).max(1);
        let first_shown = today - (SPAN - 1);
        parts.petals.set(Plot::Petals {
            days: span.iter().map(|d| (f64::from(effort(d)) / f64::from(busiest), d.dominant())).collect(),
            // Four dates at the compass points: top, right, bottom, left.
            labels: (0..4).map(|q| q * SPAN / 4).map(|i| (i as usize, date_label(first_shown + i, today))).collect(),
            active: span.iter().filter(|d| d.active()).count() as u32,
        });
    }

    fn build(&self, mtm: MainThreadMarker) -> Parts {
        let dark = unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) };

        // Stat cards.
        let ring = Chart::new(Plot::Blank, mtm);
        size(&ring, 70.0, 70.0);
        let (active_card, active_pct, _) = stat_card("Active days", "calendar", PURPLE, Some(&ring), "this month", mtm);
        let (streak_card, streak, streak_note) = stat_card("Current streak", "flame", AMBER, None, "", mtm);
        let focus_bars = WeekBars::new(mtm);
        size(&focus_bars, week_bars::WIDTH, week_bars::HEIGHT);
        let (focus_card, focus_time, focus_note) = stat_card("Focus time", "timer", PINK, Some(&focus_bars), "this week", mtm);
        let (best_card, best, best_note) = stat_card("Best day", "star", SAGE, None, "", mtm);
        let pair = |a: &NSView, b: &NSView| {
            let row = w::stack(&[a, b], false, GAP, mtm);
            row.setDistribution(NSStackViewDistribution::FillEqually);
            row
        };
        let stats = w::stack(&[&pair(&active_card, &streak_card), &pair(&focus_card, &best_card)], true, GAP, mtm);
        stats.setAlignment(NSLayoutAttribute::Width);

        // Month rings beside the stats, as tall as their grid.
        let petals = Chart::new(Plot::Blank, mtm);
        let petals_card = card("Month rings", &petals, mtm);
        petals.setToolTip(Some(&NSString::from_str("Estimated contribution: pomodoros = min(focus minutes, sessions × 25); deep focus = remaining minutes; tasks = completed tasks × 10. Purple: mostly deep focus; amber: mostly pomodoros; green: mostly tasks. Petal length is the day against the busiest; colours show shares, not measured task time.")));
        // The ring is limited by the notch's height, so a fixed width for its card loses nothing
        // and leaves the stat cards room for their charts beside the text.
        petals_card.widthAnchor().constraintEqualToConstant(RING_CARD_WIDTH).setActive(true);
        let rows = w::stack(&[&petals_card, &stats], false, GAP, mtm);
        rows.setAlignment(NSLayoutAttribute::Height);
        let page = Chart::new(Plot::Blank, mtm);
        page.addSubview(&rows);
        rows.leadingAnchor().constraintEqualToAnchor(&page.leadingAnchor()).setActive(true);
        rows.trailingAnchor().constraintEqualToAnchor(&page.trailingAnchor()).setActive(true);
        rows.topAnchor().constraintEqualToAnchor(&page.topAnchor()).setActive(true);
        rows.bottomAnchor().constraintEqualToAnchor(&page.bottomAnchor()).setActive(true);

        let scroll = NSScrollView::new(mtm);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDocumentView(Some(&page));
        let clip = scroll.contentView();
        page.leadingAnchor().constraintEqualToAnchor(&clip.leadingAnchor()).setActive(true);
        page.trailingAnchor().constraintEqualToAnchor(&clip.trailingAnchor()).setActive(true);
        page.topAnchor().constraintEqualToAnchor(&clip.topAnchor()).setActive(true);

        let heading = w::label("No activity yet", 16.0, unsafe { NSFontWeightSemibold }, &rgba(255, 255, 255, 0.85), mtm);
        let hint = w::label("Finish a pomodoro or tick off a task and your days show up here.", 12.0, unsafe { NSFontWeightRegular }, &rgba(255, 255, 255, 0.45), mtm);
        let empty = w::stack(&[&heading, &hint], true, 6.0, mtm);
        empty.setAlignment(NSLayoutAttribute::CenterX);
        empty.setHidden(true);

        let body = NSView::new(mtm);
        body.setTranslatesAutoresizingMaskIntoConstraints(false);
        body.setAppearance(dark.as_deref());
        body.addSubview(&scroll);
        body.addSubview(&empty);
        w::pin(&scroll, &body, 0.0, Some(0.0));
        empty.centerXAnchor().constraintEqualToAnchor(&body.centerXAnchor()).setActive(true);
        empty.centerYAnchor().constraintEqualToAnchor(&body.centerYAnchor()).setActive(true);

        Parts {
            body, scroll, empty, ring, active_pct, streak, streak_note, focus_time, focus_note, focus_bars, best, best_note, petals,
        }
    }
}

fn size(view: &NSView, width: f64, height: f64) {
    view.widthAnchor().constraintEqualToConstant(width).setActive(true);
    view.heightAnchor().constraintEqualToConstant(height).setActive(true);
}

/// A rounded card with a title top-left, `accessory` top-right and `content` filling the rest.
/// A titled card whose content runs up beside the title, so a round chart gets the full height.
fn card(title: &str, content: &NSView, mtm: MainThreadMarker) -> Retained<NSView> {
    let card = surface(mtm);
    let title = w::label(title, 13.0, unsafe { NSFontWeightSemibold }, &rgba(255, 255, 255, 0.9), mtm);
    title.setTranslatesAutoresizingMaskIntoConstraints(false);
    card.addSubview(&title);
    title.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), CARD_INSET).setActive(true);
    title.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 14.0).setActive(true);
    card.addSubview(content);
    content.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), CARD_INSET).setActive(true);
    content.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -CARD_INSET).setActive(true);
    content.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 14.0).setActive(true);
    content.bottomAnchor().constraintEqualToAnchor_constant(&card.bottomAnchor(), -14.0).setActive(true);
    card
}

/// A card surface with a hairline edge.
fn surface(mtm: MainThreadMarker) -> Retained<NSView> {
    let card = w::filled(&rgba(SURFACE.0, SURFACE.1, SURFACE.2, 1.0), 18.0, mtm);
    if let Some(layer) = card.layer() {
        layer.setBorderWidth(1.0);
        layer.setBorderColor(Some(&rgba(255, 255, 255, 0.06).CGColor()));
    }
    card
}

/// A small rounded tile holding an SF Symbol in `hue`.
fn chip(symbol: &str, hue: Rgb, mtm: MainThreadMarker) -> Retained<NSView> {
    let tile = w::filled(&rgba(hue.0, hue.1, hue.2, 0.16), 8.0, mtm);
    size(&tile, 24.0, 24.0);
    let icon = NSImageView::imageViewWithImage(&w::symbol(symbol), mtm);
    icon.setContentTintColor(Some(&rgba(hue.0, hue.1, hue.2, 1.0)));
    icon.setTranslatesAutoresizingMaskIntoConstraints(false);
    tile.addSubview(&icon);
    icon.centerXAnchor().constraintEqualToAnchor(&tile.centerXAnchor()).setActive(true);
    icon.centerYAnchor().constraintEqualToAnchor(&tile.centerYAnchor()).setActive(true);
    tile
}

/// A stat card: title, big value and a note (two lines at most) down the left, `chart` bottom right.
fn stat_card(title: &str, symbol: &str, hue: Rgb, chart: Option<&NSView>, note: &str, mtm: MainThreadMarker) -> (Retained<NSView>, Retained<NSTextField>, Retained<NSTextField>) {
    let card = surface(mtm);
    card.heightAnchor().constraintEqualToConstant(124.0).setActive(true);
    let title = w::label(title, 12.0, unsafe { NSFontWeightMedium }, &rgba(255, 255, 255, 0.55), mtm);
    let heading = w::stack(&[&chip(symbol, hue, mtm), &title], false, 8.0, mtm);
    let value = w::label("", 26.0, unsafe { NSFontWeightBold }, &rgba(255, 255, 255, 0.95), mtm);
    let note = w::label(note, 11.0, unsafe { NSFontWeightRegular }, &rgba(255, 255, 255, 0.45), mtm);
    note.setMaximumNumberOfLines(2);
    note.setAlignment(NSTextAlignment::Left);
    let column = w::stack(&[&heading, &value, &note], true, 4.0, mtm);
    card.addSubview(&column);
    column.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), CARD_INSET).setActive(true);
    column.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 14.0).setActive(true);
    if let Some(chart) = chart {
        card.addSubview(chart);
        chart.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -CARD_INSET).setActive(true);
        chart.bottomAnchor().constraintEqualToAnchor_constant(&card.bottomAnchor(), -14.0).setActive(true);
        // The note shares the bottom row with the chart, so it wraps rather than running under it.
        note.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        note.setPreferredMaxLayoutWidth(NOTE_BESIDE_CHART);
        note.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&chart.leadingAnchor(), -8.0).setActive(true);
    }
    column.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&card.trailingAnchor(), -CARD_INSET).setActive(true);
    (card, value, note)
}

fn plural(count: u32, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

/// `6h 10m`, or `45m` below an hour.
fn duration(minutes: u32) -> String {
    if minutes >= 60 { format!("{}h {:02}m", minutes / 60, minutes % 60) } else { format!("{minutes}m") }
}

/// `9 Nov`, with the year added for a day outside this year.
fn date_label(n: i64, today: i64) -> String {
    let (year, month, day) = civil(n);
    let name = MONTH_NAMES[month as usize - 1];
    if year == civil(today).0 { format!("{day} {name}") } else { format!("{day} {name} {year}") }
}

#[cfg(test)]
mod tests {
    use super::{date_label, duration, plural};
    use crate::insights::parse_day;

    #[test]
    fn formats_counts_durations_and_dates() {
        assert_eq!(plural(1, "day"), "1 day");
        assert_eq!(plural(0, "task"), "0 tasks");
        assert_eq!(duration(45), "45m");
        assert_eq!(duration(370), "6h 10m");
        assert_eq!(duration(60), "1h 00m");
        let today = parse_day("2026-11-09").unwrap();
        assert_eq!(date_label(parse_day("2026-10-12").unwrap(), today), "12 Oct");
        assert_eq!(date_label(parse_day("2025-12-31").unwrap(), today), "31 Dec 2025");
    }
}
