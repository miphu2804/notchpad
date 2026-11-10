//! The Settings window: a sidebar of panes and, beside it, grouped rows of switches.
//! Every switch and menu saves the settings and reports them to the controller, which applies them live,
//! except Open at login: that is the system's login-item state, read and set through `SMAppService`.

use std::cell::{OnceCell, RefCell};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSButton, NSCellImagePosition,
    NSControlSize, NSControlStateValueOn, NSFont, NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular, NSImageView,
    NSPopUpButton, NSSecureTextField, NSSwitch, NSTextAlignment, NSTextField, NSView, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString, NSTimer};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::calendar::{self, Access};
use crate::reminders;
use crate::claude_mcp;
use crate::linear;
use crate::settings::{Background, FOCUS_CHOICES, LONG_BREAK_CHOICES, REMIND_CHOICES, SECTIONS, SHORT_BREAK_CHOICES, Settings, Shape, Size, TimerStyle};
use crate::widgets::{self as w, rgba};

const WIDTH: f64 = 680.0;
const HEIGHT: f64 = 470.0;
const SIDEBAR: f64 = 170.0;
const PANE: f64 = WIDTH - SIDEBAR;
const MARGIN: f64 = 24.0;
const ROW: f64 = 50.0;
const MENU_WIDTH: f64 = 120.0;

/// Sidebar rows, top to bottom: SF Symbol and name.
const PANES: [(&str, &str); 5] = [
    ("square.grid.2x2", "Sections"),
    ("rectangle.topthird.inset.filled", "Notch"),
    ("timer", "Focus"),
    ("puzzlepiece.extension", "Integrations"),
    ("gearshape", "General"),
];
/// Rows of the Focus pane, in order; their tags in `setFocusOption:`.
const FOCUS_ROW: isize = 0;
const SHORT_BREAK_ROW: isize = 1;
const LONG_BREAK_ROW: isize = 2;
const REMIND_ROW: isize = 3;
const STYLE_ROW: isize = 4;
/// Rows of the Integrations pane, in order; they are the tags of their controls (not below 10: labels and
/// image views default to tag 0), and their blurbs carry `STATUS + row` so the actions can rewrite them.
const CLAUDE_CODE_ROW: isize = 10;
const CLAUDE_DESKTOP_ROW: isize = 11;
const CALENDAR_ROW: isize = 12;
const REMINDERS_ROW: isize = 13;
const LINEAR_ROW: isize = 14;
/// Generic secure-field tags used by integration rows that still have a field.
const FIELD: isize = 200;
const STATUS: isize = 100;
const BUTTON_WIDTH: f64 = 90.0;
const FIELD_WIDTH: f64 = 120.0;
/// Index of the Integrations pane in [`PANES`].
const INTEGRATIONS_PANE: usize = 3;

/// One settings row: symbol, name, optional dim description, and its controls, laid out right to
/// left: a button (title, action), a switch (state, action) and left of those a menu (items,
/// selected index, action).
#[derive(Default)]
struct Row<'a> {
    symbol: &'a str,
    name: &'a str,
    blurb: Option<&'a str>,
    switch: Option<(bool, Sel)>,
    menu: Option<(Vec<String>, usize, Sel)>,
    button: Option<(&'a str, Sel)>,
    /// Placeholder of a secure text field, left of the button.
    field: Option<&'a str>,
}
const WINGS: [(&str, &str); 2] = [("play.circle", "Show what's playing"), ("timer", "Show focus countdown")];

struct Parts {
    window: Retained<NSWindow>,
    rows: Vec<Retained<NSButton>>,
    panes: Vec<Retained<NSView>>,
}

pub struct Ivars {
    settings: RefCell<Settings>,
    on_change: Box<dyn Fn(&Settings)>,
    on_insights: Box<dyn Fn()>,
    parts: OnceCell<Parts>,
    linear_oauth_pending: RefCell<Option<linear::OAuthAttempt>>,
    linear_oauth_timer: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct SettingsWindow;

    impl SettingsWindow {
        #[unsafe(method(openInsights:))]
        fn open_insights(&self, _sender: &NSButton) {
            self.window().orderOut(None);
            (self.ivars().on_insights)();
        }

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

