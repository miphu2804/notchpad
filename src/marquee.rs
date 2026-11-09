//! A one-line text that scrolls when it is too long for its width, the way music players show a
//! long track title: a rest at the start, one smooth pass to the left, then the text comes round
//! again. Core Animation runs the pass in the window server, so the app stays idle while it scrolls.
//!
//! The text is a `CATextLayer`, not a label: AppKit draws a layer-backed view only where it is
//! visible, so the part of a label still waiting outside the clip would scroll in blank.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSAttributedStringNSStringDrawing, NSScreen, NSView};
use objc2_foundation::{NSArray, NSAttributedString, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CAKeyframeAnimation, CAMediaTiming, CATextLayer, kCATruncationEnd, kCATruncationNone};

use crate::widgets as w;

/// Between the end of the text and its repeat.
const GAP: &str = "        ";
/// Points per second; slow enough to read along.
const SPEED: f64 = 28.0;
/// Seconds the start rests before each pass.
const REST: f64 = 2.5;
const SCROLL_KEY: &str = "marquee";

pub struct Marquee {
    /// Fixed-size box that clips the text; put this in the layout.
    pub view: Retained<NSView>,
    text_layer: Retained<CATextLayer>,
    font: Retained<NSFont>,
    color: Retained<NSColor>,
    size: NSSize,
    text: RefCell<String>,
}

impl Marquee {
    pub fn new(font: Retained<NSFont>, color: Retained<NSColor>, width: f64, mtm: MainThreadMarker) -> Self {
        let height = (font.ascender() - font.descender() + font.leading()).ceil();
        let size = NSSize::new(width, height);
        let view = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, size));
        view.setWantsLayer(true);
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        view.widthAnchor().constraintEqualToConstant(size.width).setActive(true);
        view.heightAnchor().constraintEqualToConstant(size.height).setActive(true);
        let host = view.layer().expect("layer-backed view");
        host.setMasksToBounds(true);
        let text_layer = CATextLayer::new();
        text_layer.setContentsScale(NSScreen::mainScreen(mtm).map_or(2.0, |s| s.backingScaleFactor()));
        host.addSublayer(&text_layer);
        Self { view, text_layer, font, color, size, text: RefCell::new(String::new()) }
    }

    /// Shows `text`, scrolling it only when it does not fit (and Reduce Motion is off). The same
    /// text again is a no-op, so the once-a-second music refresh never restarts a pass.
    pub fn set_text(&self, text: &str) {
        if self.text.replace(text.to_owned()) == text {
            return;
        }
        let layer = &self.text_layer;
        layer.removeAnimationForKey(&NSString::from_str(SCROLL_KEY));
        if self.styled(text).size().width <= self.size.width || w::reduce_motion() {
            unsafe {
                layer.setString(Some(&self.styled(text)));
                layer.setTruncationMode(kCATruncationEnd);
            }
            layer.setFrame(NSRect::new(NSPoint::ZERO, self.size));
            return;
        }
        // Two copies, so the second one is where the first started when the pass ends: the loop
        // back to the start is then invisible.
        let pass = self.styled(&format!("{text}{GAP}")).size().width;
        let doubled = self.styled(&format!("{text}{GAP}{text}"));
        layer.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(doubled.size().width.ceil(), self.size.height)));
        unsafe {
            layer.setTruncationMode(kCATruncationNone);
            layer.setString(Some(&doubled));
        }

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
        // Keyed, so a new text replaces this pass instead of stacking a second one.
        layer.addAnimation_forKey(&scroll, Some(&NSString::from_str(SCROLL_KEY)));
    }

    fn styled(&self, text: &str) -> Retained<NSAttributedString> {
        let attributes = unsafe {
            NSDictionary::<NSString, AnyObject>::from_slices(
                &[NSFontAttributeName, NSForegroundColorAttributeName],
                &[self.font.as_ref() as &AnyObject, self.color.as_ref() as &AnyObject],
            )
        };
        unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(text), &attributes) }
    }
}
