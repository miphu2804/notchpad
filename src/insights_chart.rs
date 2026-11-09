//! One drawing view for every chart on the Insights page. `Plot` holds what to draw, already
//! reduced to levels and counts; each `draw_*` function below is only geometry.

use std::cell::RefCell;
use std::f64::consts::{PI, TAU};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSFont, NSFontAttributeName, NSFontWeightMedium, NSFontWeightSemibold, NSForegroundColorAttributeName,
    NSLineCapStyle, NSStringDrawing, NSView,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::insights::LEVELS;
use crate::widgets::rgba;

pub type Rgb = (u8, u8, u8);

pub const PURPLE: Rgb = (156, 139, 232);
pub const AMBER: Rgb = (224, 162, 103);
pub const SAGE: Rgb = (111, 201, 160);
pub const PINK: Rgb = (232, 125, 160);
/// The moons' light: a warm cream.
pub const MOON: Rgb = (232, 215, 154);
/// A finished task is drawn this many minutes tall next to focus minutes in the Pulse bars.
pub const TASK_BAR_MIN: u32 = 10;

#[derive(Clone, PartialEq)]
pub enum Plot {
    /// Draws nothing: a flipped view, used as the scroll view's document.
    Blank,
    /// Active days this month out of the days so far, as a ring with `active/total` inside.
    Ring { active: u32, total: u32 },
    /// `(column, row, level)` per day; the week labels as `(column, text)`.
    Heatmap { cells: Vec<(usize, u32, u8)>, weeks: Vec<(usize, String)>, hue: Rgb },
    /// One petal per day, oldest first: its level and whether focus (not tasks) led the day; the
    /// week labels as `(day index, text)`.
    Petals { days: Vec<(u8, bool)>, weeks: Vec<(usize, String)>, active: u32 },
    /// `(focus minutes, task height in minutes)` per day, oldest first, with x labels as `(index, text)`.
    Pulse { bars: Vec<(u32, u32)>, labels: Vec<(usize, String)> },
    /// The month as a calendar: weekday of the 1st (Monday 0), a level per day (`None` once the
    /// day has not come), and the index of today.
    Moon { lead: u32, days: Vec<Option<u8>>, today: usize },
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<Plot>]
    #[name = "InsightsChart"]
    pub struct Chart;

    impl Chart {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let size = self.bounds().size;
            match &*self.ivars().borrow() {
                Plot::Blank => {}
                Plot::Ring { active, total } => draw_ring(size, *active, *total),
                Plot::Heatmap { cells, weeks, hue } => draw_heatmap(size, cells, weeks, *hue),
                Plot::Petals { days, weeks, active } => draw_petals(size, days, weeks, *active),
                Plot::Pulse { bars, labels } => draw_pulse(size, bars, labels),
                Plot::Moon { lead, days, today } => draw_moon(size, *lead, days, *today),
            }
        }
    }
);

impl Chart {
    pub fn new(plot: Plot, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(RefCell::new(plot));
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    pub fn set(&self, plot: Plot) {
        if *self.ivars().borrow() != plot {
            *self.ivars().borrow_mut() = plot;
            self.setNeedsDisplay(true);
        }
    }
}

fn color((r, g, b): Rgb, alpha: f64) -> Retained<NSColor> {
    rgba(r, g, b, alpha)
}

fn white(alpha: f64) -> Retained<NSColor> {
    rgba(255, 255, 255, alpha)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// Empty cells are a faint white; higher levels are the hue at rising strength.
fn ramp(hue: Rgb, level: u8) -> Retained<NSColor> {
    match level {
        0 => white(0.07),
        l => color(hue, [0.28, 0.5, 0.75, 1.0][(l.min(LEVELS) - 1) as usize]),
    }
}

fn fill_round(r: NSRect, radius: f64, color: &NSColor) {
    color.setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, radius, radius).fill();
}

/// Draws `s` and returns its width. `align` is 0 for text starting at `x`, 0.5 centred on it,
/// 1 ending at it.
fn text(s: &str, x: f64, y: f64, align: f64, size: f64, semibold: bool, color: &NSColor) -> f64 {
    let weight = if semibold { unsafe { NSFontWeightSemibold } } else { unsafe { NSFontWeightMedium } };
    let font = NSFont::systemFontOfSize_weight(size, weight);
    let attrs = unsafe {
        let keys = [NSFontAttributeName, NSForegroundColorAttributeName];
        let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
        NSDictionary::from_slices(&keys, &values)
    };
    let s = NSString::from_str(s);
    let width = unsafe { s.sizeWithAttributes(Some(&attrs)) }.width;
    unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x - width * align, y), Some(&attrs)) };
    width
}