        #[unsafe(method(setSize:))]
        fn set_size(&self, sender: &NSPopUpButton) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.sizes[sender.tag() as usize] = Size::ALL[sender.indexOfSelectedItem() as usize];
            self.commit(settings);
        }

        #[unsafe(method(setShape:))]
        fn set_shape(&self, sender: &NSPopUpButton) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.shape = Shape::ALL[sender.indexOfSelectedItem().max(0) as usize];
            self.commit(settings);
        }

        #[unsafe(method(setBackground:))]
        fn set_background(&self, sender: &NSPopUpButton) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.background = Background::ALL[sender.indexOfSelectedItem().max(0) as usize];
            self.commit(settings);
        }

        #[unsafe(method(setFocusOption:))]
        fn set_focus_option(&self, sender: &NSPopUpButton) {
            let mut settings = self.ivars().settings.borrow().clone();
            let index = sender.indexOfSelectedItem().max(0) as usize;
            match sender.tag() {
                FOCUS_ROW => settings.focus = FOCUS_CHOICES[index],
                SHORT_BREAK_ROW => settings.short_break = SHORT_BREAK_CHOICES[index],
                LONG_BREAK_ROW => settings.long_break = LONG_BREAK_CHOICES[index],
                REMIND_ROW => settings.remind = REMIND_CHOICES[index],
                STYLE_ROW => settings.timer_style = TimerStyle::ALL[index],
                _ => return,
            }
            self.commit(settings);
        }

        #[unsafe(method(toggleSound:))]
        fn toggle_sound(&self, sender: &NSSwitch) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.sound = sender.state() == NSControlStateValueOn;
            self.commit(settings);
        }

        #[unsafe(method(toggleLyrics:))]
        fn toggle_lyrics(&self, sender: &NSSwitch) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.lyrics = sender.state() == NSControlStateValueOn;
            self.commit(settings);
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

        #[unsafe(method(addClaudeCode:))]
        fn add_claude_code(&self, _sender: &NSButton) {
            let error = claude_mcp::add_to_code().err();
            self.refresh_integrations(CLAUDE_CODE_ROW, error);
        }

        #[unsafe(method(addClaudeDesktop:))]
        fn add_claude_desktop(&self, _sender: &NSButton) {
            let error = claude_mcp::add_to_desktop().err();
            self.refresh_integrations(CLAUDE_DESKTOP_ROW, error);
        }

        /// Starts OAuth (or, with no OAuth client configured, connects the pasted personal key),
        /// cancels a pending browser flow, or disconnects. Credentials go only to the Keychain.
        #[unsafe(method(toggleLinear:))]
        fn toggle_linear(&self, _sender: &NSButton) {
            if let Some(attempt) = self.ivars().linear_oauth_pending.borrow_mut().take() {
                attempt.cancel();
                if let Some(timer) = self.ivars().linear_oauth_timer.borrow_mut().take() {
                    timer.invalidate();
                }
                return self.refresh_integrations(LINEAR_ROW, Some("Linear authorization canceled".to_owned()));
            }
            let mut settings = self.ivars().settings.borrow().clone();
            if settings.linear {
                linear::disconnect();
                settings.linear = false;
                self.commit(settings);
                return self.refresh_integrations(-1, None);
            }
            if linear::oauth_client_id().is_none() {
                let pane = &self.ivars().parts.get().expect("built in new()").panes[INTEGRATIONS_PANE];
                let field = pane.viewWithTag(FIELD + LINEAR_ROW).and_then(|v| v.downcast::<NSTextField>().ok());
                let key = field.as_ref().map(|f| f.stringValue().to_string()).unwrap_or_default();
                if key.trim().is_empty() {
                    let hint = linear::open_key_page().map_or_else(|e| e, |()| "Paste the new key here, then Connect".to_owned());
                    return self.refresh_integrations(LINEAR_ROW, Some(hint));
                }
                if let Err(error) = linear::connect_key(key.trim()) {
                    return self.refresh_integrations(LINEAR_ROW, Some(error));
                }
                if let Some(field) = field {
                    field.setStringValue(&NSString::new());
                }
                settings.linear = true;
                self.commit(settings);
                return self.refresh_integrations(-1, None);
            }
            match linear::start_oauth() {
                Ok(attempt) => {
                    *self.ivars().linear_oauth_pending.borrow_mut() = Some(attempt);
                    let timer = unsafe {
                        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                            0.1, self.as_ref(), sel!(pollLinearOAuth:), None, true,
                        )
                    };
                    *self.ivars().linear_oauth_timer.borrow_mut() = Some(timer);
                    self.refresh_integrations(-1, None);
                }
                Err(error) => self.refresh_integrations(LINEAR_ROW, Some(error)),
            }
        }

        #[unsafe(method(pollLinearOAuth:))]
        fn poll_linear_oauth(&self, _timer: &NSTimer) {
            let result = self.ivars().linear_oauth_pending.borrow().as_ref().and_then(linear::OAuthAttempt::try_result);
            let Some(result) = result else { return };
            self.ivars().linear_oauth_pending.borrow_mut().take();
            if let Some(timer) = self.ivars().linear_oauth_timer.borrow_mut().take() {
                timer.invalidate();
            }
            match result {
                Ok(()) => {
                    let mut settings = self.ivars().settings.borrow().clone();
                    settings.linear = true;
                    self.commit(settings);
                    self.refresh_integrations(-1, None);
                }
                Err(error) => self.refresh_integrations(LINEAR_ROW, Some(error)),
            }
        }

        #[unsafe(method(toggleCalendar:))]
        fn toggle_calendar(&self, sender: &NSSwitch) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.calendar = sender.state() == NSControlStateValueOn;
            self.commit(settings);
            self.refresh_integrations(-1, None);
        }

        #[unsafe(method(toggleReminders:))]
        fn toggle_reminders(&self, sender: &NSSwitch) {
            let mut settings = self.ivars().settings.borrow().clone();
            settings.reminders = sender.state() == NSControlStateValueOn;
            self.commit(settings);
            self.refresh_integrations(-1, None);
        }

        /// Registers or removes the app as a login item. macOS refuses for a binary outside an
        /// app bundle; the switch then snaps back to the real state.
        #[unsafe(method(toggleLogin:))]
        fn toggle_login(&self, sender: &NSSwitch) {
            let service = unsafe { SMAppService::mainAppService() };
            let result = if sender.state() == NSControlStateValueOn {
                unsafe { service.registerAndReturnError() }
            } else {
                unsafe { service.unregisterAndReturnError() }
            };
            if let Err(err) = result {
                eprintln!("notchpad: could not change the login item: {}", err.localizedDescription());
            }
            sender.setState(if opens_at_login() { NSControlStateValueOn } else { 0 });
        }
    }
);

