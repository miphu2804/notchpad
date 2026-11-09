//! Elapsed progress around the focus clock. Updated by the controller's existing timer.

use std::cell::Cell;
use std::f64::consts::TAU;

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSLineCapStyle, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::widgets::rgba;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<(f64, bool)>]
    pub struct FocusRing;

    impl FocusRing {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let size = self.bounds().size;
            let (fraction, on_break) = self.ivars().get();
            let (cx, cy) = (size.width / 2.0, size.height / 2.0);
            let radius = size.width.min(size.height) / 2.0 - 4.0;
            let (r, g, b) = if on_break { (111, 201, 160) } else { (232, 107, 86) };
            let rim = NSRect::new(NSPoint::new(cx - radius, cy - radius), NSSize::new(radius * 2.0, radius * 2.0));
            let track = NSBezierPath::bezierPathWithOvalInRect(rim);
            track.setLineWidth(4.0);
            rgba(r, g, b, 0.17).setStroke();
            track.stroke();
            let point = |fraction: f64| {
                let angle = TAU * fraction;
                NSPoint::new(cx + radius * angle.sin(), cy - radius * angle.cos())
            };
            let arc = NSBezierPath::bezierPath();
            arc.moveToPoint(point(0.0));
            let steps = (fraction * 120.0).ceil().max(1.0) as usize;
            for i in 1..=steps {
                arc.lineToPoint(point(fraction * i as f64 / steps as f64));
            }
            arc.setLineWidth(4.0);
            arc.setLineCapStyle(NSLineCapStyle::Round);
            rgba(r, g, b, 1.0).setStroke();
            arc.stroke();
            let end = point(fraction);
            rgba(r, g, b, 1.0).setFill();
            NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(end.x - 2.5, end.y - 2.5), NSSize::new(5.0, 5.0))).fill();
        }
    }
);

impl FocusRing {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Cell::new((0.0, false)));
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    pub fn set(&self, fraction: f64, on_break: bool) {
        let state = (fraction.clamp(0.0, 1.0), on_break);
        if self.ivars().replace(state) != state { self.setNeedsDisplay(true); }
    }
}