/// Points on a circle of `radius` around `(cx, cy)`; angle 0 is up and grows clockwise.
fn on_circle(cx: f64, cy: f64, radius: f64, angle: f64) -> NSPoint {
    NSPoint::new(cx + radius * angle.sin(), cy - radius * angle.cos())
}

fn draw_ring(size: NSSize, active: u32, total: u32) {
    let (cx, cy) = (size.width / 2.0, size.height / 2.0);
    let radius = size.width.min(size.height) / 2.0 - 4.0;
    let ring = NSBezierPath::bezierPathWithOvalInRect(rect(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius));
    ring.setLineWidth(6.0);
    white(0.09).setStroke();
    ring.stroke();
    let fraction = if total == 0 { 0.0 } else { (active as f64 / total as f64).min(1.0) };
    if fraction > 0.0 {
        let arc = NSBezierPath::bezierPath();
        let steps = (fraction * 90.0).ceil() as usize;
        for i in 0..=steps {
            let point = on_circle(cx, cy, radius, TAU * fraction * i as f64 / steps as f64);
            if i == 0 { arc.moveToPoint(point) } else { arc.lineToPoint(point) }
        }
        arc.setLineCapStyle(NSLineCapStyle::Round);
        // A wider, faint pass under the arc gives it a soft glow.
        arc.setLineWidth(11.0);
        color(PURPLE, 0.16).setStroke();
        arc.stroke();
        arc.setLineWidth(6.0);
        color(PURPLE, 1.0).setStroke();
        arc.stroke();
    }
    text(&format!("{active}/{total}"), cx, cy - 8.0, 0.5, 14.0, true, &white(0.95));
}

fn draw_heatmap(size: NSSize, cells: &[(usize, u32, u8)], weeks: &[(usize, String)], hue: Rgb) {
    const LABELS: f64 = 30.0;
    const TOP: f64 = 18.0;
    const LEGEND: f64 = 22.0;
    const GAP: f64 = 4.0;
    const SWATCH: f64 = 10.0;
    let cols = crate::insights::HEAT_COLS as f64;
    let pitch = ((size.width - LABELS) / cols).min((size.height - TOP - LEGEND) / 7.0).min(46.0);
    let cell = pitch - GAP;
    // Few columns leave the grid narrow; it sits in the middle of the card.
    let left = LABELS + (size.width - LABELS - cols * pitch) / 2.0;
    for (col, row, level) in cells {
        fill_round(rect(left + *col as f64 * pitch, TOP + *row as f64 * pitch, cell, cell), 5.0, &ramp(hue, *level));
    }
    if let Some((col, row, _)) = cells.last() {
        let today = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(left + *col as f64 * pitch - 1.5, TOP + *row as f64 * pitch - 1.5, cell + 3.0, cell + 3.0), 6.0, 6.0);
        today.setLineWidth(1.2);
        white(0.75).setStroke();
        today.stroke();
    }
    let dim = white(0.4);
    for (col, label) in weeks {
        text(label, left + *col as f64 * pitch + cell / 2.0, 0.0, 0.5, 10.0, false, &dim);
    }
    for (row, name) in [(0, "Mon"), (2, "Wed"), (4, "Fri")] {
        text(name, left - 8.0, TOP + row as f64 * pitch + (cell - 12.0) / 2.0, 1.0, 9.0, false, &dim);
    }
    // Legend, right-aligned under the grid: Less ▢▢▢▢▢ More.
    let y = size.height - LEGEND + 7.0;
    let right = left + cols * pitch - GAP;
    let swatches = f64::from(LEVELS + 1) * (SWATCH + 3.0);
    text("More", right, y, 1.0, 10.0, false, &dim);
    let mut x = right - 30.0 - swatches;
    for level in 0..=LEVELS {
        fill_round(rect(x, y + 1.0, SWATCH, SWATCH), 3.0, &ramp(hue, level));
        x += SWATCH + 3.0;
    }
    text("Less", right - 30.0 - swatches - 6.0, y, 1.0, 10.0, false, &dim);
}

