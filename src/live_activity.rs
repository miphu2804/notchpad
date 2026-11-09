//! What the collapsed notch shows beside the hardware cut-out while something is live:
//! artwork and moving waveform bars for music, the countdown for a focus or break session.
//! The panel grows by [`Activity::wing`] on each side to make room; this view fills those wings.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSFont, NSFontWeightMedium, NSImage, NSImageScaling, NSImageView,
    NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, kCAMediaTimingFunctionEaseInEaseOut};

use crate::panel;
use crate::widgets::{self as w, rgba};

/// Width the panel gains on each side of the hardware notch for music.
const WING: f64 = 34.0;
/// Wider while a countdown shows: `mm:ss` doesn't fit in [`WING`] beside the hardware notch.
const TIME_WING: f64 = 50.0;
const TIME_WIDTH: f64 = 44.0;
const ART: f64 = 22.0;
/// Space between the notch's rounded corner and the artwork.
const ART_INSET: f64 = 8.0;
const FADE_SECS: f64 = 0.2;

const BAR_WIDTH: f64 = 3.0;
const BAR_GAP: f64 = 2.5;
const BAR_HEIGHT: f64 = 14.0;
/// Shortest a bar gets, as a fraction of [`BAR_HEIGHT`].
const BAR_LOW: f64 = 0.25;
/// One period per bar, none a multiple of another, so the bars never fall into step.
const BAR_PERIODS: [f64; 4] = [0.47, 0.61, 0.38, 0.54];

const CORAL: (u8, u8, u8) = (217, 112, 90);
const BREAK_GREEN: (u8, u8, u8) = (120, 190, 130);

/// What is live, as the controller sees it.
pub struct Activity<'a> {
    /// `Some` while music plays; the inner image is the artwork, if the player gave one.
    pub music: Option<Option<Retained<NSImage>>>,
    /// `mm:ss` and whether it is a break, while a session runs or is paused.
    pub countdown: Option<(&'a str, bool)>,
    /// The countdown is ASCII-styled text, which wants a fixed-pitch font.
    pub mono: bool,
}

impl Activity<'_> {
    pub fn is_live(&self) -> bool {
        self.music.is_some() || self.countdown.is_some()
    }

    /// Width the panel gains on each side of the hardware notch.
    pub fn wing(&self) -> f64 {
        if self.countdown.is_some() {
            TIME_WING
        } else if self.music.is_some() {
            WING
        } else {
            0.0
        }
    }
}

pub struct LiveActivity {
    pub view: Retained<NSView>,
    art: Retained<NSImageView>,
    bars: Retained<NSView>,
    bar_layers: Vec<Retained<CALayer>>,
    animating: Cell<bool>,
    time: Retained<NSTextField>,
}

