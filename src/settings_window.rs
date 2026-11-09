//! The Settings window: a sidebar of panes and, beside it, grouped rows of switches.
//! Every switch saves the settings and reports them to the controller, which applies them live.

use std::cell::{OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSButton, NSCellImagePosition,
    NSControlStateValueOn, NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular, NSImageView,
    NSSwitch, NSTextAlignment, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};

use crate::settings::{SECTIONS, Settings};
use crate::widgets::{self as w, rgba};

const WIDTH: f64 = 640.0;
const HEIGHT: f64 = 420.0;
const SIDEBAR: f64 = 170.0;
const PANE: f64 = WIDTH - SIDEBAR;
const MARGIN: f64 = 24.0;
const ROW: f64 = 50.0;

/// Sidebar rows, top to bottom: SF Symbol and name.
const PANES: [(&str, &str); 2] = [("square.grid.2x2", "Sections"), ("rectangle.topthird.inset.filled", "Notch")];
const WINGS: [(&str, &str); 2] = [("music.note", "Show music while playing"), ("timer", "Show focus countdown")];

struct Parts {
    window: Retained<NSWindow>,
    rows: Vec<Retained<NSButton>>,
    panes: Vec<Retained<NSView>>,
}

pub struct Ivars {
    settings: RefCell<Settings>,
    on_change: Box<dyn Fn(&Settings)>,
    parts: OnceCell<Parts>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct SettingsWindow;

    impl SettingsWindow {
        #[unsafe(method(selectPane:))]
        fn select_pane(&self, sender: &NSButton) {
            self.show_pane(sender.tag() as usize);
        }

        #[unsafe(method(toggleSection:))]
        fn toggle_section(&self, sender: &NSSwitch) {
            let mut settings = self.ivars().settings.borrow().clone();
            if settings.set_shown(sender.tag() as usize, sender.state() == NSControlStateValueOn) {
                self.commit(settings);
            } else {
                // Refused: the header needs one section. The switch stays enabled and snaps back.
                sender.setState(NSControlStateValueOn);
            }
        }

        #[unsafe(method(toggleWing:))]
        fn toggle_wing(&self, sender: &NSSwitch) {
            let mut settings = self.ivars().settings.borrow().clone();
            let on = sender.state() == NSControlStateValueOn;
            if sender.tag() == 0 {
                settings.wing_music = on;
            } else {
                settings.wing_focus = on;
            }
            self.commit(settings);
        }
    }
);

impl SettingsWindow {
    pub fn new(settings: Settings, on_change: Box<dyn Fn(&Settings)>, mtm: MainThreadMarker) -> Retained<Self> {
        let ivars = Ivars { settings: RefCell::new(settings), on_change, parts: OnceCell::new() };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let parts = this.build(mtm);
        this.ivars().parts.set(parts).ok().expect("built once");
        this.show_pane(0);
        this
    }

