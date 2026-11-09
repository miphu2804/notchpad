//! A one-line label that scrolls a text too long for its width, the way music players show a long
//! track title: a rest at the start, one smooth pass to the left, then the text comes round again.
//! Core Animation runs the pass in the window server, so the app stays idle while it scrolls.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSLineBreakMode, NSTextField, NSView};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CAKeyframeAnimation, CAMediaTiming};

use crate::widgets as w;

/// Between the end of the text and its repeat.
const GAP: &str = "        ";
/// Points per second; slow enough to read along.
const SPEED: f64 = 28.0;
/// Seconds the start rests before each pass.
const REST: f64 = 2.5;

pub struct Marquee {
    /// Fixed-size box that clips the label; put this in the layout.
    pub view: Retained<NSView>,
    label: Retained<NSTextField>,
    size: NSSize,
    text: RefCell<String>,
}

impl Marquee {
    /// Wraps `label` (font and colour already set) in a `width`-wide box.
    pub fn new(label: Retained<NSTextField>, width: f64, mtm: MainThreadMarker) -> Self {
        label.setStringValue(&NSString::from_str("Ag"));
        label.sizeToFit();
        let size = NSSize::new(width, label.frame().size.height);
        let view = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, size));
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setMasksToBounds(true);
        }
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        view.widthAnchor().constraintEqualToConstant(size.width).setActive(true);
        view.heightAnchor().constraintEqualToConstant(size.height).setActive(true);
        label.setTranslatesAutoresizingMaskIntoConstraints(true);
        label.setWantsLayer(true);
        view.addSubview(&label);
        Self { view, label, size, text: RefCell::new(String::new()) }
    }

    /// Shows `text`, scrolling it only when it does not fit (and Reduce Motion is off). The same
    /// text again is a no-op, so the once-a-second music refresh never restarts a pass.
    pub fn set_text(&self, text: &str) {
        if self.text.replace(text.to_owned()) == text {
            return;
        }
        let Some(layer) = self.label.layer() else { return };
        layer.removeAllAnimations();
        if self.measure(text) <= self.size.width || w::reduce_motion() {
            self.label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            self.label.setStringValue(&NSString::from_str(text));
            self.label.setFrame(NSRect::new(NSPoint::ZERO, self.size));
            return;
        }
        // Two copies, so the second one is where the first started when the pass ends: the loop
        // back to the start is then invisible.
        let pass = self.measure(&format!("{text}{GAP}"));
        self.label.setLineBreakMode(NSLineBreakMode::ByClipping);
        self.label.setStringValue(&NSString::from_str(&format!("{text}{GAP}{text}")));
        self.label.sizeToFit();
        self.label.setFrameOrigin(NSPoint::ZERO);

        let duration = REST + pass / SPEED;
        let numbers = |values: [f64; 3]| NSArray::from_retained_slice(&values.map(NSNumber::new_f64));
        let scroll = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.translation.x")));
        let values = numbers([0.0, 0.0, -pass]);
        unsafe {
            scroll.setValues(Some(&*(Retained::as_ptr(&values) as *const NSArray<AnyObject>)));
            scroll.setKeyTimes(Some(&numbers([0.0, REST / duration, 1.0])));
        }
        scroll.setDuration(duration);
        scroll.setRepeatCount(f32::INFINITY);
        layer.addAnimation_forKey(&scroll, None);
    }

    /// Width of `text` in the label's font.
    fn measure(&self, text: &str) -> f64 {
        self.label.setStringValue(&NSString::from_str(text));
        self.label.sizeToFit();
        self.label.frame().size.width
    }
}
