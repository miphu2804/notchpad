//! The Terminal tab's view: draws a `TerminalSession`'s grid and turns keys, paste and
//! the scroll wheel into input for it. Text goes through `NSTextInputClient` so input
//! methods such as Telex can compose before anything reaches the shell.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, Rgb};
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSEvent, NSEventModifierFlags, NSFont, NSFontAttributeName,
    NSFontWeightBold, NSFontWeightRegular, NSForegroundColorAttributeName, NSPasteboard, NSPasteboardTypeString,
    NSStringDrawing, NSTextInputClient, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSNotFound, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRange,
    NSRangePointer, NSRect, NSSize, NSString, NSUInteger,
};

use crate::terminal_keys::{self, Mods, Special};
use crate::terminal_palette::{self, BACKGROUND, CURSOR, FOREGROUND};
use crate::terminal_session::{Size, TerminalSession, Wake};

/// Posted when the view stops being first responder, so the panel can re-check the hover.
pub const RESIGNED_NOTIFICATION: &str = "NotchpadTerminalResigned";

const EXITED_NOTICE: &str = "[process exited — press Return to restart]";
const RETURN_KEY: u16 = 36;
/// Lines the wheel moves a full-screen program that has no scrollback (like a key press).
const ALT_SCREEN_ARROWS: i32 = 3;

pub struct ViewIvars {
    session: RefCell<Option<TerminalSession>>,
    font: Retained<NSFont>,
    bold_font: Retained<NSFont>,
    /// Cell size in points.
    cell: (f64, f64),
    /// Text an input method is composing; shown at the cursor, not yet sent.
    marked: RefCell<String>,
    /// Fraction of a line left over from smooth trackpad scrolling.
    scroll_rest: Cell<f64>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ViewIvars]
    #[name = "TerminalView"]
    pub struct TerminalView;

    unsafe impl NSObjectProtocol for TerminalView {}

    impl TerminalView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            let name = NSString::from_str(RESIGNED_NOTIFICATION);
            let object: &AnyObject = self.as_ref();
            unsafe { NSNotificationCenter::defaultCenter().postNotificationName_object(&name, Some(object)) };
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.fit();
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.handle_key(event);
        }

        /// ⌘V arrives as a key equivalent, and nothing else in this app owns a Paste item.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            let focused = self.window().and_then(|w| w.firstResponder()).is_some_and(|r| std::ptr::eq(&*r as *const _ as *const u8, self as *const _ as *const u8));
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            let pastes = event.charactersIgnoringModifiers().is_some_and(|c| c.to_string() == "v");
            if focused && command && pastes {
                self.paste();
                true
            } else {
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.scroll_by(event.scrollingDeltaY(), event.hasPreciseScrollingDeltas());
        }
    }

    unsafe impl NSTextInputClient for TerminalView {
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _range: NSRange) {
            self.ivars().marked.borrow_mut().clear();
            self.send(text_of(string).into_bytes());
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, string: &AnyObject, _selected: NSRange, _range: NSRange) {
            *self.ivars().marked.borrow_mut() = text_of(string);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            self.ivars().marked.borrow_mut().clear();
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            !self.ivars().marked.borrow().is_empty()
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            let len = self.ivars().marked.borrow().encode_utf16().count();
            if len == 0 { NSRange::new(NSNotFound as usize, 0) } else { NSRange::new(0, len) }
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange::new(NSNotFound as usize, 0)
        }

        #[unsafe(method(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(&self, _range: NSRange, _actual: NSRangePointer) -> *mut NSAttributedString {
            std::ptr::null_mut()
        }

        #[unsafe(method(validAttributesForMarkedText))]
        fn valid_attributes(&self) -> *mut NSArray<NSAttributedStringKey> {
            Retained::autorelease_ptr(NSArray::new())
        }

        /// Where the input method puts its candidate window: under the cursor cell.
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(&self, _range: NSRange, _actual: NSRangePointer) -> NSRect {
            let (cw, ch) = self.ivars().cell;
            let cursor = self.cursor_cell().unwrap_or((0, 0));
            let local = NSRect::new(NSPoint::new(cursor.1 as f64 * cw, cursor.0 as f64 * ch), NSSize::new(cw, ch));
            let in_window = self.convertRect_toView(local, None);
            self.window().map_or(in_window, |w| w.convertRectToScreen(in_window))
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index(&self, _point: NSPoint) -> NSUInteger {
            NSNotFound as usize
        }

        /// Commands the input method did not consume (arrows, Return) are handled before
        /// it is asked, so whatever remains has no meaning for a terminal.
        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, _selector: objc2::runtime::Sel) {}
    }
);

