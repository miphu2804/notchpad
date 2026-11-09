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
use objc2_foundation::{NSAttributedStringKey, NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::insights::{KINDS, Kind, LEVELS};
use crate::widgets::rgba;

pub type Rgb = (u8, u8, u8);

pub const PURPLE: Rgb = (156, 139, 232);
pub const AMBER: Rgb = (224, 162, 103);
pub const SAGE: Rgb = (111, 201, 160);
pub const PINK: Rgb = (232, 125, 160);
/// The moons' light: a warm cream.
pub const MOON: Rgb = (232, 215, 154);

#[derive(Clone, PartialEq)]
pub enum Plot {
    /// Draws nothing: a flipped view, used as the scroll view's document.
    Blank,
    /// Active days this month out of the days so far, as a ring with `active/total` inside.
    Ring { active: u32, total: u32 },
    /// `(column, row, level)` per day; the week labels as `(column, text)`.
    Heatmap { cells: Vec<(usize, u32, u8)>, weeks: Vec<(usize, String)>, hue: Rgb },
    /// One petal per day, oldest first: its effort as a fraction of the busiest day's and the
    /// kind that led it (none when idle); a few dates around the ring as `(day index, text)`.
    Petals { days: Vec<(f64, Option<Kind>)>, labels: Vec<(usize, String)>, active: u32 },
    /// Each day's share of effort per kind, in `KINDS` order, oldest first, with x labels as
    /// `(index, text)`.
    Pulse { days: Vec<[f64; 3]>, labels: Vec<(usize, String)> },
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
                Plot::Petals { days, labels, active } => draw_petals(size, days, labels, *active),
                Plot::Pulse { days, labels } => draw_pulse(size, days, labels),
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
    let attrs = attributes(size, semibold, color);
    let s = NSString::from_str(s);
    let width = unsafe { s.sizeWithAttributes(Some(&attrs)) }.width;
    unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x - width * align, y), Some(&attrs)) };
    width
}

/// How wide [`text`] would draw `s` at `size` without semibold.
fn text_width(s: &str, size: f64) -> f64 {
    unsafe { NSString::from_str(s).sizeWithAttributes(Some(&attributes(size, false, &white(1.0)))) }.width
}

fn attributes(size: f64, semibold: bool, color: &NSColor) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let weight = if semibold { unsafe { NSFontWeightSemibold } } else { unsafe { NSFontWeightMedium } };
    let font = NSFont::systemFontOfSize_weight(size, weight);
    unsafe {
        let keys = [NSFontAttributeName, NSForegroundColorAttributeName];
        let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
        NSDictionary::from_slices(&keys, &values)
    }
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

fn draw_petals(size: NSSize, days: &[(f64, Option<Kind>)], labels: &[(usize, String)], active: u32) {
    const FOOTER: f64 = 40.0;
    const LABELS: f64 = 22.0;
    // Below this share of the busiest day a petal would be a stub, so the day stays a tinted dot.
    const SHORTEST: f64 = 0.12;
    let (cx, cy) = (size.width / 2.0, (size.height - FOOTER) / 2.0);
    let outer = (size.width.min(size.height - FOOTER) / 2.0 - LABELS).max(1.0);
    let inner = outer * 0.58;
    let angle_of = |i: usize| TAU * i as f64 / days.len().max(1) as f64;
    // Thick petals read best, but neighbours must not touch at the base.
    let width = (TAU * inner / days.len().max(1) as f64 * 0.7).min(6.0);
    dotted_circle(cx, cy, outer);
    dotted_circle(cx, cy, (inner + outer) / 2.0);
    for (i, (length, kind)) in days.iter().enumerate() {
        let angle = angle_of(i);
        let base = on_circle(cx, cy, inner, angle);
        match kind {
            None => dot(base, 1.6, &white(0.12)),
            Some(kind) if *length < SHORTEST => dot(base, width / 2.0, &color(hue(*kind), 0.4)),
            Some(kind) => {
                let petal = NSBezierPath::bezierPath();
                petal.moveToPoint(base);
                petal.lineToPoint(on_circle(cx, cy, inner + (outer - inner) * length.min(1.0), angle));
                petal.setLineWidth(width);
                petal.setLineCapStyle(NSLineCapStyle::Round);
                color(hue(*kind), 0.9).setStroke();
                petal.stroke();
            }
        }
    }
    let dim = white(0.38);
    for (i, label) in labels {
        let angle = angle_of(*i);
        let at = on_circle(cx, cy, outer + 14.0, angle);
        // Left and right labels hang off the ring; top and bottom ones centre on it.
        text(label, at.x, at.y - 7.0, 0.5 - 0.5 * angle.sin(), 10.0, false, &dim);
    }
    text(&format!("last {} days", days.len()), cx, cy - 31.0, 0.5, 10.0, false, &dim);
    text(&active.to_string(), cx, cy - 19.0, 0.5, 26.0, true, &white(0.95));
    text("active days", cx, cy + 12.0, 0.5, 10.0, false, &dim);
    let y = size.height - FOOTER + 4.0;
    legend(&[("mostly deep focus", Kind::Focus), ("mostly pomodoros", Kind::Pomodoros), ("mostly tasks", Kind::Tasks)], cx, y, 0.5);
    text("petal length = that day's contribution", cx, y + 20.0, 0.5, 10.0, false, &white(0.3));
}