    pub fn show(&self) {
        let window = self.window();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)] // `activate()` needs macOS 14
        NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
    }

    pub fn window(&self) -> &NSWindow {
        &self.ivars().parts.get().expect("built in new()").window
    }

    fn commit(&self, settings: Settings) {
        if let Err(err) = settings.save() {
            eprintln!("notchpad: could not save settings: {err}");
        }
        (self.ivars().on_change)(&settings);
        *self.ivars().settings.borrow_mut() = settings;
    }

    fn show_pane(&self, index: usize) {
        let parts = self.ivars().parts.get().expect("built in new()");
        for (i, (pane, row)) in parts.panes.iter().zip(&parts.rows).enumerate() {
            pane.setHidden(i != index);
            w::paint(row, &rgba(255, 255, 255, if i == index { 0.12 } else { 0.0 }), 8.0);
        }
    }

    fn build(&self, mtm: MainThreadMarker) -> Parts {
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(), NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, HEIGHT)), style, NSBackingStoreType::Buffered, false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("Notchpad Settings"));
        // The sidebar runs up behind the traffic lights.
        window.setTitlebarAppearsTransparent(true);
        window.setBackgroundColor(Some(&rgba(30, 30, 33, 1.0)));
        window.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());
        window.center();

        let content = window.contentView().expect("window has a content view");
        let sidebar = frame_view(0.0, 0.0, SIDEBAR, HEIGHT, mtm);
        w::paint(&sidebar, &rgba(255, 255, 255, 0.04), 0.0);
        content.addSubview(&sidebar);

        let target: &AnyObject = self.as_ref();
        let rows: Vec<_> = PANES.iter().enumerate().map(|(i, (symbol, name))| {
            let row = w::icon_button(symbol, name, 13.0, &rgba(255, 255, 255, 0.9), target, sel!(selectPane:), mtm);
            row.setTag(i as isize);
            row.setAlignment(NSTextAlignment::Left);
            row.setImagePosition(NSCellImagePosition::ImageLeft);
            row.setFrame(rect(10.0, from_top(HEIGHT, 52.0 + i as f64 * 34.0, 30.0), SIDEBAR - 20.0, 30.0));
            sidebar.addSubview(&row);
            row
        }).collect();

        let settings = self.ivars().settings.borrow();
        let sections: Vec<_> = SECTIONS.iter().enumerate()
            .map(|(i, s)| (s.symbol, s.name, Some(s.blurb), !settings.hidden[i], sel!(toggleSection:)))
            .collect();
        let wings: Vec<_> = WINGS.iter().zip([settings.wing_music, settings.wing_focus])
            .map(|((symbol, name), on)| (*symbol, *name, None, on, sel!(toggleWing:)))
            .collect();
        let panes = vec![self.pane("Sections", &sections, mtm), self.pane("Collapsed notch", &wings, mtm)];
        for pane in &panes {
            content.addSubview(pane);
        }
        Parts { window, rows, panes }
    }

    /// A large title over one card of switch rows: symbol, name, optional dim description.
    fn pane(&self, title: &str, rows: &[(&str, &str, Option<&str>, bool, objc2::runtime::Sel)], mtm: MainThreadMarker) -> Retained<NSView> {
        let pane = frame_view(SIDEBAR, 0.0, PANE, HEIGHT, mtm);
        let heading = w::label(title, 24.0, unsafe { NSFontWeightBold }, &rgba(255, 255, 255, 1.0), mtm);
        heading.setFrame(rect(MARGIN, from_top(HEIGHT, 36.0, 32.0), PANE - 2.0 * MARGIN, 32.0));
        pane.addSubview(&heading);

        let card_width = PANE - 2.0 * MARGIN;
        let card_height = rows.len() as f64 * ROW;
        let card = frame_view(MARGIN, from_top(HEIGHT, 84.0, card_height), card_width, card_height, mtm);
        w::paint(&card, &rgba(255, 255, 255, 0.06), 10.0);
        pane.addSubview(&card);

        let target: &AnyObject = self.as_ref();
        for (i, (symbol, name, blurb, on, action)) in rows.iter().enumerate() {
            let row = frame_view(0.0, from_top(card_height, i as f64 * ROW, ROW), card_width, ROW, mtm);
            let icon = NSImageView::imageViewWithImage(&w::symbol(symbol), mtm);
            icon.setContentTintColor(Some(&rgba(255, 255, 255, 0.85)));
            icon.setFrame(rect(14.0, (ROW - 20.0) / 2.0, 22.0, 20.0));
            row.addSubview(&icon);

            let name = w::label(name, 13.0, unsafe { NSFontWeightMedium }, &rgba(255, 255, 255, 0.95), mtm);
            let text_width = card_width - 48.0 - 66.0;
            match blurb {
                Some(blurb) => {
                    name.setFrame(rect(48.0, ROW - 9.0 - 16.0, text_width, 16.0));
                    let blurb = w::label(blurb, 11.0, unsafe { NSFontWeightRegular }, &rgba(255, 255, 255, 0.5), mtm);
                    blurb.setFrame(rect(48.0, 9.0, text_width, 14.0));
                    row.addSubview(&blurb);
                }
                None => name.setFrame(rect(48.0, (ROW - 16.0) / 2.0, text_width, 16.0)),
            }
            row.addSubview(&name);

            let switch = NSSwitch::new(mtm);
            switch.setFrame(rect(card_width - 14.0 - 38.0, (ROW - 22.0) / 2.0, 38.0, 22.0));
            switch.setState(if *on { NSControlStateValueOn } else { 0 });
            switch.setTag(i as isize);
            unsafe {
                switch.setTarget(Some(target));
                switch.setAction(Some(*action));
            }
            row.addSubview(&switch);

            if i + 1 < rows.len() {
                let rule = frame_view(48.0, 0.0, card_width - 48.0, 1.0, mtm);
                w::paint(&rule, &rgba(255, 255, 255, 0.08), 0.0);
                row.addSubview(&rule);
            }
            card.addSubview(&row);
        }
        pane
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn frame_view(x: f64, y: f64, width: f64, height: f64, mtm: MainThreadMarker) -> Retained<NSView> {
    NSView::initWithFrame(mtm.alloc(), rect(x, y, width, height))
}

/// AppKit's y runs upward; layout here reads from the top of a parent `parent` tall.
fn from_top(parent: f64, top: f64, height: f64) -> f64 {
    parent - top - height
}