impl SettingsWindow {
    pub fn new(settings: Settings, on_change: Box<dyn Fn(&Settings)>, on_insights: Box<dyn Fn()>, mtm: MainThreadMarker) -> Retained<Self> {
        let ivars = Ivars {
            settings: RefCell::new(settings),
            on_change,
            on_insights,
            parts: OnceCell::new(),
            linear_oauth_pending: RefCell::new(None),
            linear_oauth_timer: RefCell::new(None),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let parts = this.build(mtm);
        this.ivars().parts.set(parts).ok().expect("built once");
        this.show_pane(0);
        this.refresh_integrations(-1, None);
        this
    }

    pub fn show(&self) {
        // The user may have added the server from a terminal since the window was last open.
        self.refresh_integrations(-1, None);
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
        window.setTitle(&NSString::from_str("NotchGo Settings"));
        // The sidebar runs up behind the traffic lights. The title stays set for the Window
        // menu and accessibility but is not drawn: it would straddle the sidebar's edge.
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setBackgroundColor(Some(&rgba(30, 30, 33, 1.0)));
        window.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());
        window.center();

        let content = window.contentView().expect("window has a content view");
        let sidebar = frame_view(0.0, 0.0, SIDEBAR, HEIGHT, mtm);
        w::paint(&sidebar, &rgba(255, 255, 255, 0.04), 0.0);
        content.addSubview(&sidebar);

        let target: &AnyObject = self.as_ref();
        let rows: Vec<_> = PANES.iter().enumerate().map(|(i, (symbol, name))| {
            // AppKit puts no gap between an image and its title; the spaces add one.
            let row = w::icon_button(symbol, &format!("  {name}"), 13.0, &rgba(255, 255, 255, 0.9), target, sel!(selectPane:), mtm);
            row.setTag(i as isize);
            row.setAlignment(NSTextAlignment::Left);
            row.setImagePosition(NSCellImagePosition::ImageLeft);
            row.setFrame(rect(10.0, from_top(HEIGHT, 52.0 + i as f64 * 34.0, 30.0), SIDEBAR - 20.0, 30.0));
            sidebar.addSubview(&row);
            row
        }).collect();

        let settings = self.ivars().settings.borrow();
        let size_names = || Size::ALL.iter().map(|s| s.name().to_owned()).collect::<Vec<_>>();
        let sections: Vec<_> = SECTIONS.iter().enumerate().map(|(i, s)| Row {
            symbol: s.symbol,
            name: s.name,
            blurb: Some(s.blurb),
            switch: Some((!settings.hidden[i], sel!(toggleSection:))),
            menu: Some((size_names(), Size::ALL.iter().position(|z| *z == settings.sizes[i]).unwrap_or(0), sel!(setSize:))),
            ..Row::default()
        }).collect();
        let wings: Vec<_> = WINGS.iter().zip([settings.wing_music, settings.wing_focus])
            .map(|((symbol, name), on)| Row { symbol, name, switch: Some((on, sel!(toggleWing:))), ..Row::default() })
            .chain([
                Row {
                    symbol: "rectangle.topthird.inset.filled",
                    name: "Shape",
                    blurb: Some("Auto is a bar without a hardware notch"),
                    menu: Some((
                        Shape::ALL.iter().map(|s| s.name().to_owned()).collect(),
                        Shape::ALL.iter().position(|s| *s == settings.shape).unwrap_or(0),
                        sel!(setShape:),
                    )),
                    ..Row::default()
                },
                Row {
                    symbol: "drop",
                    name: "Background",
                    menu: Some((
                        Background::ALL.iter().map(|b| b.name().to_owned()).collect(),
                        Background::ALL.iter().position(|b| *b == settings.background).unwrap_or(0),
                        sel!(setBackground:),
                    )),
                    ..Row::default()
                },
            ])
            .collect();
        let minutes = |choices: &[u64], current: u64| {
            let items = choices.iter().map(|m| format!("{m} min")).collect();
            Some((items, choices.iter().position(|m| *m == current).unwrap_or(0), sel!(setFocusOption:)))
        };
        let remind_items = REMIND_CHOICES.iter().map(|lead| match lead {
            None => "Off".to_owned(),
            Some(0) => "At the time".to_owned(),
            Some(m) => format!("{m} min before"),
        }).collect();
        let remind_index = REMIND_CHOICES.iter().position(|r| *r == settings.remind).unwrap_or(0);
        let menu_row = |symbol, name, blurb, menu| Row { symbol, name, blurb, menu, ..Row::default() };
        let focus = [
            menu_row("timer", "Focus length", Some("Clicking the timer still cycles lengths"), minutes(&FOCUS_CHOICES, settings.focus)),
            menu_row("cup.and.saucer", "Short break", None, minutes(&SHORT_BREAK_CHOICES, settings.short_break)),
            menu_row("sofa", "Long break", Some("After every fourth pomodoro"), minutes(&LONG_BREAK_CHOICES, settings.long_break)),
            menu_row("bell", "Task reminders", Some("A card under the notch for timed tasks"), Some((remind_items, remind_index, sel!(setFocusOption:)))),
            menu_row("textformat", "Notch countdown", Some("How the time beside the notch looks"), Some((TimerStyle::ALL.iter().map(|t| t.name().to_owned()).collect(), settings.timer_style as usize, sel!(setFocusOption:)))),
            Row { symbol: "speaker.wave.2", name: "Sound", blurb: Some("Chime when a session ends or a task is due"), switch: Some((settings.sound, sel!(toggleSound:))), ..Row::default() },
        ];
        let general = [
            Row { symbol: "power", name: "Open at login", switch: Some((opens_at_login(), sel!(toggleLogin:))), ..Row::default() },
            Row {
                symbol: "quote.bubble",
                name: "Synced lyrics",
                blurb: Some("Looks up the playing song on lrclib.net"),
                switch: Some((settings.lyrics, sel!(toggleLyrics:))),
                ..Row::default()
            },
        ];
        let sections_pane = self.pane("Sections", &sections, mtm);
        let insights = [Row {
            symbol: "macwindow", name: "Insights window", blurb: Some("Insights in a larger, resizable window"),
            button: Some(("Open", sel!(openInsights:))), ..Row::default()
        }];
        sections_pane.addSubview(&self.card(&insights, 100.0 + SECTIONS.len() as f64 * ROW, 0, mtm));
        let panes = vec![
            sections_pane,
            self.pane("Notch", &wings, mtm),
            self.pane("Focus", &focus, mtm),
            self.integrations_pane(mtm),
            self.pane("General", &general, mtm),
        ];
        for pane in &panes {
            content.addSubview(pane);
        }
        Parts { window, rows, panes }
    }