fn draw_petals(size: NSSize, days: &[(u8, bool)], weeks: &[(usize, String)], active: u32) {
    const LEGEND: f64 = 26.0;
    const LABELS: f64 = 16.0;
    let (cx, cy) = (size.width / 2.0, (size.height - LEGEND) / 2.0);
    let outer = (size.width.min(size.height - LEGEND) / 2.0 - LABELS).max(1.0);
    let inner = outer * 0.4;
    // Faint guides at the base and the full length of a petal.
    for radius in [inner - 4.0, (inner + outer) / 2.0, outer + 3.0] {
        let guide = NSBezierPath::bezierPathWithOvalInRect(rect(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius));
        guide.setLineWidth(1.0);
        white(0.05).setStroke();
        guide.stroke();
    }
    let angle_of = |i: usize| TAU * i as f64 / days.len() as f64;
    for (i, name) in weeks {
        let at = on_circle(cx, cy, outer + LABELS - 4.0, angle_of(*i));
        text(name, at.x, at.y - 6.0, 0.5, 9.0, false, &white(0.4));
    }
    for (i, (level, focus_led)) in days.iter().enumerate() {
        let angle = angle_of(i);
        let length = 2.0 + (outer - inner - 2.0) * f64::from(*level) / f64::from(LEVELS);
        let petal = NSBezierPath::bezierPath();
        petal.moveToPoint(on_circle(cx, cy, inner, angle));
        petal.lineToPoint(on_circle(cx, cy, inner + length, angle));
        petal.setLineWidth(2.0);
        petal.setLineCapStyle(NSLineCapStyle::Round);
        if *level == 0 {
            white(0.1).setStroke();
        } else {
            color(if *focus_led { PURPLE } else { SAGE }, 0.4 + 0.6 * f64::from(*level) / f64::from(LEVELS)).setStroke();
        }
        petal.stroke();
    }
    text(&active.to_string(), cx, cy - 17.0, 0.5, 26.0, true, &white(0.95));
    text("active days", cx, cy + 11.0, 0.5, 10.0, false, &white(0.45));
    let y = size.height - LEGEND + 4.0;
    for (x, name, hue) in [(cx - 50.0, "Focus", PURPLE), (cx + 8.0, "Tasks", SAGE)] {
        fill_round(rect(x, y + 2.0, 8.0, 8.0), 4.0, &color(hue, 1.0));
        text(name, x + 13.0, y, 0.0, 10.0, false, &white(0.55));
    }
}

fn draw_pulse(size: NSSize, bars: &[(u32, u32)], labels: &[(usize, String)]) {
    const TOP: f64 = 22.0;
    const BOTTOM: f64 = 18.0;
    let area = size.height - TOP - BOTTOM;
    let top = bars.iter().map(|(focus, tasks)| focus + tasks).max().unwrap_or(0).max(TASK_BAR_MIN * 6) as f64;
    let pitch = size.width / bars.len().max(1) as f64;
    let width = pitch * 0.6;
    let base = TOP + area;
    for third in 1..=3 {
        white(if third == 0 { 0.08 } else { 0.04 }).setFill();
        NSBezierPath::fillRect(rect(0.0, base - area * f64::from(third) / 3.0, size.width, 1.0));
    }
    white(0.1).setFill();
    NSBezierPath::fillRect(rect(0.0, base, size.width, 1.0));
    for (i, (focus, tasks)) in bars.iter().enumerate() {
        let x = i as f64 * pitch + (pitch - width) / 2.0;
        let focus_h = area * f64::from(*focus) / top;
        let task_h = area * f64::from(*tasks) / top;
        if focus + tasks == 0 {
            fill_round(rect(x, base - 2.0, width, 2.0), 1.0, &white(0.12));
            continue;
        }
        fill_round(rect(x, base - focus_h, width, focus_h.max(0.0)), 2.0, &color(PURPLE, 1.0));
        fill_round(rect(x, base - focus_h - task_h - if focus_h > 0.0 { 1.0 } else { 0.0 }, width, task_h), 2.0, &color(SAGE, 1.0));
    }
    for (i, label) in labels {
        text(label, *i as f64 * pitch + pitch / 2.0, base + 4.0, 0.5, 9.0, false, &white(0.4));
    }
    let dim = white(0.55);
    let mut x = size.width;
    for (name, hue) in [(format!("Tasks ({TASK_BAR_MIN} min each)"), SAGE), ("Focus minutes".to_owned(), PURPLE)] {
        let label_width = text(&name, x, 0.0, 1.0, 10.0, false, &dim);
        fill_round(rect(x - label_width - 13.0, 2.0, 8.0, 8.0), 4.0, &color(hue, 1.0));
        x -= label_width + 28.0;
    }
}

