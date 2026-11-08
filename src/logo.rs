//! The app mark: two interlinked hexagon rings (violet and amber) on a light rounded tile.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSColor, NSGraphicsContext, NSLineJoinStyle, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::widgets::rgba;

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
            rgba(242, 241, 245, 1.0).setFill();
            tile.fill();

            let violet = hexagon(s, 0.40, 0.40);
            let amber = hexagon(s, 0.60, 0.60);
            stroke(&amber, &rgba(245, 172, 74, 1.0));
            stroke(&violet, &rgba(132, 98, 238, 1.0));
            // Bring amber back in front where its lower-left edge crosses violet's right side,
            // so the rings read as linked rather than stacked.
            NSGraphicsContext::saveGraphicsState_class();
            let crossing = NSRect::new(NSPoint::new(s * 0.50, s * 0.22), NSSize::new(s * 0.30, s * 0.28));
            NSBezierPath::clipRect(crossing);
            stroke(&amber, &rgba(245, 172, 74, 1.0));
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

/// Pointy-top hexagon centred at (cx, cy), given as fractions of the tile size `s`.
fn hexagon(s: f64, cx: f64, cy: f64) -> Retained<NSBezierPath> {
    let path = NSBezierPath::bezierPath();
    for k in 0..6 {
        let angle = (90.0 + 60.0 * k as f64).to_radians();
        let point = NSPoint::new(s * (cx + 0.22 * angle.cos()), s * (cy + 0.22 * angle.sin()));
        if k == 0 { path.moveToPoint(point) } else { path.lineToPoint(point) }
    }
    path.closePath();
    path.setLineWidth(s * 0.11);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    path
}

fn stroke(path: &NSBezierPath, color: &NSColor) {
    color.setStroke();
    path.stroke();
}