/// One full-height column per day, split into each kind's share of it, so the row reads as a
/// steady rhythm and the colours tell what kind of day it was.
fn draw_pulse(size: NSSize, days: &[[f64; 3]], labels: &[(usize, String)]) {
    const TOP: f64 = 26.0;
    const BOTTOM: f64 = 20.0;
    const GAP: f64 = 2.0;
    let area = size.height - TOP - BOTTOM;
    let base = TOP + area;
    let pitch = size.width / days.len().max(1) as f64;
    let width = (pitch * 0.62).min(18.0);
    let radius = (width / 2.0).min(4.0);
    for (i, shares) in days.iter().enumerate() {
        let x = i as f64 * pitch + (pitch - width) / 2.0;
        let parts = shares.iter().filter(|s| **s > 0.0).count();
        if parts == 0 {
            fill_round(rect(x, TOP, width, area), radius, &white(0.045));
            continue;
        }
        let room = area - GAP * (parts - 1) as f64;
        let mut y = base;
        for (share, kind) in shares.iter().zip(KINDS).filter(|(s, _)| **s > 0.0) {
            let height = room * share;
            fill_round(rect(x, y - height, width, height), radius.min(height / 2.0), &color(hue(kind), 0.9));
            y -= height + GAP;
        }
    }
    for (i, label) in labels {
        text(label, *i as f64 * pitch + pitch / 2.0, base + 6.0, 0.5, 10.0, false, &white(0.38));
    }
    legend(&[("Deep focus", Kind::Focus), ("Pomodoros", Kind::Pomodoros), ("Tasks", Kind::Tasks)], size.width, 0.0, 1.0);
}

fn hue(kind: Kind) -> Rgb {
    match kind {
        Kind::Focus => PURPLE,
        Kind::Pomodoros => AMBER,
        Kind::Tasks => SAGE,
    }
}

fn dot(at: NSPoint, radius: f64, color: &NSColor) {
    color.setFill();
    NSBezierPath::bezierPathWithOvalInRect(rect(at.x - radius, at.y - radius, 2.0 * radius, 2.0 * radius)).fill();
}

/// A barely visible guide: small dots every few points around the circle.
fn dotted_circle(cx: f64, cy: f64, radius: f64) {
    let count = (TAU * radius / 6.0).round().max(1.0);
    for i in 0..count as usize {
        dot(on_circle(cx, cy, radius, TAU * i as f64 / count), 0.6, &white(0.1));
    }
}

/// A row of coloured dots with names; `align` places the row at `x` as [`text`] does.
fn legend(entries: &[(&str, Kind)], x: f64, y: f64, align: f64) {
    const DOT: f64 = 7.0;
    const SPACING: f64 = 16.0;
    let widths: Vec<f64> = entries.iter().map(|(name, _)| text_width(name, 10.0)).collect();
    let total = widths.iter().map(|w| DOT + 6.0 + w).sum::<f64>() + SPACING * (entries.len().max(1) - 1) as f64;
    let mut left = x - total * align;
    for ((name, kind), width) in entries.iter().zip(widths) {
        dot(NSPoint::new(left + DOT / 2.0, y + 7.0), DOT / 2.0, &color(hue(*kind), 0.9));
        text(name, left + DOT + 6.0, y, 0.0, 10.0, false, &white(0.5));
        left += DOT + 6.0 + width + SPACING;
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
