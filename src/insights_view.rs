//! The Insights page of the dashboard window: stat cards, a yearly heatmap, year rings, the last
//! month's pulse and a moon calendar, in a scroll view. All numbers come from [`Insights`]; the
//! charts are drawn by [`Chart`]. The page redraws only when `show` is given different data
//! or a heatmap tab is picked, so it costs nothing while idle.

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSApplication, NSBitmapImageFileType, NSButton, NSColor,
    NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSImageView, NSLayoutAttribute, NSModalResponseOK,
    NSSavePanel, NSScrollView, NSStackView, NSStackViewDistribution, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSArray, NSDictionary, NSObject, NSString};

use crate::days;
use crate::insights::{self, Insights, MONTH_NAMES, Metric, SPAN, civil, days_in_month, level, weekday};
use crate::insights_chart::{AMBER, Chart, PINK, PURPLE, Plot, Rgb, SAGE, TASK_BAR_MIN};
use crate::week_bars::{self, WeekBars};
use crate::widgets::{self as w, SURFACE, rgba};

/// The heatmap's tabs, in order; a tab's tag is its index.
const METRICS: [(&str, Metric, Rgb); 3] = [("Focus", Metric::Focus, PURPLE), ("Pomodoros", Metric::Pomodoros, AMBER), ("Tasks", Metric::Tasks, SAGE)];
const PULSE_DAYS: i64 = 30;
const ROW_HEIGHT: f64 = 330.0;
const GAP: f64 = 12.0;
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
    heat: Retained<Chart>,
    metric_pills: Vec<Retained<NSButton>>,
    petals: Retained<Chart>,
    pulse_card: Retained<NSView>,
    export: Retained<NSButton>,
    pulse: Retained<Chart>,
    moon_month: Retained<NSTextField>,
    moon: Retained<Chart>,
}

pub struct Ivars {
    parts: OnceCell<Parts>,
    data: RefCell<Insights>,
    metric: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "InsightsView"]
    pub struct InsightsView;

    impl InsightsView {
        #[unsafe(method(selectMetric:))]
        fn select_metric(&self, sender: &NSButton) {
            self.ivars().metric.set(sender.tag() as usize);
            self.render_heatmap();
        }

        /// Saves the Pulse card as a PNG at a path the user picks; nothing is written otherwise.
        #[unsafe(method(exportPulse:))]
        fn export_pulse(&self, _sender: &AnyObject) {
            let parts = self.parts();
            let Some(png) = png_of(&parts.pulse_card, &parts.export) else { return };
            // The dashboard may sit behind other windows, and the save sheet opens with it.
            #[allow(deprecated)] // `activate()` needs macOS 14
            NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
            let panel = NSSavePanel::savePanel(self.mtm());
            panel.setNameFieldStringValue(&NSString::from_str(&format!("notchpad-pulse-{}.png", days::key(0))));
            #[allow(deprecated)] // `setAllowedContentTypes` needs UTType
            panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str("png")])));
            if panel.runModal() != NSModalResponseOK {
                return;
            }
            let Some(path) = panel.URL().and_then(|url| url.path()) else { return };
            if !png.writeToFile_atomically(&path, true) {
                eprintln!("notchpad: could not write {path}");
            }
        }
    }
);