    /// A large title over one card of rows.
    fn pane(&self, title: &str, rows: &[Row], mtm: MainThreadMarker) -> Retained<NSView> {
        let pane = titled_pane(title, mtm);
        pane.addSubview(&self.card(rows, 84.0, 0, mtm));
        pane
    }

    /// Titled groups of rows, each its own card under a small caption.
    fn integrations_pane(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let pane = titled_pane("Integrations", mtm);
        let claude = [
            Row { symbol: "terminal", name: "Claude Code", blurb: Some(""), button: Some(("Add", sel!(addClaudeCode:))), ..Row::default() },
            Row { symbol: "bubble.left", name: "Claude Desktop", blurb: Some(""), button: Some(("Add", sel!(addClaudeDesktop:))), ..Row::default() },
        ];
        let calendars = [
            Row {
                symbol: "calendars",
                name: "Show calendar events",
                blurb: Some(""),
                switch: Some((self.ivars().settings.borrow().calendar, sel!(toggleCalendar:))),
                ..Row::default()
            },
            Row {
                symbol: "checklist",
                name: "Sync tasks with Reminders",
                blurb: Some(""),
                switch: Some((self.ivars().settings.borrow().reminders, sel!(toggleReminders:))),
                ..Row::default()
            },
        ];
        let linear = [Row {
            symbol: "linear",
            name: "Linear",
            blurb: Some(""),
            button: Some(("Connect", sel!(toggleLinear:))),
            field: linear::oauth_client_id().is_none().then_some("Personal API key"),
            ..Row::default()
        }];
        let calendar_note = "Add a Google account in System Settings › Internet Accounts. Notion Calendar events come from that same Google calendar.";
        let (mut top, mut base) = (80.0, CLAUDE_CODE_ROW);
        for (caption, rows, note) in [("Claude", &claude[..], None), ("Calendars & Reminders", &calendars[..], Some(calendar_note)), ("Linear", &linear[..], None)] {
            let caption = w::label(&caption.to_uppercase(), 11.0, unsafe { NSFontWeightMedium }, &rgba(255, 255, 255, 0.45), mtm);
            caption.setFrame(rect(MARGIN + 4.0, from_top(HEIGHT, top, 14.0), PANE - 2.0 * MARGIN, 14.0));
            pane.addSubview(&caption);
            top += 18.0;
            pane.addSubview(&self.card(rows, top, base, mtm));
            top += rows.len() as f64 * ROW;
            base += rows.len() as isize;
            if let Some(note) = note {
                let note = NSTextField::wrappingLabelWithString(&NSString::from_str(note), mtm);
                note.setFont(Some(&NSFont::systemFontOfSize(11.0)));
                note.setTextColor(Some(&rgba(255, 255, 255, 0.45)));
                note.setFrame(rect(MARGIN + 4.0, from_top(HEIGHT, top + 6.0, 30.0), PANE - 2.0 * MARGIN - 8.0, 30.0));
                pane.addSubview(&note);
                top += 36.0;
            }
            top += 14.0;
        }
        pane
    }

