//! A small card that drops out of the notch to say something happened: a task is coming up, a
//! pomodoro or a break ended. It is its own non-activating panel, so it never takes the keyboard
//! from the app in use, and the notch panel underneath keeps its own state.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSBackingStoreType, NSButton, NSColor, NSFontWeightRegular,
    NSFontWeightSemibold, NSImageView, NSMainMenuWindowLevel, NSPanel, NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::panel::{self, NotchShape};
use crate::widgets::{self as w, rgba};

/// The card hugs its text and buttons, never narrower than the notch it hangs from, so no black
/// shoulders stand out beside short content.
const MAX_WIDTH: f64 = 420.0;
const BODY: f64 = 72.0;
/// Extra body height for the button row.
const ACTIONS: f64 = 34.0;
const ICON: f64 = 44.0;
const PAD: f64 = 16.0;
const TEXT_X: f64 = PAD + ICON + 14.0;
const BUTTON_HEIGHT: f64 = 26.0;

/// What a banner says. `actions` shows Snooze and Done, which only a task reminder has.
pub struct Notice<'a> {
    pub symbol: &'a str,
    pub tint: &'a NSColor,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub actions: bool,
}

pub struct NotchBanner {
    panel: Retained<NSPanel>,
    body: Retained<NSView>,
    badge: Retained<NSView>,
    icon: Retained<NSImageView>,
    title: Retained<NSTextField>,
    subtitle: Retained<NSTextField>,
    snooze: Retained<NSButton>,
    done: Retained<NSButton>,
}

impl NotchBanner {
    /// The buttons send `snooze` and `done` to `target`; clicking the card elsewhere sends `dismiss`.
    pub fn new(target: &AnyObject, snooze: Sel, done: Sel, dismiss: Sel, mtm: MainThreadMarker) -> Self {
        let panel: Retained<NSPanel> = unsafe {
            msg_send![
                NSPanel::alloc(mtm),
                initWithContentRect: NSRect::ZERO,
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: true
            ]
        };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        unsafe { panel.setReleasedWhenClosed(false) };
        // Above the notch panel, so the card reads as the notch growing.
        panel.setLevel(NSMainMenuWindowLevel + 2);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());
        // Files dropped on the card are not taken: it is gone again in a moment.
        panel.setContentView(Some(&NotchShape::new(NSRect::ZERO, Box::new(|_| {}), mtm)));

        // A borderless button under everything else, so a click anywhere on the card dismisses it.
        let body = NSView::new(mtm);
        let backdrop = unsafe { NSButton::buttonWithTitle_target_action(&NSString::new(), Some(target), Some(dismiss), mtm) };
        backdrop.setBordered(false);
        backdrop.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable);
        body.addSubview(&backdrop);

        let badge = NSView::new(mtm);
        let icon = NSImageView::new(mtm);
        icon.setContentTintColor(Some(&NSColor::whiteColor()));
        icon.setFrame(NSRect::new(NSPoint::new(10.0, 10.0), NSSize::new(ICON - 20.0, ICON - 20.0)));
        badge.addSubview(&icon);
        body.addSubview(&badge);

        let title = w::label("", 15.0, unsafe { NSFontWeightSemibold }, &NSColor::whiteColor(), mtm);
        let subtitle = w::label("", 12.5, unsafe { NSFontWeightRegular }, &rgba(255, 255, 255, 0.55), mtm);
        for field in [&title, &subtitle] {
            field.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
            body.addSubview(field);
        }

        let snooze = w::pill("Snooze 5 min", BUTTON_HEIGHT, target, snooze, mtm);
        w::style_pill(&snooze, &rgba(255, 255, 255, 0.12), &rgba(255, 255, 255, 0.9), BUTTON_HEIGHT / 2.0);
        let done = w::pill("✓  Done", BUTTON_HEIGHT, target, done, mtm);
        w::style_pill(&done, &rgba(255, 176, 32, 1.0), &NSColor::blackColor(), BUTTON_HEIGHT / 2.0);
        for button in [&snooze, &done] {
            // Placed by frame like the rest of the card.
            button.setTranslatesAutoresizingMaskIntoConstraints(true);
            body.addSubview(button);
        }
        panel.contentView().expect("content view set above").addSubview(&body);
        Self { panel, body, badge, icon, title, subtitle, snooze, done }
    }

    /// Shows `notice` hanging from `notch`, the collapsed notch frame. Replaces any card showing.
    pub fn show(&self, notch: NSRect, notice: &Notice) {
        self.title.setStringValue(&NSString::from_str(notice.title));
        self.subtitle.setStringValue(&NSString::from_str(notice.subtitle));
        let button_widths: Vec<f64> = [&self.snooze, &self.done].iter().map(|b| b.intrinsicContentSize().width + 28.0).collect();
        let buttons = if notice.actions { button_widths.iter().sum::<f64>() + 8.0 } else { 0.0 };
        let text = self.title.intrinsicContentSize().width.max(self.subtitle.intrinsicContentSize().width);
        let notch_width = notch.size.width - 2.0 * panel::ear(notch.size.height);
        let width = (TEXT_X + text.max(buttons) + PAD + 4.0).clamp(notch_width, MAX_WIDTH);
        // Icon to the end of the longer of text and buttons, centred in the card; at the cap the
        // text truncates instead.
        let content = (TEXT_X - PAD + text.max(buttons) + 4.0).min(width - 2.0 * PAD);
        let left = ((width - content) / 2.0).round();
        let text_x = left + TEXT_X - PAD;

        let body = BODY + if notice.actions { ACTIONS } else { 0.0 };
        let height = notch.size.height + body;
        let ear = panel::ear(height);
        let mid = notch.origin.x + notch.size.width / 2.0;
        let top = notch.origin.y + notch.size.height;
        self.panel.setFrame_display(NSRect::new(NSPoint::new(mid - width / 2.0 - ear, top - height), NSSize::new(width + 2.0 * ear, height)), false);
        self.body.setFrame(NSRect::new(NSPoint::new(ear, 0.0), NSSize::new(width, body)));
        if let Some(backdrop) = self.body.subviews().firstObject() {
            backdrop.setFrame(self.body.bounds());
        }

        let text_top = body - 16.0;
        let at = |view: &NSView, x: f64, y: f64, width: f64, height: f64| view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
        // The icon centres on the text block, which sits at the top when there are buttons.
        let text_mid = if notice.actions { text_top - 19.0 } else { body / 2.0 };
        at(&self.badge, left, text_mid - ICON / 2.0, ICON, ICON);
        w::paint(&self.badge, notice.tint, 12.0);
        self.icon.setImage(Some(&w::symbol(notice.symbol)));
        let text_width = width - text_x - PAD;
        at(&self.title, text_x, text_mid + 1.0, text_width, 20.0);
        at(&self.subtitle, text_x, text_mid - 18.0, text_width, 16.0);

        let mut x = text_x;
        for (button, button_width) in [&self.snooze, &self.done].into_iter().zip(button_widths) {
            button.setHidden(!notice.actions);
            at(button, x, 12.0, button_width, BUTTON_HEIGHT);
            x += button_width + 8.0;
        }

        self.panel.setAlphaValue(1.0);
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }
}
