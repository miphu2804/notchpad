//! The app mark: two interlinked rounded-hexagon rings (violet and amber) on a lavender tile.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSColor, NSGradient, NSGraphicsContext, NSView, NSWindingRule};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::widgets::rgba;

/// Ring geometry as fractions of the tile size.
const OUTER: f64 = 0.25;
const INNER: f64 = 0.135;
const VIOLET_CENTER: (f64, f64) = (0.41, 0.40);
const AMBER_CENTER: (f64, f64) = (0.56, 0.55);

define_class!(
    /// Drawn as vectors so it stays crisp at any size and needs no bundled image.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "LogoMark"]
    pub struct LogoMark;

    impl LogoMark {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let s = self.bounds().size.width;
            let tile = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(self.bounds(), s * 0.24, s * 0.24);
            fill(&tile, (246, 244, 252), (228, 224, 246), -90.0);

            let violet = ring(s, VIOLET_CENTER);
            let amber = ring(s, AMBER_CENTER);
            fill(&amber, (255, 203, 77), (255, 154, 46), -45.0);
            fill(&violet, (180, 126, 249), (99, 67, 240), -45.0);
            // Bring amber back in front where its lower-left side crosses violet's right side,
            // so the rings read as linked rather than stacked. Same gradient, so the clip leaves no seam.
            NSGraphicsContext::saveGraphicsState_class();
            NSBezierPath::clipRect(NSRect::new(NSPoint::new(s * 0.44, s * 0.15), NSSize::new(s * 0.31, s * 0.32)));
            fill(&amber, (255, 203, 77), (255, 154, 46), -45.0);
            NSGraphicsContext::restoreGraphicsState_class();
        }
    }
);

impl LogoMark {
    pub fn new(size: f64, mtm: MainThreadMarker) -> Retained<Self> {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size, size));
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// Hexagonal ring: outer minus inner rounded hexagon, filled even-odd so it can take a gradient.
fn ring(s: f64, (cx, cy): (f64, f64)) -> Retained<NSBezierPath> {
    let path = NSBezierPath::bezierPath();
    add_hexagon(&path, s * cx, s * cy, s * OUTER, s * 0.06);
    add_hexagon(&path, s * cx, s * cy, s * INNER, s * 0.03);
    path.setWindingRule(NSWindingRule::EvenOdd);
    path
}

/// Pointy-top hexagon of circumradius `r` with corners rounded to `corner`.
fn add_hexagon(path: &NSBezierPath, cx: f64, cy: f64, r: f64, corner: f64) {
    let vertex = |k: usize| {
        let angle = (90.0 + 60.0 * k as f64).to_radians();
        NSPoint::new(cx + r * angle.cos(), cy + r * angle.sin())
    };
    let (last, first) = (vertex(5), vertex(0));
    path.moveToPoint(NSPoint::new((last.x + first.x) / 2.0, (last.y + first.y) / 2.0));
    for k in 0..6 {
        path.appendBezierPathWithArcFromPoint_toPoint_radius(vertex(k), vertex((k + 1) % 6), corner);
    }
    path.closePath();
}

fn fill(path: &NSBezierPath, from: (u8, u8, u8), to: (u8, u8, u8), angle: f64) {
    let start: Retained<NSColor> = rgba(from.0, from.1, from.2, 1.0);
    let end: Retained<NSColor> = rgba(to.0, to.1, to.2, 1.0);
    if let Some(gradient) = NSGradient::initWithStartingColor_endingColor(NSGradient::alloc(), &start, &end) {
        gradient.drawInBezierPath_angle(path, angle);
    }
}
