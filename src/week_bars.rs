//! Seven small bars for the last week's focus sessions, with weekday initials underneath.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSFont, NSFontAttributeName, NSFontWeightMedium, NSForegroundColorAttributeName, NSStringDrawing, NSView};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::insights_chart::PURPLE;
use crate::pomodoros::DAILY_GOAL;
use crate::widgets::rgba;

const BAR_WIDTH: f64 = 8.0;
const GAP: f64 = 6.0;
const BAR_AREA: f64 = 24.0;
const LABEL_AREA: f64 = 11.0;
/// Idle days keep a round dot so the week still reads as seven bars.
pub const WIDTH: f64 = 7.0 * BAR_WIDTH + 6.0 * GAP;
pub const HEIGHT: f64 = BAR_AREA + LABEL_AREA;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<([u32; 7], [char; 7])>]
    #[name = "WeekBars"]
    pub struct WeekBars;

    impl WeekBars {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let (counts, initials) = self.ivars().get();
            // A full-goal day fills the bar area; a better week rescales instead of clipping.
            let scale = counts.iter().copied().max().unwrap_or(0).max(DAILY_GOAL as u32) as f64;
            let dim = rgba(255, 255, 255, 0.16);
            let today = rgba(PURPLE.0, PURPLE.1, PURPLE.2, 0.9);
            let label_font = NSFont::systemFontOfSize_weight(8.0, unsafe { NSFontWeightMedium });
            let label_color = rgba(255, 255, 255, 0.35);
            let attrs = unsafe {
                let keys = [NSFontAttributeName, NSForegroundColorAttributeName];
                let values: [&AnyObject; 2] = [label_font.as_ref(), label_color.as_ref()];
                NSDictionary::from_slices(&keys, &values)
            };
            for (i, (&n, initial)) in counts.iter().zip(initials).enumerate() {
                let x = i as f64 * (BAR_WIDTH + GAP);
                let height = (BAR_AREA * n as f64 / scale).max(BAR_WIDTH);
                let bar = NSRect::new(NSPoint::new(x, BAR_AREA - height), NSSize::new(BAR_WIDTH, height));
                (if i == 6 { &today } else { &dim }).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bar, BAR_WIDTH / 2.0, BAR_WIDTH / 2.0).fill();

                let text = NSString::from_str(&initial.to_string());
                let size = unsafe { text.sizeWithAttributes(Some(&attrs)) };
                let at = NSPoint::new(x + (BAR_WIDTH - size.width) / 2.0, BAR_AREA + 2.0);
                unsafe { text.drawAtPoint_withAttributes(at, Some(&attrs)) };
            }
        }
    }
);

impl WeekBars {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Cell::new(([0; 7], [' '; 7])));
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    /// `counts` and `initials` run oldest day first, today last.
    pub fn set(&self, counts: [u32; 7], initials: [char; 7]) {
        if self.ivars().replace((counts, initials)) != (counts, initials) {
            self.setNeedsDisplay(true);
        }
    }
}