/// A moon whose lit part grows with `level`: crescent, half, gibbous, full.
fn draw_moon_disc(cx: f64, cy: f64, radius: f64, level: u8, hue: Rgb) {
    let disc = NSBezierPath::bezierPathWithOvalInRect(rect(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius));
    white(0.09).setFill();
    disc.fill();
    if level == 0 {
        return;
    }
    if level >= 3 {
        let halo = NSBezierPath::bezierPathWithOvalInRect(rect(cx - radius - 2.5, cy - radius - 2.5, 2.0 * radius + 5.0, 2.0 * radius + 5.0));
        color(hue, 0.12).setFill();
        halo.fill();
    }
    // The lit area is bounded by the right half of the rim and a terminator ellipse; `bulge` is
    // positive for a crescent (terminator on the right) and negative for a gibbous moon.
    let lit = f64::from(level) / f64::from(LEVELS);
    let bulge = 1.0 - 2.0 * lit;
    let path = NSBezierPath::bezierPath();
    let steps = 24;
    for i in 0..=steps {
        let a = PI * i as f64 / steps as f64;
        let point = NSPoint::new(cx + radius * a.sin(), cy - radius * a.cos());
        if i == 0 { path.moveToPoint(point) } else { path.lineToPoint(point) }
    }
    for i in (0..=steps).rev() {
        let a = PI * i as f64 / steps as f64;
        path.lineToPoint(NSPoint::new(cx + bulge * radius * a.sin(), cy - radius * a.cos()));
    }
    path.closePath();
    color(hue, 1.0).setFill();
    path.fill();
}

fn draw_moon(size: NSSize, lead: u32, days: &[Option<u8>], today: usize) {
    const HEADER: f64 = 16.0;
    let rows = (lead as usize + days.len()).div_ceil(7);
    // A wide card keeps the grid compact and centred instead of spreading seven far-apart columns.
    let pitch_x = (size.width / 7.0).min(60.0);
    let left = (size.width - 7.0 * pitch_x) / 2.0;
    let pitch_y = (size.height - HEADER) / rows.max(1) as f64;
    let radius = ((pitch_x - 8.0).min(pitch_y - 18.0) / 2.0).clamp(4.0, 14.0);
    for (i, initial) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        text(initial, left + (i as f64 + 0.5) * pitch_x, 0.0, 0.5, 10.0, false, &white(0.4));
    }
    for (i, level) in days.iter().enumerate() {
        let slot = lead as usize + i;
        let (cx, top) = ((slot % 7) as f64 + 0.5, HEADER + (slot / 7) as f64 * pitch_y);
        let (cx, cy) = (left + cx * pitch_x, top + 2.0 + radius);
        match level {
            Some(level) => draw_moon_disc(cx, cy, radius, *level, MOON),
            None => {
                let ring = NSBezierPath::bezierPathWithOvalInRect(rect(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius));
                white(0.07).setStroke();
                ring.stroke();
            }
        }
        if i == today {
            let ring = NSBezierPath::bezierPathWithOvalInRect(rect(cx - radius - 3.0, cy - radius - 3.0, 2.0 * radius + 6.0, 2.0 * radius + 6.0));
            ring.setLineWidth(1.5);
            white(0.7).setStroke();
            ring.stroke();
        }
        let number = if i == today { white(0.95) } else { white(if level.is_some() { 0.45 } else { 0.25 }) };
        text(&(i + 1).to_string(), cx, cy + radius + 3.0, 0.5, 9.0, i == today, &number);
    }
}