impl TerminalView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let font = NSFont::monospacedSystemFontOfSize_weight(12.0, unsafe { NSFontWeightRegular });
        let bold_font = NSFont::monospacedSystemFontOfSize_weight(12.0, unsafe { NSFontWeightBold });
        let attrs = Self::attributes(&font, &NSColor::whiteColor());
        let glyph = unsafe { NSString::from_str("M").sizeWithAttributes(Some(&attrs)) };
        // Whole-point cells keep backgrounds seamless and the grid arithmetic exact.
        let cell = (glyph.width.round().max(1.0), glyph.height.ceil().max(1.0));
        let ivars = ViewIvars {
            session: RefCell::new(None),
            font,
            bold_font,
            cell,
            marked: RefCell::new(String::new()),
            scroll_rest: Cell::new(0.0),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        unsafe { msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(100.0, 100.0))] }
    }

    /// Starts the shell the first time it is called; later calls do nothing, so the
    /// shell survives the panel collapsing and the tab switching.
    pub fn start(&self) {
        if self.ivars().session.borrow().is_some() {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else { return };
        match TerminalSession::spawn(self.grid_size(), self.wake_callback(mtm)) {
            Ok(session) => *self.ivars().session.borrow_mut() = Some(session),
            Err(err) => eprintln!("notchpad: could not start the shell: {err}"),
        }
        self.setNeedsDisplay(true);
    }

    /// The PTY thread cannot touch AppKit, so it asks the main queue to redraw. Wakeups
    /// that arrive before that runs are folded into it, which keeps a busy program from
    /// queueing a redraw per chunk of output.
    fn wake_callback(&self, mtm: MainThreadMarker) -> Wake {
        let view = Arc::new(MainThreadBound::new(Weak::new(self), mtm));
        let pending = Arc::new(AtomicBool::new(false));
        Arc::new(move || {
            if pending.swap(true, Ordering::SeqCst) {
                return;
            }
            let (view, pending) = (view.clone(), pending.clone());
            DispatchQueue::main().exec_async(move || {
                pending.store(false, Ordering::SeqCst);
                if let Some(mtm) = MainThreadMarker::new()
                    && let Some(view) = view.get(mtm).load()
                {
                    view.setNeedsDisplay(true);
                }
            });
        })
    }

    fn grid_size(&self) -> Size {
        let (cw, ch) = self.ivars().cell;
        let bounds = self.bounds();
        // A zero-sized view (not laid out yet) still needs a usable grid.
        let cells = |extent: f64, cell: f64| ((extent / cell).floor() as u16).max(2);
        Size {
            cols: cells(bounds.size.width, cw),
            rows: cells(bounds.size.height, ch),
            cell_width: cw as u16,
            cell_height: ch as u16,
        }
    }

    fn fit(&self) {
        if let Some(session) = self.ivars().session.borrow().as_ref() {
            session.resize(self.grid_size());
        }
    }

    fn mode(&self) -> TermMode {
        self.ivars().session.borrow().as_ref().map_or(TermMode::NONE, |s| *s.term().mode())
    }

    /// Input of any kind returns the view to the live screen first.
    fn send(&self, bytes: impl Into<std::borrow::Cow<'static, [u8]>>) {
        if let Some(session) = self.ivars().session.borrow().as_ref() {
            session.scroll(Scroll::Bottom);
            session.write(bytes);
        }
        self.setNeedsDisplay(true);
    }

    fn restart(&self) {
        *self.ivars().session.borrow_mut() = None;
        self.start();
    }

    fn exited(&self) -> bool {
        self.ivars().session.borrow().as_ref().is_some_and(|s| s.exited())
    }

    fn handle_key(&self, event: &NSEvent) {
        if self.exited() {
            if event.keyCode() == RETURN_KEY {
                self.restart();
            }
            return;
        }
        // While an input method is composing, it owns every key, Return and Backspace included.
        if !self.ivars().marked.borrow().is_empty() {
            return self.interpret(event);
        }
        let flags = event.modifierFlags();
        if flags.contains(NSEventModifierFlags::Command) {
            return;
        }
        let mods = Mods {
            shift: flags.contains(NSEventModifierFlags::Shift),
            ctrl: flags.contains(NSEventModifierFlags::Control),
            alt: flags.contains(NSEventModifierFlags::Option),
        };
        if let Some(key) = terminal_keys::special(event.keyCode()) {
            let app_cursor = self.mode().contains(TermMode::APP_CURSOR);
            return self.send(terminal_keys::special_bytes(key, mods, app_cursor));
        }
        let plain = event.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default();
        // Keys without a character of their own report private-use code points.
        if plain.chars().next().is_none_or(|c| ('\u{f700}'..='\u{f8ff}').contains(&c)) {
            return;
        }
        if mods.ctrl {
            if let Some(byte) = plain.chars().next().and_then(terminal_keys::ctrl_byte) {
                let bytes = if mods.alt { vec![0x1b, byte] } else { vec![byte] };
                self.send(bytes);
            }
        } else if mods.alt {
            self.send(terminal_keys::meta(&plain));
        } else {
            self.interpret(event);
        }
    }

    fn interpret(&self, event: &NSEvent) {
        self.interpretKeyEvents(&NSArray::from_slice(&[event]));
    }

    fn paste(&self) {
        let pasteboard = NSPasteboard::generalPasteboard();
        if let Some(text) = pasteboard.stringForType(unsafe { NSPasteboardTypeString }) {
            let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
            self.send(terminal_keys::paste(&text.to_string(), bracketed));
        }
    }

    fn scroll_by(&self, delta: f64, precise: bool) {
        let lines = if precise { delta / self.ivars().cell.1 } else { delta };
        let total = self.ivars().scroll_rest.get() + lines;
        let whole = total.trunc();
        self.ivars().scroll_rest.set(total - whole);
        let steps = whole as i32;
        if steps == 0 {
            return;
        }
        let mode = self.mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            // No scrollback on the alternate screen: do what the arrow keys would.
            let key = if steps > 0 { Special::Up } else { Special::Down };
            let arrow = terminal_keys::special_bytes(key, Mods::default(), mode.contains(TermMode::APP_CURSOR));
            self.send(arrow.repeat((steps.abs() * ALT_SCREEN_ARROWS) as usize));
        } else if let Some(session) = self.ivars().session.borrow().as_ref() {
            session.scroll(Scroll::Delta(steps));
            self.setNeedsDisplay(true);
        }
    }

    /// Cursor position as (row, column) on screen.
    fn cursor_cell(&self) -> Option<(usize, usize)> {
        let session = self.ivars().session.borrow();
        let term = session.as_ref()?.term();
        let point = term.grid().cursor.point;
        let row = point.line.0 + term.grid().display_offset() as i32;
        (row >= 0).then_some((row as usize, point.column.0))
    }

    // ---- drawing -------------------------------------------------------------------

    fn attributes(font: &NSFont, color: &NSColor) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
        let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
        let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
        NSDictionary::from_slices(&keys, &values)
    }

    fn paint(&self) {
        let cells = self.visible_cells();
        let (cw, ch) = self.ivars().cell;
        let origin = |row: usize, col: usize| NSPoint::new(col as f64 * cw, row as f64 * ch);
        let mut styles: HashMap<((u8, u8, u8), bool), Retained<NSDictionary<NSAttributedStringKey, AnyObject>>> = HashMap::new();

        for cell in &cells.drawn {
            if cell.bg != BACKGROUND {
                fill(cell.bg, NSRect::new(origin(cell.row, cell.col), NSSize::new(cw * cell.width as f64, ch)));
            }
        }
        for cell in &cells.drawn {
            let at = origin(cell.row, cell.col);
            if cell.ch != ' ' && !cell.hidden {
                let style = styles.entry(((cell.fg.r, cell.fg.g, cell.fg.b), cell.bold)).or_insert_with(|| {
                    let font = if cell.bold { &self.ivars().bold_font } else { &self.ivars().font };
                    Self::attributes(font, &ns_color(cell.fg))
                });
                draw_text(&cell.ch.to_string(), at, style);
            }
            if cell.underline {
                fill(cell.fg, NSRect::new(NSPoint::new(at.x, at.y + ch - 2.0), NSSize::new(cw * cell.width as f64, 1.0)));
            }
        }

        let marked = self.ivars().marked.borrow();
        let notice = self.exited().then_some(EXITED_NOTICE);
        let Some((row, col)) = cells.cursor else { return };
        let plain = Self::attributes(&self.ivars().font, &ns_color(FOREGROUND));
        if !marked.is_empty() {
            let width = marked.chars().count() as f64 * cw;
            let at = origin(row, col);
            fill(BACKGROUND, NSRect::new(at, NSSize::new(width, ch)));
            draw_text(&marked, at, &plain);
            fill(FOREGROUND, NSRect::new(NSPoint::new(at.x, at.y + ch - 1.0), NSSize::new(width, 1.0)));
        }
        if let Some(notice) = notice {
            let at = origin(row + 1, 0);
            let dim = Self::attributes(&self.ivars().font, &ns_color(terminal_palette::rgb(8)));
            draw_text(notice, at, &dim);
        }
    }

    /// Copies what is visible out of the emulator so drawing happens without its lock.
    fn visible_cells(&self) -> Screen {
        let mut screen = Screen { drawn: Vec::new(), cursor: None };
        let session = self.ivars().session.borrow();
        let Some(session) = session.as_ref() else { return screen };
        let term = session.term();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let row = content.cursor.point.line.0 + offset;
        screen.cursor = (row >= 0).then_some((row as usize, content.cursor.point.column.0));
        let shown = content.mode.contains(TermMode::SHOW_CURSOR);
        for item in content.display_iter {
            let (cell, row) = (item.cell, item.point.line.0 + offset);
            let flags = cell.flags;
            if row < 0 || flags.contains(Flags::WIDE_CHAR_SPACER) || flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fg, content.colors), resolve(cell.bg, content.colors));
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = Rgb { r: fg.r / 3 * 2, g: fg.g / 3 * 2, b: fg.b / 3 * 2 };
            }
            let col = item.point.column.0;
            let at_cursor = shown && screen.cursor == Some((row as usize, col));
            if at_cursor {
                (fg, bg) = (BACKGROUND, CURSOR);
            }
            let underline = flags.intersects(Flags::ALL_UNDERLINES);
            if cell.c == ' ' && bg == BACKGROUND && !underline {
                continue;
            }
            screen.drawn.push(Painted {
                row: row as usize,
                col,
                ch: cell.c,
                fg,
                bg,
                width: if flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 },
                bold: flags.contains(Flags::BOLD),
                underline,
                hidden: flags.contains(Flags::HIDDEN),
            });
        }
        screen
    }
}