    /// One card of rows whose top edge is `top` below the pane's. Each control's tag is `base` plus its row's index.
    fn card(&self, rows: &[Row], top: f64, base: isize, mtm: MainThreadMarker) -> Retained<NSView> {
        let card_width = PANE - 2.0 * MARGIN;
        let card_height = rows.len() as f64 * ROW;
        let card = frame_view(MARGIN, from_top(HEIGHT, top, card_height), card_width, card_height, mtm);
        w::paint(&card, &rgba(255, 255, 255, 0.06), 10.0);

        let target: &AnyObject = self.as_ref();
        for (i, Row { symbol, name, blurb, switch, menu, button, field }) in rows.iter().enumerate() {
            let tag = base + i as isize;
            let row = frame_view(0.0, from_top(card_height, i as f64 * ROW, ROW), card_width, ROW, mtm);
            let logo = crate::brand_logos::logo(symbol);
            let image = logo.as_ref().cloned().unwrap_or_else(|| w::symbol(symbol));
            let icon = NSImageView::imageViewWithImage(&image, mtm);
            let width = if logo.is_some() { image.size().width } else { 22.0 };
            if logo.is_none() {
                icon.setContentTintColor(Some(&rgba(255, 255, 255, 0.85)));
            }
            icon.setFrame(rect(25.0 - width / 2.0, (ROW - 22.0) / 2.0, width, 22.0));
            row.addSubview(&icon);

            // Controls are laid out right to left from their intrinsic sizes: macOS 26 draws a
            // wider switch than the classic 38×22, and a fixed frame let it overrun the menu.
            let mut controls_x = card_width - 2.0;
            if let Some((title, action)) = button {
                let button = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(target), Some(*action), mtm) };
                button.setControlSize(NSControlSize::Small);
                let height = button.intrinsicContentSize().height;
                controls_x -= 12.0 + BUTTON_WIDTH;
                button.setFrame(rect(controls_x, (ROW - height) / 2.0, BUTTON_WIDTH, height));
                button.setTag(tag);
                row.addSubview(&button);
            }
            if let Some(placeholder) = field {
                let field = NSSecureTextField::initWithFrame(mtm.alloc(), NSRect::ZERO);
                field.setControlSize(NSControlSize::Small);
                field.setFont(Some(&NSFont::systemFontOfSize(11.0)));
                field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
                let height = field.intrinsicContentSize().height;
                controls_x -= 12.0 + FIELD_WIDTH;
                field.setFrame(rect(controls_x, (ROW - height) / 2.0, FIELD_WIDTH, height));
                field.setTag(FIELD + tag);
                row.addSubview(&field);
            }
            if let Some((on, action)) = switch {
                let switch = NSSwitch::new(mtm);
                let size = switch.intrinsicContentSize();
                controls_x -= 12.0 + size.width;
                switch.setFrame(rect(controls_x, (ROW - size.height) / 2.0, size.width, size.height));
                switch.setState(if *on { NSControlStateValueOn } else { 0 });
                switch.setTag(tag);
                unsafe {
                    switch.setTarget(Some(target));
                    switch.setAction(Some(*action));
                }
                row.addSubview(&switch);
            }
            if let Some((items, selected, action)) = menu {
                let menu = NSPopUpButton::initWithFrame_pullsDown(mtm.alloc(), NSRect::ZERO, false);
                menu.setControlSize(NSControlSize::Small);
                for item in items {
                    menu.addItemWithTitle(&NSString::from_str(item));
                }
                let menu_height = menu.intrinsicContentSize().height;
                controls_x -= 12.0 + MENU_WIDTH;
                menu.setFrame(rect(controls_x, (ROW - menu_height) / 2.0, MENU_WIDTH, menu_height));
                menu.selectItemAtIndex(*selected as isize);
                menu.setTag(tag);
                unsafe {
                    menu.setTarget(Some(target));
                    menu.setAction(Some(*action));
                }
                row.addSubview(&menu);
            }