impl LiveActivity {
    /// `collapsed` is the notch-only frame: it fixes where the wings sit, and the view then
    /// keeps its edges as the panel widens around it.
    pub fn new(collapsed: NSRect, mtm: MainThreadMarker) -> Self {
        let ear = panel::ear(collapsed.size.height);
        let view = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::ZERO, collapsed.size));
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        view.setAlphaValue(0.0);

        let notch_height = collapsed.size.height - crate::controller::TOP_SLOP;
        let y = ((notch_height - ART) / 2.0).round();
        let right_edge = collapsed.size.width - ear - WING;

        let art = NSImageView::new(mtm);
        art.setFrame(NSRect::new(NSPoint::new(ear + ART_INSET, y), NSSize::new(ART, ART)));
        art.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        art.setEditable(false);
        art.setContentTintColor(Some(&rgba(255, 255, 255, 0.85)));
        art.setWantsLayer(true);
        if let Some(layer) = art.layer() {
            layer.setCornerRadius(5.0);
            layer.setMasksToBounds(true);
        }
        art.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMaxXMargin);

        let wing_frame = NSRect::new(NSPoint::new(right_edge, y), NSSize::new(WING, ART));
        let bars = NSView::initWithFrame(mtm.alloc(), wing_frame);
        bars.setWantsLayer(true);
        bars.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        let host = bars.layer().expect("layer-backed view");
        let white = NSColor::whiteColor().CGColor();
        let left = ((WING - BAR_PERIODS.len() as f64 * (BAR_WIDTH + BAR_GAP) + BAR_GAP) / 2.0).round();
        let bar_layers: Vec<_> = (0..BAR_PERIODS.len()).map(|i| {
            let bar = CALayer::new();
            bar.setFrame(NSRect::new(
                NSPoint::new(left + i as f64 * (BAR_WIDTH + BAR_GAP), (ART - BAR_HEIGHT) / 2.0),
                NSSize::new(BAR_WIDTH, BAR_HEIGHT),
            ));
            bar.setCornerRadius(BAR_WIDTH / 2.0);
            bar.setBackgroundColor(Some(&white));
            host.addSublayer(&bar);
            bar
        }).collect();

        // Digits keep one width, so the text does not jitter as the seconds change.
        let time = NSTextField::labelWithString(&NSString::new(), mtm);
        time.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium })));
        time.setAlignment(NSTextAlignment::Right);
        // Pinned to the right edge with the artwork's inset, so the digits end clear of the
        // panel's corner and start clear of the hardware notch once the panel grows by TIME_WING.
        // Label height is its line height; centre it on the artwork row.
        let time_x = collapsed.size.width - ear - ART_INSET - TIME_WIDTH;
        time.setFrame(NSRect::new(NSPoint::new(time_x, y + (ART - 16.0) / 2.0), NSSize::new(TIME_WIDTH, 16.0)));
        time.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);

        for child in [&*art, &*bars] {
            view.addSubview(child);
        }
        view.addSubview(&time);
        Self { view, art, bars, bar_layers, animating: Cell::new(false), time }
    }

    /// Fades the wings in or out and fills them; `visible` is false while the panel is open.
    pub fn show(&self, activity: &Activity, visible: bool) {
        self.art.setHidden(activity.music.is_none());
        if let Some(artwork) = &activity.music {
            self.art.setImage(Some(&artwork.clone().unwrap_or_else(|| w::symbol("music.note"))));
        }
        let music_only = activity.music.is_some() && activity.countdown.is_none();
        self.bars.setHidden(!music_only);
        self.time.setHidden(activity.countdown.is_none());
        if let Some((text, on_break)) = activity.countdown {
            let (r, g, b) = if on_break { BREAK_GREEN } else { CORAL };
            // Digits keep one width in either font; the ASCII spinner needs the fixed-pitch one to stay in the wing.
            let font = if activity.mono {
                NSFont::monospacedSystemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })
            } else {
                NSFont::monospacedDigitSystemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium })
            };
            self.time.setFont(Some(&font));
            self.time.setStringValue(&NSString::from_str(text));
            self.time.setTextColor(Some(&rgba(r, g, b, 1.0)));
        }
        let shown = visible && activity.is_live();
        self.animate_bars(shown && music_only && !w::reduce_motion());
        w::fade(&self.view, if shown { 1.0 } else { 0.0 }, FADE_SECS);
    }

    /// The bars bounce only while they can be seen. Core Animation runs them in the window
    /// server, so the app itself stays idle; started once, not on every refresh, so the
    /// one-second focus ticks don't restart them.
    fn animate_bars(&self, on: bool) {
        if self.animating.replace(on) == on {
            return;
        }
        let ease = CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseInEaseOut });
        for (bar, period) in self.bar_layers.iter().zip(BAR_PERIODS) {
            if !on {
                bar.removeAllAnimations();
                continue;
            }
            let bounce = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.scale.y")));
            unsafe {
                bounce.setFromValue(Some(&NSNumber::new_f64(BAR_LOW)));
                bounce.setToValue(Some(&NSNumber::new_f64(1.0)));
            }
            bounce.setDuration(period);
            bounce.setAutoreverses(true);
            bounce.setRepeatCount(f32::INFINITY);
            bounce.setTimingFunction(Some(&ease));
            bar.addAnimation_forKey(&bounce, None);
        }
    }
}