struct Painted {
    row: usize,
    col: usize,
    ch: char,
    fg: Rgb,
    bg: Rgb,
    width: usize,
    bold: bool,
    underline: bool,
    hidden: bool,
}

struct Screen {
    /// Only cells that differ from an empty default one.
    drawn: Vec<Painted>,
    cursor: Option<(usize, usize)>,
}

fn resolve(color: Color, overrides: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(i) => overrides[usize::from(i)].unwrap_or_else(|| terminal_palette::rgb(usize::from(i))),
        Color::Named(name) => overrides[name as usize].unwrap_or_else(|| terminal_palette::rgb(name as usize)),
    }
}

fn draw_text(text: &str, at: NSPoint, attributes: &NSDictionary<NSAttributedStringKey, AnyObject>) {
    unsafe { NSString::from_str(text).drawAtPoint_withAttributes(at, Some(attributes)) };
}

fn ns_color(rgb: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(f64::from(rgb.r) / 255.0, f64::from(rgb.g) / 255.0, f64::from(rgb.b) / 255.0, 1.0)
}

fn fill(rgb: Rgb, rect: NSRect) {
    ns_color(rgb).setFill();
    NSBezierPath::fillRect(rect);
}

/// Input methods hand over either a plain or an attributed string.
fn text_of(string: &AnyObject) -> String {
    if let Some(attributed) = string.downcast_ref::<NSAttributedString>() {
        attributed.string().to_string()
    } else if let Some(plain) = string.downcast_ref::<NSString>() {
        plain.to_string()
    } else {
        String::new()
    }
}