impl InsightsView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let ivars = Ivars { parts: OnceCell::new(), data: RefCell::new(Insights::default()), metric: Cell::new(0) };
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

        self.render_heatmap_with(&data);

        let span = data.recent(SPAN);
        let peak = data.peak(Metric::Score);
        parts.petals.set(Plot::Petals {
            days: span.iter().map(|d| (level(d.score(), peak), d.focus_led())).collect(),
            weeks: insights::week_labels(today).into_iter().map(|(monday, text)| (monday - (today - (SPAN - 1)), text)).filter(|(i, _)| (0..SPAN - 3).contains(i)).map(|(i, text)| (i as usize, text)).collect(),
            active: span.iter().filter(|d| d.active()).count() as u32,
        });

        let recent = data.recent(PULSE_DAYS);
        parts.pulse.set(Plot::Pulse {
            bars: recent.iter().map(|d| (d.focus_min, d.tasks * TASK_BAR_MIN)).collect(),
            labels: (0..recent.len()).filter(|i| i % 5 == 4).map(|i| (i, date_label(today - (PULSE_DAYS - 1 - i as i64), today))).collect(),
        });

        let (year_now, month, day_of_month) = civil(today);
        let first = today - i64::from(day_of_month - 1);
        parts.moon_month.setStringValue(&NSString::from_str(&format!("{} {year_now}", MONTH_NAMES[month as usize - 1])));
        parts.moon.set(Plot::Moon {
            lead: weekday(first),
            days: (0..i64::from(days_in_month(year_now, month))).map(|k| (first + k <= today).then(|| level(data.day(first + k).score(), peak))).collect(),
            today: day_of_month as usize - 1,
        });
    }

    fn render_heatmap(&self) {
        self.render_heatmap_with(&self.ivars().data.borrow());
    }

    fn render_heatmap_with(&self, data: &Insights) {
        let parts = self.parts();
        let selected = self.ivars().metric.get();
        let (_, metric, hue) = METRICS[selected];
        let today = data.today();
        let peak = data.peak(metric);
        let cells = (today - (SPAN - 1)..=today)
            .map(|n| {
                let (col, row) = insights::heat_cell(today, n);
                (col, row, level(data.day(n).value(metric), peak))
            })
            .collect();
        let weeks = insights::week_labels(today).into_iter().enumerate().map(|(col, (_, text))| (col, text)).collect();
        parts.heat.set(Plot::Heatmap { cells, weeks, hue });
        for (i, pill) in parts.metric_pills.iter().enumerate() {
            if i == selected {
                w::style_pill(pill, &rgba(255, 255, 255, 1.0), &NSColor::blackColor(), 12.0);
            } else {
                w::style_pill(pill, &rgba(255, 255, 255, 0.1), &rgba(255, 255, 255, 0.8), 12.0);
            }
        }
    }

    fn build(&self, mtm: MainThreadMarker) -> Parts {
        let target: &AnyObject = self.as_ref();
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
        let stats = w::stack(&[&active_card, &streak_card, &focus_card, &best_card], false, GAP, mtm);
        stats.setDistribution(NSStackViewDistribution::FillEqually);

        // Heatmap card, with the metric tabs in its title row.
        let metric_pills: Vec<_> = METRICS.iter().enumerate().map(|(i, (name, _, _))| {
            let pill = w::pill(name, 24.0, target, sel!(selectMetric:), mtm);
            pill.setTag(i as isize);
            w::fit_pill(&pill, 10.0);
            pill
        }).collect();
        let tabs = w::stack(&metric_pills.iter().map(|p| &**p as &NSView).collect::<Vec<_>>(), false, 6.0, mtm);
        let heat = Chart::new(Plot::Blank, mtm);
        let heat_card = card("Activity", Some(&tabs), &heat, mtm);

        // Rings, then the moon calendar beside the pulse below.
        let petals = Chart::new(Plot::Blank, mtm);
        let petals_card = card("Month rings", None, &petals, mtm);
        let top = w::stack(&[&heat_card, &petals_card], false, GAP, mtm);
        top.setDistribution(NSStackViewDistribution::FillEqually);
        top.heightAnchor().constraintEqualToConstant(ROW_HEIGHT).setActive(true);
        let moon_month = w::label("", 11.0, unsafe { NSFontWeightMedium }, &rgba(255, 255, 255, 0.5), mtm);
        let moon = Chart::new(Plot::Blank, mtm);
        let moon_card = card("Moon calendar", Some(&moon_month), &moon, mtm);

        // Pulse card with the export button.
        let export = w::pill("Export PNG", 24.0, target, sel!(exportPulse:), mtm);
        w::fit_pill(&export, 10.0);
        w::style_pill(&export, &rgba(255, 255, 255, 0.1), &rgba(255, 255, 255, 0.8), 12.0);
        let pulse = Chart::new(Plot::Blank, mtm);
        let pulse_card = card(&format!("Pulse · last {PULSE_DAYS} days"), Some(&export), &pulse, mtm);
        let bottom = w::stack(&[&moon_card, &pulse_card], false, GAP, mtm);
        bottom.heightAnchor().constraintEqualToConstant(ROW_HEIGHT).setActive(true);
        // The calendar needs seven narrow columns; the bars take the rest.
        moon_card.widthAnchor().constraintEqualToAnchor_multiplier(&bottom.widthAnchor(), 0.36).setActive(true);

        let rows = w::stack(&[&stats, &top, &bottom], true, GAP, mtm);
        rows.setAlignment(NSLayoutAttribute::Width);
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
            body, scroll, empty, ring, active_pct, streak, streak_note, focus_time, focus_note, focus_bars, best, best_note, heat, metric_pills,
            petals, pulse_card, export, pulse, moon_month, moon,
        }
    }
}

fn size(view: &NSView, width: f64, height: f64) {
    view.widthAnchor().constraintEqualToConstant(width).setActive(true);
    view.heightAnchor().constraintEqualToConstant(height).setActive(true);
}

/// A rounded card with a title top-left, `accessory` top-right and `content` filling the rest.
fn card(title: &str, accessory: Option<&NSView>, content: &NSView, mtm: MainThreadMarker) -> Retained<NSView> {
    let card = surface(mtm);
    let title = w::label(title, 13.0, unsafe { NSFontWeightSemibold }, &rgba(255, 255, 255, 0.9), mtm);
    title.setTranslatesAutoresizingMaskIntoConstraints(false);
    card.addSubview(&title);
    title.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), CARD_INSET).setActive(true);
    title.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 14.0).setActive(true);
    if let Some(accessory) = accessory {
        accessory.setTranslatesAutoresizingMaskIntoConstraints(false);
        card.addSubview(accessory);
        accessory.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -CARD_INSET).setActive(true);
        accessory.centerYAnchor().constraintEqualToAnchor(&title.centerYAnchor()).setActive(true);
    }
    card.addSubview(content);
    content.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), CARD_INSET).setActive(true);
    content.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -CARD_INSET).setActive(true);
    content.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 42.0).setActive(true);
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
    }
    column.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&card.trailingAnchor(), -CARD_INSET).setActive(true);
    (card, value, note)
}

/// `view` as drawn now, without `hidden`, as PNG data.
fn png_of(view: &NSView, hidden: &NSView) -> Option<Retained<objc2_foundation::NSData>> {
    let bounds = view.bounds();
    let rep = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
    hidden.setHidden(true);
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    hidden.setHidden(false);
    unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
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