            let name = w::label(name, 13.0, unsafe { NSFontWeightMedium }, &rgba(255, 255, 255, 0.95), mtm);
            let text_width = controls_x - 12.0 - 48.0;
            match blurb {
                Some(blurb) => {
                    name.setFrame(rect(48.0, ROW - 9.0 - 16.0, text_width, 16.0));
                    let blurb = w::label(blurb, 11.0, unsafe { NSFontWeightRegular }, &rgba(255, 255, 255, 0.5), mtm);
                    blurb.setTag(STATUS + tag);
                    blurb.setFrame(rect(48.0, 9.0, text_width, 14.0));
                    row.addSubview(&blurb);
                }
                None => name.setFrame(rect(48.0, (ROW - 16.0) / 2.0, text_width, 16.0)),
            }
            row.addSubview(&name);

            if i + 1 < rows.len() {
                let rule = frame_view(48.0, 0.0, card_width - 48.0, 1.0, mtm);
                w::paint(&rule, &rgba(255, 255, 255, 0.08), 0.0);
                row.addSubview(&rule);
            }
            card.addSubview(&row);
        }
        card
    }

    /// Rewrites the Integrations rows from what is on disk and in macOS; `error` is shown on row `failed` instead of its blurb.
    fn refresh_integrations(&self, failed: isize, error: Option<String>) {
        let pane = &self.ivars().parts.get().expect("built in new()").panes[INTEGRATIONS_PANE];
        let linear_on = self.ivars().settings.borrow().linear;
        let linear_pending = self.ivars().linear_oauth_pending.borrow().is_some();
        let linear_blurb = match &error {
            Some(error) if failed == LINEAR_ROW => error,
            _ if linear_pending => "Waiting for Linear authorization…",
            _ if linear_on => "Syncing your assigned issues",
            _ if linear::oauth_client_id().is_some() => "Connects with Linear using OAuth",
            _ => "Connect opens Linear to make a key",
        };
        if let Some(label) = pane.viewWithTag(STATUS + LINEAR_ROW).and_then(|v| v.downcast::<NSTextField>().ok()) {
            label.setStringValue(&NSString::from_str(linear_blurb));
        }
        if let Some(button) = pane.viewWithTag(LINEAR_ROW).and_then(|v| v.downcast::<NSButton>().ok()) {
            button.setTitle(&NSString::from_str(if linear_on { "Disconnect" } else if linear_pending { "Cancel" } else { "Connect" }));
            button.setEnabled(true);
        }
        if let Some(field) = pane.viewWithTag(FIELD + LINEAR_ROW) {
            field.setHidden(linear_on);
        }
        let rows = [
            (CLAUDE_CODE_ROW, claude_mcp::code_added(), "Gives Claude Code your tasks", "Claude Code can use your tasks"),
            (CLAUDE_DESKTOP_ROW, claude_mcp::desktop_added(), "Gives Claude Desktop your tasks", "Added. Restart Claude Desktop"),
        ];
        // Access can be refused in the prompt or later in System Settings; say where to change it.
        let calendar = if self.ivars().settings.borrow().calendar && calendar::access() == Access::Denied {
            "Allow NotchGo in System Settings › Privacy › Calendars"
        } else {
            "Today's events, read-only"
        };
        if let Some(label) = pane.viewWithTag(STATUS + CALENDAR_ROW).and_then(|v| v.downcast::<NSTextField>().ok()) {
            label.setStringValue(&NSString::from_str(calendar));
        }
        let reminders = if self.ivars().settings.borrow().reminders && reminders::access() == Access::Denied {
            "Allow NotchGo in System Settings › Privacy › Reminders"
        } else {
            "Two-way with the NotchGo list, for iPhone"
        };
        if let Some(label) = pane.viewWithTag(STATUS + REMINDERS_ROW).and_then(|v| v.downcast::<NSTextField>().ok()) {
            label.setStringValue(&NSString::from_str(reminders));
        }
        for (row, added, idle, done) in rows {
            let blurb = match &error {
                Some(error) if row == failed => error,
                _ if added => done,
                _ => idle,
            };
            if let Some(label) = pane.viewWithTag(STATUS + row).and_then(|v| v.downcast::<NSTextField>().ok()) {
                label.setStringValue(&NSString::from_str(blurb));
            }
            if let Some(button) = pane.viewWithTag(row).and_then(|v| v.downcast::<NSButton>().ok()) {
                button.setTitle(&NSString::from_str(if added { "Added ✓" } else { "Add" }));
                button.setEnabled(!added);
            }
        }
    }
}

fn titled_pane(title: &str, mtm: MainThreadMarker) -> Retained<NSView> {
    let pane = frame_view(SIDEBAR, 0.0, PANE, HEIGHT, mtm);
    let heading = w::label(title, 24.0, unsafe { NSFontWeightBold }, &rgba(255, 255, 255, 1.0), mtm);
    heading.setFrame(rect(MARGIN, from_top(HEIGHT, 36.0, 32.0), PANE - 2.0 * MARGIN, 32.0));
    pane.addSubview(&heading);
    pane
}

/// Whether macOS will start the app at login; "requires approval" counts, since the user has asked.
fn opens_at_login() -> bool {
    let status = unsafe { SMAppService::mainAppService().status() };
    status == SMAppServiceStatus::Enabled || status == SMAppServiceStatus::RequiresApproval
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
