//! One drawing view for every chart on the Insights page. `Plot` holds what to draw, already
//! reduced to levels and counts; each `draw_*` function below is only geometry.

use std::cell::RefCell;
use std::f64::consts::TAU;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSFont, NSFontAttributeName, NSFontWeightMedium, NSFontWeightSemibold, NSForegroundColorAttributeName,
    NSLineCapStyle, NSStringDrawing, NSView,
};
use objc2_foundation::{NSAttributedStringKey, NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::insights::Kind;
use crate::widgets::rgba;

pub type Rgb = (u8, u8, u8);

pub const PURPLE: Rgb = (156, 139, 232);
pub const AMBER: Rgb = (224, 162, 103);
pub const SAGE: Rgb = (111, 201, 160);
pub const PINK: Rgb = (232, 125, 160);

#[derive(Clone, PartialEq)]
pub enum Plot {
    /// Draws nothing: a flipped view, used as the scroll view's document.
    Blank,
    /// Active days this month out of the days so far, as a ring with `active/total` inside.
    Ring { active: u32, total: u32 },
    /// One petal per day, oldest first: its effort as a fraction of the busiest day's and the
    /// kind that led it (none when idle); a few dates around the ring as `(day index, text)`.
    Petals { days: Vec<(f64, Option<Kind>)>, labels: Vec<(usize, String)>, active: u32 },
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
                Plot::Petals { days, labels, active } => draw_petals(size, days, labels, *active),
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

/// Draws `s` and returns its width. `align` is 0 for text starting at `x`, 0.5 centred on it,
/// 1 ending at it.
fn text(s: &str, x: f64, y: f64, align: f64, size: f64, semibold: bool, color: &NSColor) -> f64 {
    let attrs = attributes(size, semibold, color);
    let s = NSString::from_str(s);
    let width = unsafe { s.sizeWithAttributes(Some(&attrs)) }.width;
    unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x - width * align, y), Some(&attrs)) };
    width
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

fn draw_petals(size: NSSize, days: &[(f64, Option<Kind>)], labels: &[(usize, String)], active: u32) {
    // Room for the dates: beside the ring left and right, one line above and below.
    const SIDE_LABELS: f64 = 48.0;
    const TOP_LABELS: f64 = 22.0;
    // Below this share of the busiest day a petal would be a stub, so the day stays a tinted dot.
    const SHORTEST: f64 = 0.12;
    let (cx, cy) = (size.width / 2.0, size.height / 2.0);
    let outer = (size.width / 2.0 - SIDE_LABELS).min(size.height / 2.0 - TOP_LABELS).max(1.0);
    let inner = outer * 0.5;
    let angle_of = |i: usize| TAU * i as f64 / days.len().max(1) as f64;
    // Thick petals read best, but neighbours must not touch at the base.
    let width = (TAU * inner / days.len().max(1) as f64 * 0.7).min(6.0);
    dotted_circle(cx, cy, outer);
    dotted_circle(cx, cy, (inner + outer) / 2.0);
    for (i, (length, kind)) in days.iter().enumerate() {
        let angle = angle_of(i);
        let base = on_circle(cx, cy, inner, angle);
        match kind {
            None => dot(base, 1.8, &white(0.2)),
            Some(kind) if *length < SHORTEST => dot(base, width * 0.6, &color(hue(*kind), 0.55)),
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
    let dim = white(0.4);
    // The ring runs clockwise from the oldest day at the top round to today just left of it, so
    // the top names both ends instead of a date.
    let top = on_circle(cx, cy, outer + 14.0, 0.0);
    text("start", top.x + 6.0, top.y - 7.0, 0.0, 10.0, false, &dim);
    text("today", top.x - 6.0, top.y - 7.0, 1.0, 10.0, false, &dim);
    for (i, label) in labels.iter().filter(|(i, _)| *i != 0) {
        let angle = angle_of(*i);
        let at = on_circle(cx, cy, outer + 14.0, angle);
        // Left and right labels hang off the ring; the bottom one centres on it.
        text(label, at.x, at.y - 7.0, 0.5 - 0.5 * angle.sin(), 10.0, false, &dim);
    }
    text(&format!("last {} days", days.len()), cx, cy - 27.0, 0.5, 10.0, false, &dim);
    text(&active.to_string(), cx, cy - 16.0, 0.5, 24.0, true, &white(0.95));
    text("active days", cx, cy + 13.0, 0.5, 10.0, false, &dim);
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

