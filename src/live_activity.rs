//! What the collapsed notch shows beside the hardware cut-out while something is live:
//! artwork and a waveform glyph for music, the countdown for a focus or break session.
//! The panel grows by [`WING`] on each side to make room; this view fills those wings.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSFont, NSFontWeightMedium, NSImage, NSImageScaling, NSImageView,
    NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::panel;
use crate::widgets::{self as w, rgba};

/// Width the panel gains on each side of the hardware notch.
pub const WING: f64 = 34.0;
const ART: f64 = 22.0;
/// Space between the notch's rounded corner and the artwork.
const ART_INSET: f64 = 8.0;
const FADE_SECS: f64 = 0.2;

const CORAL: (u8, u8, u8) = (217, 112, 90);
const BREAK_GREEN: (u8, u8, u8) = (120, 190, 130);

/// What is live, as the controller sees it.
pub struct Activity<'a> {
    /// `Some` while music plays; the inner image is the artwork, if the player gave one.
    pub music: Option<Option<Retained<NSImage>>>,
    /// `mm:ss` and whether it is a break, while a session runs or is paused.
    pub countdown: Option<(&'a str, bool)>,
}

impl Activity<'_> {
    pub fn is_live(&self) -> bool {
        self.music.is_some() || self.countdown.is_some()
    }
}

pub struct LiveActivity {
    pub view: Retained<NSView>,
    art: Retained<NSImageView>,
    glyph: Retained<NSImageView>,
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
        let coral = rgba(CORAL.0, CORAL.1, CORAL.2, 1.0);

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
        let glyph = NSImageView::imageViewWithImage(&w::symbol("waveform"), mtm);
        glyph.setFrame(wing_frame);
        glyph.setEditable(false);
        glyph.setContentTintColor(Some(&coral));
        glyph.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);

        // Digits keep one width, so the text does not jitter as the seconds change.
        let time = NSTextField::labelWithString(&NSString::new(), mtm);
        time.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium })));
        time.setAlignment(NSTextAlignment::Center);
        // Label height is its line height; centre it on the artwork row. Wider than the wing because the cell insets would truncate five digits.
        time.setFrame(NSRect::new(NSPoint::new(right_edge - 7.0, y + (ART - 16.0) / 2.0), NSSize::new(WING + 8.0, 16.0)));
        time.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);

        for child in [&*art, &*glyph] {
            view.addSubview(child);
        }
        view.addSubview(&time);
        Self { view, art, glyph, time }
    }

    /// Fades the wings in or out and fills them; `visible` is false while the panel is open.
    pub fn show(&self, activity: &Activity, visible: bool) {
        self.art.setHidden(activity.music.is_none());
        if let Some(artwork) = &activity.music {
            self.art.setImage(Some(&artwork.clone().unwrap_or_else(|| w::symbol("music.note"))));
        }
        self.glyph.setHidden(activity.music.is_none() || activity.countdown.is_some());
        self.time.setHidden(activity.countdown.is_none());
        if let Some((text, on_break)) = activity.countdown {
            let (r, g, b) = if on_break { BREAK_GREEN } else { CORAL };
            self.time.setStringValue(&NSString::from_str(text));
            self.time.setTextColor(Some(&rgba(r, g, b, 1.0)));
        }
        let shown = visible && activity.is_live();
        w::fade(&self.view, if shown { 1.0 } else { 0.0 }, FADE_SECS);
    }
}
