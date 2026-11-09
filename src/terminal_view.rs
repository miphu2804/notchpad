//! The Terminal tab's view: draws a `TerminalSession`'s grid and turns keys, paste and
//! the scroll wheel into input for it. Text goes through `NSTextInputClient` so input
//! methods such as Telex can compose before anything reaches the shell.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, Rgb};
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSEvent, NSEventModifierFlags, NSFont, NSFontAttributeName,
    NSFontWeightBold, NSFontWeightRegular, NSForegroundColorAttributeName, NSPasteboard, NSPasteboardTypeString,
    NSStringDrawing, NSTextInputClient, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSNotFound, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRange,
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
/// Quiet time after the last size change before the grid follows it.
const GRID_SETTLE_SECS: f64 = 0.12;

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
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            self.select_at(event, true);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.select_at(event, false);
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            // The panel resizes on every frame while it springs to a new height. Telling the
            // program only once it has settled spares it a redraw per row along the way.
            let none: Option<&AnyObject> = None;
            let _: () = unsafe { msg_send![NSObject::class(), cancelPreviousPerformRequestsWithTarget: self, selector: sel!(fitGrid), object: none] };
            let _: () = unsafe { msg_send![self, performSelector: sel!(fitGrid), withObject: none, afterDelay: GRID_SETTLE_SECS] };
        }

        #[unsafe(method(fitGrid))]
        fn fit_grid(&self) {
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

        /// ⌘V and ⌘C arrive as key equivalents, and nothing else in this app owns those items.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            let focused = self.window().and_then(|w| w.firstResponder()).is_some_and(|r| std::ptr::eq(&*r as *const _ as *const u8, self as *const _ as *const u8));
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            let key = event.charactersIgnoringModifiers().map(|c| c.to_string());
            if focused && command && key.as_deref() == Some("v") {
                self.paste();
                true
            } else if focused && command && key.as_deref() == Some("c") {
                self.copy();
                true
            } else {
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.scroll_by(event);
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
            session.clear_selection();
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

    /// Copies the selection; with none, ⌘C is swallowed rather than sent to the shell as a key.
    fn copy(&self) {
        let text = self.ivars().session.borrow().as_ref().and_then(|s| s.selected_text());
        if let Some(text) = text {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            pasteboard.setString_forType(&NSString::from_str(&text), unsafe { NSPasteboardTypeString });
        }
    }

    /// Starts (click) or extends (drag) the selection under the pointer. A program that
    /// asked for mouse reports owns the mouse, except while Shift is held.
    fn select_at(&self, event: &NSEvent, start: bool) {
        let owned_by_program = self.mode().intersects(TermMode::MOUSE_MODE) && !event.modifierFlags().contains(NSEventModifierFlags::Shift);
        let session = self.ivars().session.borrow();
        let Some(session) = session.as_ref() else { return };
        if owned_by_program {
            return;
        }
        let local = self.convertPoint_fromView(event.locationInWindow(), None);
        let (point, side) = {
            let term = session.term();
            cell_at(local, self.ivars().cell, term.grid().display_offset(), term.columns(), term.screen_lines())
        };
        if start {
            let ty = match event.clickCount() {
                1 => SelectionType::Simple,
                2 => SelectionType::Semantic,
                _ => SelectionType::Lines,
            };
            session.begin_selection(ty, point, side);
        } else {
            session.extend_selection(point, side);
        }
        self.setNeedsDisplay(true);
    }

    fn scroll_by(&self, event: &NSEvent) {
        let delta = event.scrollingDeltaY();
        let lines = if event.hasPreciseScrollingDeltas() { delta / self.ivars().cell.1 } else { delta };
        let total = self.ivars().scroll_rest.get() + lines;
        let whole = total.trunc();
        self.ivars().scroll_rest.set(total - whole);
        let steps = whole as i32;
        if steps == 0 {
            return;
        }
        let mode = self.mode();
        if mode.intersects(TermMode::MOUSE_MODE) {
            // The program tracks the mouse, so it scrolls its own view: report the wheel.
            let Some((column, row)) = self.pointer_cell(event) else { return };
            let report = wheel_report(steps > 0, column, row, mode.contains(TermMode::SGR_MOUSE));
            self.send(report.repeat(steps.unsigned_abs() as usize));
        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            // No scrollback on the alternate screen: do what the arrow keys would.
            let key = if steps > 0 { Special::Up } else { Special::Down };
            let arrow = terminal_keys::special_bytes(key, Mods::default(), mode.contains(TermMode::APP_CURSOR));
            self.send(arrow.repeat((steps.abs() * ALT_SCREEN_ARROWS) as usize));
        } else if let Some(session) = self.ivars().session.borrow().as_ref() {
            session.scroll(Scroll::Delta(steps));
            self.setNeedsDisplay(true);
        }
    }

    /// Screen (column, row) under the pointer.
    fn pointer_cell(&self, event: &NSEvent) -> Option<(usize, usize)> {
        let session = self.ivars().session.borrow();
        let term = session.as_ref()?.term();
        let local = self.convertPoint_fromView(event.locationInWindow(), None);
        let (point, _) = cell_at(local, self.ivars().cell, 0, term.columns(), term.screen_lines());
        Some((point.column.0, point.line.0 as usize))
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
        if !cells.selected.is_empty() {
            NSColor::selectedTextBackgroundColor().colorWithAlphaComponent(0.5).setFill();
            for &(row, col) in &cells.selected {
                NSBezierPath::fillRect(NSRect::new(origin(row, col), NSSize::new(cw, ch)));
            }
        }
        for cell in &cells.drawn {
            let at = origin(cell.row, cell.col);
            if cell.ch != ' ' && !cell.hidden {
                let style = styles.entry(((cell.fg.r, cell.fg.g, cell.fg.b), cell.bold)).or_insert_with(|| {
                    let font = if cell.bold { &self.ivars().bold_font } else { &self.ivars().font };
                    Self::attributes(font, &ns_color(cell.fg))
                });
                if let Some((alpha, parts)) = block_shape(cell.ch) {
                    ns_color(cell.fg).colorWithAlphaComponent(alpha).setFill();
                    for [x, y, w, h] in parts {
                        // Snapped to whole points, neighbouring cells share an edge and leave no seam.
                        let (left, top) = ((at.x + x * cw).round(), (at.y + y * ch).round());
                        let (right, bottom) = ((at.x + (x + w) * cw).round(), (at.y + (y + h) * ch).round());
                        NSBezierPath::fillRect(NSRect::new(NSPoint::new(left, top), NSSize::new(right - left, bottom - top)));
                    }
                } else {
                    draw_text(&cell.ch.to_string(), at, style);
                }
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
        let mut screen = Screen { drawn: Vec::new(), selected: Vec::new(), cursor: None };
        let session = self.ivars().session.borrow();
        let Some(session) = session.as_ref() else { return screen };
        let term = session.term();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let row = content.cursor.point.line.0 + offset;
        screen.cursor = (row >= 0).then_some((row as usize, content.cursor.point.column.0));
        let shown = content.mode.contains(TermMode::SHOW_CURSOR);
        if let Some(range) = content.selection {
            for row in 0..term.screen_lines() {
                for col in 0..term.columns() {
                    if range.contains(Point::new(Line(row as i32 - offset), Column(col))) {
                        screen.selected.push((row, col));
                    }
                }
            }
        }
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
    /// Visible (row, column) cells inside the selection.
    selected: Vec<(usize, usize)>,
    cursor: Option<(usize, usize)>,
}

/// The grid cell under a point in the view, clamped to the grid. The side says which half
/// of the cell was hit, which decides whether a selection includes that cell's edge.
fn cell_at(point: NSPoint, cell: (f64, f64), display_offset: usize, cols: usize, rows: usize) -> (Point, Side) {
    let column = (point.x / cell.0).floor().clamp(0.0, (cols - 1) as f64) as usize;
    let row = (point.y / cell.1).floor().clamp(0.0, (rows - 1) as f64) as i32;
    let right_half = point.x >= cols as f64 * cell.0 || (point.x >= 0.0 && point.x % cell.0 >= cell.0 / 2.0);
    let side = if right_half { Side::Right } else { Side::Left };
    // Scrolled back by N lines, the top screen row is grid line -N.
    (Point::new(Line(row - display_offset as i32), Column(column)), side)
}

/// One wheel notch as a mouse report: button 64 is up, 65 down. SGR when the program
/// asked for it, else the legacy encoding, which offsets each value by 32 and so cannot
/// address past column or row 223.
fn wheel_report(up: bool, column: usize, row: usize, sgr: bool) -> Vec<u8> {
    let button: u8 = if up { 64 } else { 65 };
    if sgr {
        format!("\x1b[<{button};{};{}M", column + 1, row + 1).into_bytes()
    } else {
        let coord = |v: usize| (v + 33).min(255) as u8;
        vec![0x1b, b'[', b'M', 32 + button, coord(column), coord(row)]
    }
}

fn resolve(color: Color, overrides: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(i) => overrides[usize::from(i)].unwrap_or_else(|| terminal_palette::rgb(usize::from(i))),
        Color::Named(name) => overrides[name as usize].unwrap_or_else(|| terminal_palette::rgb(name as usize)),
    }
}

/// Block elements (U+2580–U+259F) as rectangles in cell fractions `[x, y, w, h]`, y from
/// the top, with the fill alpha (below 1 for the shades). The font's glyphs don't fill a
/// cell taller than the font, so pictures built from them, like logos, come apart.
fn block_shape(ch: char) -> Option<(f64, Vec<[f64; 4]>)> {
    const UL: [f64; 4] = [0.0, 0.0, 0.5, 0.5];
    const UR: [f64; 4] = [0.5, 0.0, 0.5, 0.5];
    const LL: [f64; 4] = [0.0, 0.5, 0.5, 0.5];
    const LR: [f64; 4] = [0.5, 0.5, 0.5, 0.5];
    const FULL: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
    let eighths = |n: u32| f64::from(n) / 8.0;
    let parts = match ch {
        '\u{2580}' => vec![[0.0, 0.0, 1.0, 0.5]],
        '\u{2581}'..='\u{2587}' => {
            let h = eighths(ch as u32 - 0x2580);
            vec![[0.0, 1.0 - h, 1.0, h]]
        }
        '\u{2588}' => vec![FULL],
        '\u{2589}'..='\u{258F}' => vec![[0.0, 0.0, eighths(0x2590 - ch as u32), 1.0]],
        '\u{2590}' => vec![[0.5, 0.0, 0.5, 1.0]],
        '\u{2591}'..='\u{2593}' => return Some((f64::from(ch as u32 - 0x2590) / 4.0, vec![FULL])),
        '\u{2594}' => vec![[0.0, 0.0, 1.0, 0.125]],
        '\u{2595}' => vec![[0.875, 0.0, 0.125, 1.0]],
        '\u{2596}' => vec![LL],
        '\u{2597}' => vec![LR],
        '\u{2598}' => vec![UL],
        '\u{2599}' => vec![UL, LL, LR],
        '\u{259A}' => vec![UL, LR],
        '\u{259B}' => vec![UL, UR, LL],
        '\u{259C}' => vec![UL, UR, LR],
        '\u{259D}' => vec![UR],
        '\u{259E}' => vec![UR, LL],
        '\u{259F}' => vec![UR, LL, LR],
        _ => return None,
    };
    Some((1.0, parts))
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

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (f64, f64) = (7.0, 14.0);

    #[test]
    fn wheel_reports_in_sgr_and_legacy_form() {
        assert_eq!(wheel_report(true, 4, 9, true), b"\x1b[<64;5;10M");
        assert_eq!(wheel_report(false, 0, 0, true), b"\x1b[<65;1;1M");
        assert_eq!(wheel_report(true, 4, 9, false), [0x1b, b'[', b'M', 96, 37, 42]);
        assert_eq!(wheel_report(false, 300, 0, false)[4], 255);
    }

    #[test]
    fn block_elements_fill_their_part_of_the_cell() {
        assert_eq!(block_shape('█'), Some((1.0, vec![[0.0, 0.0, 1.0, 1.0]])));
        assert_eq!(block_shape('▁'), Some((1.0, vec![[0.0, 0.875, 1.0, 0.125]])));
        assert_eq!(block_shape('▌'), Some((1.0, vec![[0.0, 0.0, 0.5, 1.0]])));
        assert_eq!(block_shape('▏'), Some((1.0, vec![[0.0, 0.0, 0.125, 1.0]])));
        assert_eq!(block_shape('▛').map(|(_, parts)| parts.len()), Some(3));
        assert_eq!(block_shape('▒').map(|(alpha, _)| alpha), Some(0.5));
        assert_eq!(block_shape('a'), None);
        assert_eq!(block_shape('─'), None);
    }

    #[test]
    fn point_maps_to_cell_and_half() {
        let (point, side) = cell_at(NSPoint::new(15.0, 30.0), CELL, 0, 80, 24);
        assert_eq!((point.line.0, point.column.0), (2, 2));
        assert!(matches!(side, Side::Left));
        let (_, side) = cell_at(NSPoint::new(18.0, 30.0), CELL, 0, 80, 24);
        assert!(matches!(side, Side::Right));
    }

    #[test]
    fn scrollback_offset_moves_the_line_up() {
        let (point, _) = cell_at(NSPoint::new(0.0, 0.0), CELL, 10, 80, 24);
        assert_eq!(point.line.0, -10);
    }

    #[test]
    fn points_outside_the_view_clamp_to_the_grid() {
        let (point, side) = cell_at(NSPoint::new(9999.0, -50.0), CELL, 0, 80, 24);
        assert_eq!((point.line.0, point.column.0), (0, 79));
        assert!(matches!(side, Side::Right));
        let (point, side) = cell_at(NSPoint::new(-5.0, 9999.0), CELL, 0, 80, 24);
        assert_eq!((point.line.0, point.column.0), (23, 0));
        assert!(matches!(side, Side::Left));
    }
}
