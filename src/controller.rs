//! Owns the notch window, the tasks and the focus timer, and renders the Workspace and
//! Week tabs. Every change re-renders from `State`; the views hold no state of their own.

use std::cell::{Cell, OnceCell, RefCell};
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSClickGestureRecognizer, NSSound, 
    NSAppearance, NSImageScaling, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType, NSButton, NSCellImagePosition, NSColor, NSEvent, NSFont,
    NSFontWeightBold, NSFontWeightLight, NSFontWeightMedium, NSFontWeightRegular,
    NSFontWeightSemibold, NSImageView, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSMainMenuWindowLevel, NSMenu, NSMenuItem, NSScreen, NSScrollView, NSStackView,
    NSStackViewDistribution, NSTextAlignment, NSTextDidChangeNotification, NSTextDidEndEditingNotification, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer};
use objc2_quartz_core::{CAMediaTiming, CATransition, kCATransitionFade};

use crate::clip_card::{self, ClipCard};
use crate::clips::ClipHistory;
use crate::days;
use crate::drop_column::DropColumn;
use crate::list_row::{self, ListRow};
use crate::now_playing::{Command, NowPlaying};
use crate::note_view::{self, NoteView};
use crate::notch_motion::NotchMotion;
use crate::task_row::TaskRow;
use crate::quick_add;
use crate::terminal_view::{self, TerminalView};
use crate::pomodoros::{self, DAILY_GOAL};
use crate::tomato_view::{Tomato, TomatoView};
use crate::panel::{self, FileDrag, NotchPanel, NotchShape};
use crate::store::{self, Task};
use crate::widgets::{self as w, rgba};

const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 330.0;
/// The window extends this far above the screen. The pointer pressed against the top
/// edge sits at exactly the screen's max Y, which would otherwise count as outside the
/// window and make the panel close and reopen in a loop.
const TOP_SLOP: f64 = 1.0;
/// MacBook Pro 14" (M4 Pro) notch, used on displays without one.
const DEFAULT_NOTCH: (f64, f64) = (185.0, 32.0);
const FOCUS_CARD_WIDTH: f64 = 236.0;
const MUSIC_PROGRESS_WIDTH: f64 = 340.0;
const ICON_TAB_WIDTH: f64 = 32.0;
const ARTWORK_SIZE: f64 = 120.0;
const MAX_CARD_ROWS: usize = 5;
const MAX_COLUMN_ROWS: usize = 6;
/// Gap between Today rows; a hairline is drawn in its middle.
const CARD_ROW_GAP: f64 = 15.0;
/// Quiet period after the last keystroke before the note is written to disk.
const NOTES_SAVE_DELAY: f64 = 0.5;
const DURATIONS_MIN: [u64; 4] = [15, 25, 45, 60];
const SHORT_BREAK_MIN: u64 = 5;
const LONG_BREAK_MIN: u64 = 15;
/// A long break follows every this many focus sessions.
const CYCLE: usize = 4;
const TOMATO_SIZE: f64 = 33.0;
const CORAL: (u8, u8, u8) = (217, 112, 90);

/// Week columns, also used as the add-field tag that picks the new task's day.
const INBOX: isize = 0;
const TODAY: isize = 1;
const TOMORROW: isize = 2;

/// Dark text on the pastel cards.
fn ink(alpha: f64) -> Retained<NSColor> {
    rgba(26, 30, 26, alpha)
}

fn white(alpha: f64) -> Retained<NSColor> {
    rgba(255, 255, 255, alpha)
}

struct Column {
    list: Retained<NSStackView>,
    count: Retained<NSTextField>,
}

struct FocusCard {
    count: Retained<NSTextField>,
    tomatoes: Vec<Retained<TomatoView>>,
    task: Retained<NSTextField>,
    badge: Retained<NSTextField>,
    time: Retained<NSTextField>,
    ends: Retained<NSTextField>,
    pause: Retained<NSButton>,
    end: Retained<NSButton>,
}

struct TaskCard {
    todo: Retained<NSButton>,
    done: Retained<NSButton>,
    rows: Retained<NSStackView>,
    input: Retained<NSTextField>,
}

struct ClipsView {
    body: Retained<NSView>,
    /// Cards newest first, left to right, inside a horizontal scroll view.
    strip: Retained<NSStackView>,
    count: Retained<NSTextField>,
    empty: Retained<NSTextField>,
}

struct MusicView {
    body: Retained<NSView>,
    /// Artwork plus the track column; hidden while nothing plays.
    now: Retained<NSStackView>,
    empty: Retained<NSTextField>,
    art: Retained<NSImageView>,
    title: Retained<NSTextField>,
    subtitle: Retained<NSTextField>,
    progress: Retained<NSLayoutConstraint>,
    elapsed: Retained<NSTextField>,
    total: Retained<NSTextField>,
    toggle: Retained<NSButton>,
}

struct NotesView {
    body: Retained<NSView>,
    text: Retained<NoteView>,
    /// Placeholder shown while the note is empty.
    hint: Retained<NSTextField>,
    /// Brief confirmation after ⌘↩ turns a line into a task.
    added: Retained<NSTextField>,
}

struct TerminalPane {
    body: Retained<NSView>,
    view: Retained<TerminalView>,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Workspace,
    Week,
    Clips,
    Music,
    Notes,
    Terminal,
}

/// The header's views: close, the tab groups left and right of the notch, and each tab.
struct Header {
    quit: Retained<NSButton>,
    left: Retained<NSStackView>,
    right: Retained<NSStackView>,
    workspace: Retained<NSButton>,
    week: Retained<NSButton>,
    clips: Retained<NSButton>,
    music: Retained<NSButton>,
    notes: Retained<NSButton>,
    terminal: Retained<NSButton>,
}

struct Ui {
    panel: Retained<NotchPanel>,
    content: Retained<NSView>,
    workspace: Retained<NSStackView>,
    week: Retained<NSStackView>,
    tab_workspace: Retained<NSButton>,
    tab_week: Retained<NSButton>,
    tab_clips: Retained<NSButton>,
    tab_music: Retained<NSButton>,
    tab_notes: Retained<NSButton>,
    tab_terminal: Retained<NSButton>,
    clips: ClipsView,
    music: MusicView,
    notes: NotesView,
    terminal: TerminalPane,
    focus: FocusCard,
    tasks: TaskCard,
    columns: Vec<Column>,
    expanded: Cell<NSRect>,
    /// Height of the header band beside the notch; follows the display's safe area.
    header_height: Retained<NSLayoutConstraint>,
    /// Keep the tab groups clear of the hardware notch: half its width plus a margin.
    notch_gaps: [Retained<NSLayoutConstraint>; 2],
    motion: Retained<NotchMotion>,
}

#[derive(Default)]
pub struct State {
    ui: OnceCell<Ui>,
    tasks: RefCell<Vec<Task>>,
    expanded: Cell<bool>,
    /// Opened by hotkey: stays open until the pointer has visited and left.
    pinned: Cell<bool>,
    tab: Cell<Tab>,
    clips: RefCell<ClipHistory>,
    now_playing: RefCell<NowPlaying>,
    /// Runs only while the Music tab is visible and a track is playing.
    music_timer: RefCell<Option<Retained<NSTimer>>>,
    show_completed: Cell<bool>,
    /// Task index chosen with ▶; falls back to the first open task of today.
    focus: Cell<Option<usize>>,
    duration_idx: Cell<usize>,
    deadline: Cell<Option<Instant>>,
    /// Time left on a paused session; the ticker is off while this is set.
    paused: Cell<Option<Duration>>,
    on_break: Cell<bool>,
    break_min: Cell<u64>,
    /// Focus sessions completed today, read from `pomodoros.txt`.
    done_today: Cell<usize>,
    ticker: RefCell<Option<Retained<NSTimer>>>,
    /// Modification time of `tasks.txt` as last loaded or saved by this process.
    seen_mtime: Cell<Option<std::time::SystemTime>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    impl Controller {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.cancel_recheck();
            self.ivars().pinned.set(false);
            self.set_expanded(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if !self.ivars().pinned.get() {
                self.collapse_when_left();
            }
        }

        /// Collapses once the pointer is really off the panel. After a spurious exit the
        /// tracking area won't report the real one, so this re-polls until then.
        #[unsafe(method(recheckHover:))]
        fn recheck_hover(&self, _arg: Option<&AnyObject>) {
            self.collapse_when_left();
        }

        #[unsafe(method(addTask:))]
        fn add_task(&self, field: &NSTextField) {
            let text = field.stringValue().to_string();
            let text = text.trim();
            if !text.is_empty() {
                let (today, tomorrow) = (days::key(0), days::key(1));
                let default_day = match field.tag() {
                    INBOX => "",
                    TOMORROW => &tomorrow,
                    _ => &today,
                };
                self.push_task(text, default_day);
            }
            field.setStringValue(&NSString::new());
        }

        #[unsafe(method(toggleTask:))]
        fn toggle_task(&self, sender: &NSButton) {
            if let Some(task) = self.ivars().tasks.borrow_mut().get_mut(sender.tag() as usize) {
                task.done = !task.done;
            }
            self.save_and_refresh();
        }

        #[unsafe(method(deleteTask:))]
        fn delete_task(&self, item: &NSMenuItem) {
            let index = item.tag() as usize;
            {
                let mut tasks = self.ivars().tasks.borrow_mut();
                if index >= tasks.len() {
                    return;
                }
                tasks.remove(index);
            }
            // The focus card refers to tasks by index, so follow the shift.
            let focus = self.ivars().focus.get().and_then(|f| match f {
                f if f == index => None,
                f if f > index => Some(f - 1),
                f => Some(f),
            });
            self.ivars().focus.set(focus);
            self.save_and_refresh();
        }

        #[unsafe(method(moveToInbox:))]
        fn move_to_inbox(&self, item: &NSMenuItem) {
            self.reschedule(item.tag() as usize, String::new());
        }

        #[unsafe(method(moveToToday:))]
        fn move_to_today(&self, item: &NSMenuItem) {
            self.reschedule(item.tag() as usize, days::key(0));
        }

        #[unsafe(method(moveToTomorrow:))]
        fn move_to_tomorrow(&self, item: &NSMenuItem) {
            self.reschedule(item.tag() as usize, days::key(1));
        }

        #[unsafe(method(focusTask:))]
        fn focus_task(&self, sender: &NSButton) {
            self.ivars().focus.set(Some(sender.tag() as usize));
            self.refresh();
        }

        #[unsafe(method(showTodo:))]
        fn show_todo(&self, _sender: &AnyObject) {
            self.ivars().show_completed.set(false);
            self.refresh();
        }

        #[unsafe(method(showCompleted:))]
        fn show_completed(&self, _sender: &AnyObject) {
            self.ivars().show_completed.set(true);
            self.refresh();
        }

        #[unsafe(method(showWorkspace:))]
        fn show_workspace(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Workspace);
            self.refresh();
        }

        #[unsafe(method(showWeek:))]
        fn show_week(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Week);
            self.refresh();
        }

        #[unsafe(method(showClips:))]
        fn show_clips(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Clips);
            self.refresh();
        }

        #[unsafe(method(showMusic:))]
        fn show_music(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Music);
            // Spawned on first use, so nothing runs for people who never open the tab.
            self.ivars().now_playing.borrow_mut().start(self.target(), sel!(nowPlayingChanged:));
            self.refresh();
        }

        #[unsafe(method(musicPrevious:))]
        fn music_previous(&self, _sender: &AnyObject) {
            self.ivars().now_playing.borrow_mut().send(Command::Previous);
        }

        #[unsafe(method(musicPlayPause:))]
        fn music_play_pause(&self, _sender: &AnyObject) {
            self.ivars().now_playing.borrow_mut().send(Command::PlayPause);
        }

        #[unsafe(method(musicNext:))]
        fn music_next(&self, _sender: &AnyObject) {
            self.ivars().now_playing.borrow_mut().send(Command::Next);
        }

        #[unsafe(method(nowPlayingChanged:))]
        fn now_playing_changed(&self, _arg: Option<&AnyObject>) {
            self.ivars().now_playing.borrow_mut().drain();
            self.render_music();
        }

        #[unsafe(method(tickMusic:))]
        fn tick_music(&self, _timer: &NSTimer) {
            self.render_music();
        }

        #[unsafe(method(showNotes:))]
        fn show_notes(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Notes);
            self.refresh();
        }

        /// Text changed: toggles the placeholder and saves once typing pauses.
        #[unsafe(method(notesChanged:))]
        fn notes_changed(&self, _notification: &NSNotification) {
            self.after_note_edit();
        }

        #[unsafe(method(saveNotes:))]
        fn save_notes(&self, _arg: Option<&AnyObject>) {
            self.save_notes_now();
        }

        /// Focus left the note (clicked elsewhere): the hover may have ended meanwhile.
        #[unsafe(method(notesEditingEnded:))]
        fn notes_editing_ended(&self, _notification: &NSNotification) {
            self.save_notes_now();
            self.collapse_when_left();
        }

        /// Starts the shell on first use and hands the keyboard to it.
        #[unsafe(method(showTerminal:))]
        fn show_terminal(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Terminal);
            self.refresh();
            let ui = self.ui();
            // The view needs its real size before the shell is told how big the screen is.
            ui.content.layoutSubtreeIfNeeded();
            ui.terminal.view.start();
            ui.panel.makeFirstResponder(Some(&ui.terminal.view));
        }

        /// The terminal lost focus: the pointer may have left the panel meanwhile. Runs
        /// after the responder change, when `note_has_focus` has settled.
        #[unsafe(method(terminalResigned:))]
        fn terminal_resigned(&self, _notification: &NSNotification) {
            let none: Option<&AnyObject> = None;
            let _: () = unsafe { msg_send![self, performSelector: sel!(recheckHover:), withObject: none, afterDelay: 0.0f64] };
        }

        #[unsafe(method(clearAdded:))]
        fn clear_added(&self, _arg: Option<&AnyObject>) {
            self.ivars().ui.get().expect("ui is built in new()").notes.added.setHidden(true);
        }

        #[unsafe(method(clearClips:))]
        fn clear_clips(&self, _sender: &AnyObject) {
            self.ivars().clips.borrow_mut().clear();
            self.render_clips();
        }

        #[unsafe(method(pollClips:))]
        fn poll_clips(&self, _timer: &NSTimer) {
            let changed = self.ivars().clips.borrow_mut().poll();
            if changed {
                self.render_clips();
            }
        }

        #[unsafe(method(toggleTimer:))]
        fn toggle_timer(&self, _sender: &AnyObject) {
            let state = self.ivars();
            if state.deadline.get().is_some() {
                let left = state.deadline.get().map(|d| d.saturating_duration_since(Instant::now()));
                self.stop_timer();
                state.paused.set(left);
                self.render_focus();
            } else {
                let secs = state.paused.take().map_or_else(|| self.session_secs(), |left| left.as_secs() + 1);
                self.run_timer(secs);
            }
        }

        /// End: a focus stopped early is logged; a break is simply skipped.
        #[unsafe(method(endTimer:))]
        fn end_timer(&self, _sender: &AnyObject) {
            let state = self.ivars();
            if !state.on_break.get() && self.session_active() {
                let elapsed = self.session_secs() - self.remaining_secs().min(self.session_secs());
                pomodoros::append(&days::stamp(), elapsed / 60, false);
            }
            state.on_break.set(false);
            self.stop_timer();
        }

        #[unsafe(method(cycleDuration:))]
        fn cycle_duration(&self, _sender: &AnyObject) {
            if self.session_active() {
                return;
            }
            let next = (self.ivars().duration_idx.get() + 1) % DURATIONS_MIN.len();
            self.ivars().duration_idx.set(next);
            self.render_focus();
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            let Some(deadline) = self.ivars().deadline.get() else { return };
            if Instant::now() >= deadline {
                let state = self.ivars();
                let was_break = state.on_break.replace(false);
                self.stop_timer();
                if !was_break {
                    pomodoros::append(&days::stamp(), DURATIONS_MIN[state.duration_idx.get()], true);
                    let done = pomodoros::done_today(&days::key(0));
                    state.done_today.set(done);
                    let long = done % CYCLE == 0;
                    state.on_break.set(true);
                    state.break_min.set(if long { LONG_BREAK_MIN } else { SHORT_BREAK_MIN });
                    self.run_timer(self.session_secs());
                }
                // A soft chime rather than the alert beep: this is good news, not an error.
                if let Some(chime) = NSSound::soundNamed(&NSString::from_str("Glass")) {
                    chime.play();
                }
                self.ivars().tab.set(Tab::Workspace);
                self.refresh();
                self.pin_open();
            } else {
                self.render_focus();
            }
        }

        /// Picks up edits made to `tasks.txt` by other processes (e.g. the MCP server).
        #[unsafe(method(reload:))]
        fn reload(&self, _timer: &NSTimer) {
            let mtime = store::modified();
            if mtime != self.ivars().seen_mtime.get() {
                self.ivars().seen_mtime.set(mtime);
                *self.ivars().tasks.borrow_mut() = store::load();
                self.ivars().focus.set(None);
                self.refresh();
            }
        }

        #[unsafe(method(screenParametersChanged:))]
        fn screen_parameters_changed(&self, _notification: &NSNotification) {
            let (collapsed, expanded, safe_top) = notch_rects(self.mtm());
            let ui = self.ui();
            ui.expanded.set(expanded);
            ui.header_height.setConstant(header_band(safe_top));
            for constraint in &ui.notch_gaps {
                constraint.setConstant(notch_gap(collapsed, safe_top));
            }
            ui.motion.set_rects(collapsed, expanded);
            self.collapse_when_left();
        }

        #[unsafe(method(redraw:))]
        fn redraw(&self, _arg: Option<&AnyObject>) {
            self.refresh();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            self.save_notes_now();
            self.ivars().now_playing.borrow_mut().stop();
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let state = State {
            tasks: RefCell::new(store::load()),
            duration_idx: Cell::new(1), // 25 min
            done_today: Cell::new(pomodoros::done_today(&days::key(0))),
            ..State::default()
        };
        let this = mtm.alloc::<Self>().set_ivars(state);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let ui = this.build_ui(mtm);
        this.ivars().ui.set(ui).ok().expect("ui is built once");
        this.ivars().seen_mtime.set(store::modified());
        this.ui().notes.text.setString(&NSString::from_str(&store::load_notes()));
        this.after_note_edit_without_save();
        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            let text: &AnyObject = this.ui().notes.text.as_ref();
            center.addObserver_selector_name_object(this.target(), sel!(notesChanged:), Some(NSTextDidChangeNotification), Some(text));
            center.addObserver_selector_name_object(this.target(), sel!(notesEditingEnded:), Some(NSTextDidEndEditingNotification), Some(text));
            let terminal: &AnyObject = this.ui().terminal.view.as_ref();
            center.addObserver_selector_name_object(
                this.target(), sel!(terminalResigned:), Some(&NSString::from_str(terminal_view::RESIGNED_NOTIFICATION)), Some(terminal),
            );
            // The controller lives for the process. Re-anchor after hot-plug, resolution,
            // primary-display or arrangement changes instead of keeping startup coordinates.
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                this.target(), sel!(screenParametersChanged:),
                Some(NSApplicationDidChangeScreenParametersNotification), None,
            );
            // Lives for the whole process, so the returned timer handle is not kept.
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(2.0, this.target(), sel!(reload:), None, true);
            // Reading the pasteboard's change count is cheap, so copies show up within half a second.
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(0.5, this.target(), sel!(pollClips:), None, true);
        }
        this.refresh();
        this.render_clips();
        this.ui().panel.orderFrontRegardless();
        this
    }

    /// Hotkey entry point: open and focus today's input, or close if already open.
    pub fn toggle_pinned(&self) {
        if self.ivars().expanded.get() {
            self.ivars().pinned.set(false);
            self.set_expanded(false);
        } else {
            self.ivars().tab.set(Tab::Workspace);
            self.refresh();
            self.pin_open();
        }
    }

    fn pin_open(&self) {
        self.ivars().pinned.set(true);
        self.set_expanded(true);
        let ui = self.ui();
        ui.panel.makeKeyAndOrderFront(None);
        ui.panel.makeFirstResponder(Some(&ui.tasks.input));
    }

    fn ui(&self) -> &Ui {
        self.ivars().ui.get().expect("ui is built in new()")
    }

    fn target(&self) -> &AnyObject {
        self.as_ref()
    }

    fn collapse_when_left(&self) {
        if !self.ivars().expanded.get() || self.ivars().pinned.get() || self.note_has_focus() {
            return;
        }
        if self.pointer_over_panel() {
            self.cancel_recheck();
            let none: Option<&AnyObject> = None;
            let _: () = unsafe {
                msg_send![self, performSelector: sel!(recheckHover:), withObject: none, afterDelay: 0.25f64]
            };
        } else {
            self.set_expanded(false);
        }
    }

    fn cancel_recheck(&self) {
        let none: Option<&AnyObject> = None;
        let _: () = unsafe {
            msg_send![
                NSObject::class(),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(recheckHover:),
                object: none
            ]
        };
    }

    /// Tracking areas can report an exit while the pointer is still on the panel
    /// (e.g. mid-animation), so exits are confirmed against the real pointer position.
    /// While open, the full expanded frame counts even if the spring hasn't reached it.
    fn pointer_over_panel(&self) -> bool {
        let p = NSEvent::mouseLocation();
        let ui = self.ui();
        let inside = |f: NSRect| {
            p.x >= f.origin.x && p.x <= f.origin.x + f.size.width && p.y >= f.origin.y && p.y <= f.origin.y + f.size.height
        };
        inside(ui.panel.frame()) || (self.ivars().expanded.get() && inside(ui.expanded.get()))
    }

    fn set_expanded(&self, on: bool) {
        if self.ivars().expanded.replace(on) != on {
            self.ui().motion.set_open(on);
            self.sync_music_timer();
            if !on {
                self.save_notes_now();
                if self.note_has_focus() {
                    self.ui().panel.makeFirstResponder(None);
                }
            }
        }
    }

    /// An open note or the terminal keeps the panel up while typing, even with the
    /// pointer elsewhere.
    fn note_has_focus(&self) -> bool {
        let ui = self.ui();
        let same = |a: &AnyObject, b: &AnyObject| std::ptr::eq(a as *const _ as *const u8, b as *const _ as *const u8);
        let Some(responder) = ui.panel.firstResponder() else { return false };
        ui.panel.isKeyWindow() && (same(&responder, ui.notes.text.as_ref()) || same(&responder, ui.terminal.view.as_ref()))
    }

    /// Adds a typed line as a task; words like `6pm`, `!high` or `tomorrow` fill its fields.
    fn push_task(&self, text: &str, default_day: &str) {
        let task = quick_add::parse(text, default_day, &days::key(0), &days::key(1));
        self.ivars().tasks.borrow_mut().push(task);
        self.save_and_refresh();
    }

    fn after_note_edit(&self) {
        self.after_note_edit_without_save();
        let none: Option<&AnyObject> = None;
        let _: () = unsafe {
            msg_send![
                NSObject::class(),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(saveNotes:),
                object: none
            ]
        };
        let _: () = unsafe { msg_send![self, performSelector: sel!(saveNotes:), withObject: none, afterDelay: NOTES_SAVE_DELAY] };
    }

    fn after_note_edit_without_save(&self) {
        let notes = &self.ui().notes;
        notes.hint.setHidden(notes.text.string().length() > 0);
    }

    fn save_notes_now(&self) {
        if let Err(err) = store::save_notes(&self.ui().notes.text.string().to_string()) {
            eprintln!("notchpad: could not save notes: {err}");
        }
    }

    /// ⌘↩: the line under the caret becomes a task for today and leaves the note.
    fn note_line_to_task(&self) {
        let notes = &self.ui().notes;
        let text = notes.text.string().to_string();
        let (line, rest, caret) = note_view::take_line(&text, notes.text.selectedRange().location);
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.push_task(line, &days::key(0));
        notes.text.setString(&NSString::from_str(&rest));
        notes.text.setSelectedRange(objc2_foundation::NSRange::new(caret, 0));
        self.after_note_edit();
        notes.added.setHidden(false);
        let none: Option<&AnyObject> = None;
        let _: () = unsafe { msg_send![self, performSelector: sel!(clearAdded:), withObject: none, afterDelay: 1.5f64] };
    }

    fn save_and_refresh(&self) {
        self.save();
        self.refresh();
    }

    fn save(&self) {
        if let Err(err) = store::save(&self.ivars().tasks.borrow()) {
            eprintln!("notchpad: could not save tasks: {err}");
        }
        self.ivars().seen_mtime.set(store::modified());
    }

    fn refresh(&self) {
        // Re-read here (not per tick) so the count also rolls over at midnight.
        self.ivars().done_today.set(pomodoros::done_today(&days::key(0)));
        self.render_tabs();
        self.render_focus();
        self.render_task_card();
        self.render_week();
        self.render_music();
    }

    // ---- state queries -------------------------------------------------------------

    fn focused_task(&self) -> Option<usize> {
        let tasks = self.ivars().tasks.borrow();
        let today = days::key(0);
        match self.ivars().focus.get() {
            Some(i) if tasks.get(i).is_some_and(|t| !t.done) => Some(i),
            _ => tasks.iter().position(|t| is_open_today(t, &today)),
        }
    }

    /// Length of the current session: a break, or the chosen focus length.
    fn session_secs(&self) -> u64 {
        let state = self.ivars();
        if state.on_break.get() { state.break_min.get() * 60 } else { DURATIONS_MIN[state.duration_idx.get()] * 60 }
    }

    fn session_active(&self) -> bool {
        self.ivars().deadline.get().is_some() || self.ivars().paused.get().is_some()
    }

    /// Seconds left, rounded up so the display shows the full length when it starts.
    fn remaining_secs(&self) -> u64 {
        let state = self.ivars();
        match (state.deadline.get(), state.paused.get()) {
            (Some(deadline), _) => deadline.saturating_duration_since(Instant::now()).as_secs() + 1,
            (None, Some(left)) => left.as_secs() + 1,
            (None, None) => self.session_secs(),
        }
    }

    fn run_timer(&self, secs: u64) {
        self.ivars().deadline.set(Some(Instant::now() + Duration::from_secs(secs)));
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(1.0, self.target(), sel!(tick:), None, true)
        };
        *self.ivars().ticker.borrow_mut() = Some(timer);
        self.render_focus();
    }

    /// Drops the deadline, any pause and the ticker, so nothing runs while idle.
    fn stop_timer(&self) {
        self.ivars().deadline.set(None);
        self.ivars().paused.set(None);
        if let Some(timer) = self.ivars().ticker.borrow_mut().take() {
            timer.invalidate();
        }
        self.render_focus();
    }

    /// One-second ticks only while someone can see the position move.
    fn sync_music_timer(&self) {
        let state = self.ivars();
        let wanted = state.expanded.get() && state.tab.get() == Tab::Music && state.now_playing.borrow().playing();
        let mut timer = state.music_timer.borrow_mut();
        if wanted && timer.is_none() {
            *timer = Some(unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(1.0, self.target(), sel!(tickMusic:), None, true)
            });
        } else if !wanted && let Some(timer) = timer.take() {
            timer.invalidate();
        }
    }

    // ---- rendering -----------------------------------------------------------------

    fn render_tabs(&self) {
        let ui = self.ui();
        let current = self.ivars().tab.get();
        let tabs: [(&NSView, &NSButton, Tab); 6] = [
            (&ui.workspace, &ui.tab_workspace, Tab::Workspace),
            (&ui.week, &ui.tab_week, Tab::Week),
            (&ui.clips.body, &ui.tab_clips, Tab::Clips),
            (&ui.music.body, &ui.tab_music, Tab::Music),
            (&ui.notes.body, &ui.tab_notes, Tab::Notes),
            (&ui.terminal.body, &ui.tab_terminal, Tab::Terminal),
        ];
        if !ui.content.isHidden() && tabs.iter().any(|(body, _, tab)| body.isHidden() == (*tab == current)) {
            crossfade(&ui.content);
        }
        for (body, pill, tab) in tabs {
            body.setHidden(tab != current);
            if tab == current {
                w::style_pill(pill, &white(1.0), &NSColor::blackColor(), 12.0);
            } else {
                w::style_pill(pill, &white(0.1), &white(0.8), 12.0);
            }
        }
    }

    fn render_music(&self) {
        let view = &self.ui().music;
        {
            let now = self.ivars().now_playing.borrow();
            view.now.setHidden(now.track.is_none());
            view.empty.setHidden(now.track.is_some());
            if let Some(track) = &now.track {
                view.art.setImage(Some(&track.artwork.clone().unwrap_or_else(|| w::symbol("music.note"))));
                view.title.setStringValue(&NSString::from_str(&track.title));
                let credits = [track.artist.as_str(), track.album.as_str()];
                view.subtitle.setStringValue(&NSString::from_str(&credits.iter().filter(|c| !c.is_empty()).copied().collect::<Vec<_>>().join(" · ")));
                view.elapsed.setStringValue(&NSString::from_str(&clock(track.position())));
                view.total.setStringValue(&NSString::from_str(&clock(track.duration)));
                let fraction = if track.duration > 0.0 { track.position() / track.duration } else { 0.0 };
                view.progress.setConstant(MUSIC_PROGRESS_WIDTH * fraction.clamp(0.0, 1.0));
                view.toggle.setImage(Some(&w::symbol(if track.playing { "pause.fill" } else { "play.fill" })));
            }
        }
        self.sync_music_timer();
    }

    /// Rebuilds the cards; called only when the history changes, so a card being dragged
    /// out isn't torn down by unrelated refreshes.
    fn render_clips(&self) {
        let view = &self.ui().clips;
        let history = self.ivars().clips.borrow();
        clear(&view.strip);
        for clip in history.clips() {
            view.strip.addArrangedSubview(&ClipCard::new(clip, self.mtm()));
        }
        let n = history.clips().len();
        view.count.setStringValue(&NSString::from_str(&n.to_string()));
        view.empty.setHidden(n > 0);
    }

    /// Files dragged over the notch open it on Clips; dropped ones become clips.
    fn file_drag(&self, event: FileDrag) {
        match event {
            FileDrag::Entered => {
                self.cancel_recheck();
                self.ivars().tab.set(Tab::Clips);
                self.render_tabs();
                self.set_expanded(true);
            }
            FileDrag::Exited => self.collapse_when_left(),
            FileDrag::Dropped(paths) => {
                self.ivars().clips.borrow_mut().add_files(paths);
                self.render_clips();
                self.collapse_when_left();
            }
        }
    }

    fn render_focus(&self) {
        let card = &self.ui().focus;
        let state = self.ivars();
        let set = |field: &NSTextField, text: &str| field.setStringValue(&NSString::from_str(text));
        let running = state.deadline.get().is_some();
        let active = self.session_active();
        let remaining = self.remaining_secs();
        let done = state.done_today.get();

        set(&card.count, &format!("{done} / {DAILY_GOAL}"));
        for (i, tomato) in card.tomatoes.iter().enumerate() {
            tomato.set(if i < done {
                Tomato::Ripe
            } else if i == done && active && !state.on_break.get() {
                Tomato::Growing
            } else {
                Tomato::Empty
            });
        }
        let task = self.focused_task().map(|i| state.tasks.borrow()[i].text.clone());
        set(&card.task, task.as_deref().unwrap_or("Pick a task with ▶"));
        set(&card.badge, &if !state.on_break.get() {
            format!("Focus · {} of {CYCLE}", done % CYCLE + 1)
        } else if state.break_min.get() == LONG_BREAK_MIN {
            "Break · long".to_owned()
        } else {
            "Break · short".to_owned()
        });
        set(&card.time, &format!("{:02}:{:02}", remaining / 60, remaining % 60));
        set(&card.ends, &match (running, active) {
            (true, _) => format!("ends at {}", days::clock_in(remaining)),
            (false, true) => "paused".to_owned(),
            (false, false) => format!("{} min", remaining / 60),
        });
        card.pause.setTitle(&NSString::from_str(match (running, active) {
            (true, _) => "Pause",
            (false, true) => "Resume",
            (false, false) => "Start",
        }));
        card.end.setEnabled(active);
        card.end.setAlphaValue(if active { 1.0 } else { 0.4 });
        let hint = (!active).then(|| NSString::from_str("Click to change the length"));
        card.time.setToolTip(hint.as_deref());
    }

    fn render_task_card(&self) {
        let card = &self.ui().tasks;
        let today = days::key(0);
        let tasks = self.ivars().tasks.borrow();
        let mut todo: Vec<usize> = (0..tasks.len()).filter(|&i| is_open_today(&tasks[i], &today)).collect();
        // Timed tasks first in clock order; untimed keep their stored order after them.
        let list: &[Task] = &tasks;
        todo.sort_by_key(|&i| (list[i].time.is_empty(), list[i].time.as_str()));
        let done: Vec<usize> = (0..tasks.len()).filter(|&i| tasks[i].done && tasks[i].day == today).collect();

        let show_completed = self.ivars().show_completed.get();
        card.todo.setTitle(&NSString::from_str(&format!("To do  {}", todo.len())));
        card.done.setTitle(&NSString::from_str(&format!("Completed  {}", done.len())));
        let (on, off) = if show_completed { (&card.done, &card.todo) } else { (&card.todo, &card.done) };
        w::style_pill(on, &white(0.45), &ink(0.9), 7.0);
        w::style_pill(off, &white(0.0), &ink(0.55), 7.0);
        w::fit_pill(&card.todo, 10.0);
        w::fit_pill(&card.done, 10.0);

        let before = list_row::snapshot(&card.rows);
        clear(&card.rows);
        let shown = if show_completed { &done } else { &todo };
        let fit = rows_that_fit(&tasks, shown, MAX_CARD_ROWS);
        for (k, &i) in shown.iter().take(fit).enumerate() {
            let row = ListRow::new(&tasks[i].text, &self.card_row(i, &tasks[i]), &ink(0.07), self.mtm());
            unsafe { row.setMenu(Some(&self.task_menu(i))) };
            if k > 0 {
                row.add_rule(&ink(0.08), CARD_ROW_GAP, self.mtm());
            }
            add_full_width(&card.rows, &row, None);
        }
        let empty = if show_completed { "Nothing completed yet today" } else { "All clear. Add a task below." };
        self.add_overflow_note(&card.rows, shown.len(), fit, empty, &ink(0.5));
        list_row::animate_changes(&card.rows, before);
    }

    fn card_row(&self, index: usize, task: &Task) -> Retained<NSView> {
        let mtm = self.mtm();
        let circle = if task.done { "checkmark.circle.fill" } else { "circle" };
        let check = w::icon_button(circle, "", 15.0, &ink(0.7), self.target(), sel!(toggleTask:), mtm);
        check.setTag(index as isize);
        let alpha = if task.done { 0.45 } else { 0.9 };
        let title = with_meta(task, w::label(&task.text, 13.0, unsafe { NSFontWeightRegular }, &ink(alpha), mtm), &ink(0.5), mtm);
        let play = w::icon_button("play.fill", "", 10.0, &ink(0.45), self.target(), sel!(focusTask:), mtm);
        play.setTag(index as isize);
        play.setHidden(task.done);
        w::stack(&[&check, &title, &play], false, 10.0, mtm).into_super()
    }

    fn render_week(&self) {
        let today = days::key(0);
        let tomorrow = days::key(1);
        let tasks = self.ivars().tasks.borrow();
        for (column, ui) in [INBOX, TODAY, TOMORROW].into_iter().zip(&self.ui().columns) {
            let shown: Vec<usize> = (0..tasks.len())
                .filter(|&i| {
                    let t = &tasks[i];
                    match column {
                        INBOX => !t.done && t.day.is_empty(),
                        TODAY => is_open_today(t, &today),
                        _ => !t.done && t.day == tomorrow,
                    }
                })
                .collect();
            ui.count.setStringValue(&NSString::from_str(&shown.len().to_string()));
            let before = list_row::snapshot(&ui.list);
            clear(&ui.list);
            let fit = rows_that_fit(&tasks, &shown, MAX_COLUMN_ROWS);
            for &i in shown.iter().take(fit) {
                let row = ListRow::new(&tasks[i].text, &self.week_row(i, &tasks[i]), &white(0.07), self.mtm());
                unsafe { row.setMenu(Some(&self.task_menu(i))) };
                add_full_width(&ui.list, &row, None);
            }
            self.add_overflow_note(&ui.list, shown.len(), fit, "", &white(0.4));
            list_row::animate_changes(&ui.list, before);
        }
    }

    fn week_row(&self, index: usize, task: &Task) -> Retained<NSView> {
        let mtm = self.mtm();
        let check = w::icon_button("circle", "", 14.0, &white(0.55), self.target(), sel!(toggleTask:), mtm);
        check.setTag(index as isize);
        let title = with_meta(task, w::label(&task.text, 13.0, unsafe { NSFontWeightRegular }, &white(0.9), mtm), &white(0.45), mtm);
        let content = w::stack(&[&check, &title], false, 8.0, mtm);
        TaskRow::new(index, &content, mtm).into_super()
    }

    /// Right-click menu for the task at `index`; each item carries the index in its tag.
    fn task_menu(&self, index: usize) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::new());
        menu.setAutoenablesItems(false);
        let add = |title: &str, action| {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(title), Some(action), &NSString::new())
            };
            unsafe { item.setTarget(Some(self.target())) };
            item.setTag(index as isize);
            menu.addItem(&item);
        };
        add("Move to Inbox", sel!(moveToInbox:));
        add("Move to Today", sel!(moveToToday:));
        add("Move to Tomorrow", sel!(moveToTomorrow:));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Delete", sel!(deleteTask:));
        menu
    }

    /// Sets the day of the task at `index` (empty = Inbox), keeping its place in the list.
    fn reschedule(&self, index: usize, day: String) {
        if let Some(task) = self.ivars().tasks.borrow_mut().get_mut(index) {
            task.day = day;
        }
        self.save_and_refresh();
    }

    /// Moves a dragged task into `column`, placed above the first row whose middle is
    /// below the drop point, and reschedules it for that column's day.
    fn drop_task(&self, index: usize, column: isize, at: NSPoint) {
        let before = list_row::snapshot(&self.ui().columns[column as usize].list)
            .into_iter()
            .filter_map(|(_, row)| row.content().downcast::<TaskRow>().ok())
            .find(|row| {
                let frame = row.convertRect_toView(row.bounds(), None);
                at.y > frame.origin.y + frame.size.height / 2.0
            })
            .map(|row| row.index());
        {
            let mut tasks = self.ivars().tasks.borrow_mut();
            if index >= tasks.len() {
                return;
            }
            let mut task = tasks.remove(index);
            task.day = match column {
                INBOX => String::new(),
                TOMORROW => days::key(1),
                _ => days::key(0),
            };
            let to = match before {
                Some(b) if b > index => b - 1,
                Some(b) => b,
                None => tasks.len(),
            };
            tasks.insert(to, task);
            // The focus card refers to tasks by index, so follow the shift.
            let focus = self.ivars().focus.get().map(|f| match f {
                f if f == index => to,
                f => {
                    let f = if f > index { f - 1 } else { f };
                    if f >= to { f + 1 } else { f }
                }
            });
            self.ivars().focus.set(focus);
        }
        self.save();
        // Rebuilding the rows now would tear down the drag source while AppKit is still
        // finishing its session, so re-render on the next run-loop pass.
        let none: Option<&AnyObject> = None;
        let _: () = unsafe { msg_send![self, performSelector: sel!(redraw:), withObject: none, afterDelay: 0.0f64] };
    }

    fn add_overflow_note(&self, list: &NSStackView, total: usize, max: usize, empty: &str, color: &NSColor) {
        let text = if total > max {
            format!("+{} more", total - max)
        } else if total == 0 && !empty.is_empty() {
            empty.to_owned()
        } else {
            return;
        };
        list.addArrangedSubview(&w::label(&text, 12.0, unsafe { NSFontWeightRegular }, color, self.mtm()));
    }

    // ---- building ------------------------------------------------------------------

    fn build_ui(&self, mtm: MainThreadMarker) -> Ui {
        let (collapsed, expanded, safe_top) = notch_rects(mtm);

        let panel_alloc = mtm.alloc::<NotchPanel>().set_ivars(());
        let panel: Retained<NotchPanel> = unsafe {
            msg_send![
                super(panel_alloc),
                initWithContentRect: collapsed,
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setLevel(NSMainMenuWindowLevel + 1);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());

        let me = Weak::new(self);
        let on_file_drag = Box::new(move |event| {
            if let Some(controller) = me.load() {
                controller.file_drag(event);
            }
        });
        let background = NotchShape::new(collapsed, on_file_drag, mtm);
        panel.setContentView(Some(&background));
        let tracking = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                mtm.alloc(),
                NSRect::ZERO,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(self.target()),
                None,
            )
        };
        background.addTrackingArea(&tracking);

        // Fixed-size content laid out once at full size; hidden while collapsed.
        let ear = panel::ear(HEIGHT + TOP_SLOP);
        let content = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::new(ear, 0.0), NSSize::new(WIDTH, HEIGHT)));
        content.setHidden(true);
        background.addSubview(&content);

        // The header sits in the menu-bar band: close top-left as on macOS windows, then two
        // tab groups, one each side of the hardware notch.
        let head = self.build_header(mtm);
        let header = NSView::new(mtm);
        header.setTranslatesAutoresizingMaskIntoConstraints(false);
        content.addSubview(&header);
        header.leadingAnchor().constraintEqualToAnchor(&content.leadingAnchor()).setActive(true);
        header.trailingAnchor().constraintEqualToAnchor(&content.trailingAnchor()).setActive(true);
        header.topAnchor().constraintEqualToAnchor(&content.topAnchor()).setActive(true);
        let header_height = header.heightAnchor().constraintEqualToConstant(header_band(safe_top));
        header_height.setActive(true);
        for group in [&*head.quit, &**head.left, &**head.right] {
            group.setTranslatesAutoresizingMaskIntoConstraints(false);
            header.addSubview(group);
            group.centerYAnchor().constraintEqualToAnchor(&header.centerYAnchor()).setActive(true);
        }
        head.quit.leadingAnchor().constraintEqualToAnchor_constant(&header.leadingAnchor(), 14.0).setActive(true);
        head.left.leadingAnchor().constraintEqualToAnchor_constant(&head.quit.trailingAnchor(), 12.0).setActive(true);
        head.right.trailingAnchor().constraintEqualToAnchor_constant(&header.trailingAnchor(), -14.0).setActive(true);
        // The panel is centred on the screen, so the header's centre is the notch's centre.
        let gap = notch_gap(collapsed, safe_top);
        let notch_gaps = [
            header.centerXAnchor().constraintGreaterThanOrEqualToAnchor_constant(&head.left.trailingAnchor(), gap),
            head.right.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&header.centerXAnchor(), gap),
        ];
        for constraint in &notch_gaps {
            constraint.setActive(true);
        }

        let (focus_view, focus) = self.build_focus_card(mtm);
        let (task_view, tasks) = self.build_task_card(mtm);
        let workspace = w::stack(&[&focus_view, &task_view], false, 10.0, mtm);
        workspace.setDistribution(NSStackViewDistribution::Fill);
        task_view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        let (week, columns) = self.build_week(mtm);
        let clips = self.build_clips(mtm);
        let music = self.build_music(mtm);
        let notes = self.build_notes(mtm);
        let terminal = self.build_terminal(mtm);
        for body in [&*workspace, &*week, &*clips.body, &*music.body, &*notes.body, &*terminal.body] {
            content.addSubview(body);
            body.leadingAnchor().constraintEqualToAnchor_constant(&content.leadingAnchor(), 12.0).setActive(true);
            body.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), -12.0).setActive(true);
            body.topAnchor().constraintEqualToAnchor_constant(&header.bottomAnchor(), 8.0).setActive(true);
            body.bottomAnchor().constraintEqualToAnchor_constant(&content.bottomAnchor(), -12.0).setActive(true);
        }
        for card in [&focus_view, &task_view] {
            card.heightAnchor().constraintEqualToAnchor(&workspace.heightAnchor()).setActive(true);
        }

        let motion = NotchMotion::new(panel.clone(), content.clone(), collapsed, expanded, mtm);
        Ui {
            panel,
            content,
            workspace,
            week,
            tab_workspace: head.workspace,
            tab_week: head.week,
            tab_clips: head.clips,
            tab_music: head.music,
            tab_notes: head.notes,
            tab_terminal: head.terminal,
            clips,
            music,
            notes,
            terminal,
            focus,
            tasks,
            columns,
            expanded: Cell::new(expanded),
            header_height,
            notch_gaps,
            motion,
        }
    }

    fn build_header(&self, mtm: MainThreadMarker) -> Header {
        let pill = |title: &str, action| {
            let pill = w::pill(title, 24.0, self.target(), action, mtm);
            w::fit_pill(&pill, 10.0);
            pill
        };
        let icon_pill = |symbol: &str, action| {
            let pill = w::pill("", 24.0, self.target(), action, mtm);
            pill.setImage(Some(&w::symbol(symbol)));
            pill.setImagePosition(NSCellImagePosition::ImageOnly);
            pill.widthAnchor().constraintEqualToConstant(ICON_TAB_WIDTH).setActive(true);
            pill
        };
        let workspace = pill("Workspace", sel!(showWorkspace:));
        let week = pill("Week", sel!(showWeek:));
        let clips = pill("Clips", sel!(showClips:));
        let music = icon_pill("music.note", sel!(showMusic:));
        let notes = icon_pill("note.text", sel!(showNotes:));
        let terminal = pill("Terminal", sel!(showTerminal:));
        Header {
            quit: w::icon_button("xmark", "", 10.0, &white(0.55), self.target(), sel!(quit:), mtm),
            left: w::stack(&[&workspace, &week], false, 6.0, mtm),
            right: w::stack(&[&clips, &music, &notes, &terminal], false, 6.0, mtm),
            workspace,
            week,
            clips,
            music,
            notes,
            terminal,
        }
    }

    fn build_focus_card(&self, mtm: MainThreadMarker) -> (Retained<NSView>, FocusCard) {
        let card = w::filled(&rgba(34, 33, 38, 1.0), 18.0, mtm);
        card.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());
        card.widthAnchor().constraintEqualToConstant(FOCUS_CARD_WIDTH).setActive(true);
        let inner = FOCUS_CARD_WIDTH - 28.0;
        let coral = rgba(CORAL.0, CORAL.1, CORAL.2, 1.0);

        let heading = w::label("Today's pomodoros", 12.0, unsafe { NSFontWeightSemibold }, &white(0.92), mtm);
        let (count, count_chip) = chip(&white(0.9), &rgba(0, 0, 0, 0.4), 7.0, 3.0, mtm);
        // Too long for the card, so it lives here instead.
        count_chip.setToolTip(Some(&NSString::from_str("Completed pomodoros count toward today; ended early are logged, not counted.")));
        count.setContentCompressionResistancePriority_forOrientation(751.0, NSLayoutConstraintOrientation::Horizontal);
        count.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
        let header = w::stack(&[&heading, &w::spacer(mtm), &count_chip], false, 8.0, mtm);

        let tomatoes: Vec<_> = (0..DAILY_GOAL).map(|_| TomatoView::new(mtm)).collect();
        let rows: Vec<_> = tomatoes
            .chunks(DAILY_GOAL / 2)
            .map(|row| {
                let views: Vec<&NSView> = row.iter().map(|t| &***t).collect();
                for view in &views {
                    fixed_size(view, TOMATO_SIZE, TOMATO_SIZE);
                }
                let stack = w::stack(&views, false, 0.0, mtm);
                stack.setDistribution(NSStackViewDistribution::EqualSpacing);
                stack
            })
            .collect();
        for row in &rows {
            row.widthAnchor().constraintEqualToConstant(inner).setActive(true);
        }
        let grid = w::stack(&[&rows[0], &rows[1]], true, 4.0, mtm);

        let task = w::label("", 11.0, unsafe { NSFontWeightRegular }, &white(0.55), mtm);
        task.setAlignment(NSTextAlignment::Center);
        let (badge, badge_pill) = chip(&rgba(CORAL.0, CORAL.1, CORAL.2, 1.0), &rgba(CORAL.0, CORAL.1, CORAL.2, 0.22), 6.0, 3.0, mtm);
        badge.setFont(Some(&NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
        let time = w::label("", 36.0, unsafe { NSFontWeightLight }, &white(0.95), mtm);
        time.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(36.0, unsafe { NSFontWeightLight })));
        // With the timer tile gone, the idle countdown itself is the length control.
        let cycle = unsafe { NSClickGestureRecognizer::initWithTarget_action(mtm.alloc(), Some(self.target()), Some(sel!(cycleDuration:))) };
        time.addGestureRecognizer(&cycle);
        let ends = w::label("", 11.0, unsafe { NSFontWeightRegular }, &white(0.55), mtm);

        let pause = w::pill("Pause", 28.0, self.target(), sel!(toggleTimer:), mtm);
        w::style_pill(&pause, &coral, &white(1.0), 14.0);
        pause.widthAnchor().constraintEqualToConstant((inner - 8.0) / 2.0).setActive(true);
        let end = w::pill("End", 28.0, self.target(), sel!(endTimer:), mtm);
        w::style_pill(&end, &rgba(62, 60, 68, 1.0), &white(0.85), 14.0);
        end.widthAnchor().constraintEqualToConstant((inner - 8.0) / 2.0).setActive(true);
        let buttons = w::stack(&[&pause, &end], false, 8.0, mtm);

        let column = w::stack(&[&header, &grid, &task, &badge_pill, &time, &ends, &buttons], true, 6.0, mtm);
        column.setAlignment(NSLayoutAttribute::CenterX);
        column.setCustomSpacing_afterView(10.0, &header);
        column.setCustomSpacing_afterView(10.0, &task);
        column.setCustomSpacing_afterView(0.0, &time);
        column.setCustomSpacing_afterView(10.0, &ends);
        card.addSubview(&column);
        column.centerXAnchor().constraintEqualToAnchor(&card.centerXAnchor()).setActive(true);
        column.centerYAnchor().constraintEqualToAnchor(&card.centerYAnchor()).setActive(true);
        for view in [&**header, &**grid, &**buttons, &**task] {
            view.widthAnchor().constraintEqualToConstant(inner).setActive(true);
        }

        (card, FocusCard { count, tomatoes, task, badge, time, ends, pause, end })
    }

    fn build_task_card(&self, mtm: MainThreadMarker) -> (Retained<NSView>, TaskCard) {
        let card = w::filled(&rgba(148, 168, 143, 1.0), 18.0, mtm);
        card.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) }.as_deref());

        let title = w::label("Today’s tasks", 15.0, unsafe { NSFontWeightBold }, &ink(0.9), mtm);
        let date = w::label(&days::label(0), 12.0, unsafe { NSFontWeightMedium }, &ink(0.55), mtm);
        let head = w::stack(&[&title, &w::spacer(mtm), &date], false, 8.0, mtm);
        let todo = w::pill("To do", 22.0, self.target(), sel!(showTodo:), mtm);
        let done = w::pill("Completed", 22.0, self.target(), sel!(showCompleted:), mtm);
        let pills = w::stack(&[&todo, &done], false, 4.0, mtm);
        let rows = w::stack(&[], true, CARD_ROW_GAP, mtm);
        let top = w::stack(&[&head, &pills, &rows], true, 10.0, mtm);
        card.addSubview(&top);
        w::pin(&top, &card, 14.0, None);
        for view in [&*head, &*rows] {
            view.widthAnchor().constraintEqualToAnchor(&top.widthAnchor()).setActive(true);
        }

        let input = w::input("Add a task · try 6pm !high #work 30m", &ink(0.9), TODAY, self.target(), sel!(addTask:), mtm);
        let bottom = self.add_row(&input, &ink(0.5), mtm);
        let line = w::filled(&ink(0.1), 0.0, mtm);
        card.addSubview(&line);
        card.addSubview(&bottom);
        for view in [&*line, &*bottom] {
            view.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), 14.0).setActive(true);
            view.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -14.0).setActive(true);
        }
        line.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
        line.bottomAnchor().constraintEqualToAnchor_constant(&bottom.topAnchor(), -8.0).setActive(true);
        bottom.bottomAnchor().constraintEqualToAnchor_constant(&card.bottomAnchor(), -10.0).setActive(true);

        (card, TaskCard { todo, done, rows, input })
    }

    fn build_week(&self, mtm: MainThreadMarker) -> (Retained<NSStackView>, Vec<Column>) {
        let week = w::stack(&[], false, 10.0, mtm);
        week.setDistribution(NSStackViewDistribution::FillEqually);
        let mut columns = Vec::new();
        for (tag, offset) in [(INBOX, 0), (TODAY, 0), (TOMORROW, 1)] {
            let me = Weak::new(self);
            let on_drop = Box::new(move |index, at| {
                if let Some(controller) = me.load() {
                    controller.drop_task(index, tag, at);
                }
            });
            let card = DropColumn::new(rgba(42, 42, 45, 1.0), rgba(62, 62, 68, 1.0), on_drop, mtm);
            let title: Retained<NSView> = match tag {
                INBOX => {
                    let tray = NSImageView::imageViewWithImage(&w::symbol("tray"), mtm);
                    tray.setContentTintColor(Some(&white(0.9)));
                    let text = w::label("Inbox", 13.0, unsafe { NSFontWeightSemibold }, &white(0.95), mtm);
                    w::stack(&[&tray, &text], false, 6.0, mtm).into_super()
                }
                TODAY => {
                    let pill = w::pill("Today", 20.0, self.target(), sel!(showWorkspace:), mtm);
                    w::fit_pill(&pill, 8.0);
                    w::style_pill(&pill, &white(1.0), &NSColor::blackColor(), 10.0);
                    pill.into_super().into_super()
                }
                _ => w::label("Tomorrow", 13.0, unsafe { NSFontWeightSemibold }, &white(0.95), mtm).into_super().into_super(),
            };
            let date_text = if tag == INBOX { String::new() } else { days::label(offset) };
            let date = w::label(&date_text, 12.0, unsafe { NSFontWeightRegular }, &white(0.45), mtm);
            let count = w::label("0", 12.0, unsafe { NSFontWeightMedium }, &white(0.45), mtm);
            let head = w::stack(&[&title, &date, &w::spacer(mtm), &count], false, 6.0, mtm);
            let list = w::stack(&[], true, 8.0, mtm);
            let input = w::input("Add task", &white(0.9), tag, self.target(), sel!(addTask:), mtm);
            let add = self.add_row(&input, &white(0.45), mtm);
            let inner = w::stack(&[&head, &list, &add], true, 12.0, mtm);
            card.addSubview(&inner);
            w::pin(&inner, &card, 12.0, None);
            for view in [&*head, &*list, &*add] {
                view.widthAnchor().constraintEqualToAnchor(&inner.widthAnchor()).setActive(true);
            }
            week.addArrangedSubview(&card);
            card.heightAnchor().constraintEqualToAnchor(&week.heightAnchor()).setActive(true);
            columns.push(Column { list, count });
        }
        (week, columns)
    }

    /// Title row, then the clips hanging from a line in a sideways-scrolling strip, then a hint.
    fn build_clips(&self, mtm: MainThreadMarker) -> ClipsView {
        let title = w::label("Clipboard", 15.0, unsafe { NSFontWeightBold }, &white(0.95), mtm);
        let count = w::label("0", 12.0, unsafe { NSFontWeightMedium }, &white(0.45), mtm);
        let clear_all = w::pill("Clear", 22.0, self.target(), sel!(clearClips:), mtm);
        w::fit_pill(&clear_all, 10.0);
        w::style_pill(&clear_all, &white(0.1), &white(0.8), 11.0);
        let head = w::stack(&[&title, &count, &w::spacer(mtm), &clear_all], false, 8.0, mtm);

        let strip = w::stack(&[], false, 10.0, mtm);
        strip.setAlignment(NSLayoutAttribute::Top);
        let scroll = NSScrollView::new(mtm);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setDrawsBackground(false);
        scroll.setHasHorizontalScroller(false);
        scroll.setDocumentView(Some(&strip));
        let clip_view = scroll.contentView();
        strip.leadingAnchor().constraintEqualToAnchor(&clip_view.leadingAnchor()).setActive(true);
        strip.topAnchor().constraintEqualToAnchor(&clip_view.topAnchor()).setActive(true);
        strip.heightAnchor().constraintEqualToAnchor(&clip_view.heightAnchor()).setActive(true);
        // Room above the cards for their pegs, which straddle the line.
        scroll.heightAnchor().constraintEqualToConstant(clip_card::HEIGHT + 4.0).setActive(true);
        strip.setEdgeInsets(objc2_foundation::NSEdgeInsets { top: 2.0, left: 6.0, bottom: 2.0, right: 6.0 });

        let empty = w::label("Copy something, take a screenshot, or drop a file on the notch.", 12.0, unsafe { NSFontWeightRegular }, &white(0.45), mtm);
        let hint = w::label("Click a clip to copy it · drag it into any app", 11.0, unsafe { NSFontWeightRegular }, &white(0.35), mtm);

        let body = w::stack(&[&head, &scroll, &hint], true, 8.0, mtm);
        body.setCustomSpacing_afterView(4.0, &scroll);
        for view in [&**head, &**scroll] {
            view.widthAnchor().constraintEqualToAnchor(&body.widthAnchor()).setActive(true);
        }
        // Behind the cards, through the middle of their pegs.
        let line = w::filled(&white(0.18), 0.5, mtm);
        body.addSubview_positioned_relativeTo(&line, NSWindowOrderingMode::Below, None);
        line.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
        line.leadingAnchor().constraintEqualToAnchor(&body.leadingAnchor()).setActive(true);
        line.trailingAnchor().constraintEqualToAnchor(&body.trailingAnchor()).setActive(true);
        line.centerYAnchor().constraintEqualToAnchor_constant(&scroll.topAnchor(), 2.0 + clip_card::PEG_HEIGHT / 2.0).setActive(true);
        // On the body, not the scroll view, which lays out its own subviews.
        body.addSubview(&empty);
        empty.setTranslatesAutoresizingMaskIntoConstraints(false);
        empty.centerXAnchor().constraintEqualToAnchor(&scroll.centerXAnchor()).setActive(true);
        empty.centerYAnchor().constraintEqualToAnchor(&scroll.centerYAnchor()).setActive(true);
        ClipsView { body: body.into_super(), strip, count, empty }
    }

    /// Artwork on the left; title, credits, progress and transport controls on the right.
    fn build_music(&self, mtm: MainThreadMarker) -> MusicView {
        let art = NSImageView::new(mtm);
        art.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        art.setContentTintColor(Some(&white(0.4)));
        fixed_size(&art, ARTWORK_SIZE, ARTWORK_SIZE);
        w::paint(&art, &white(0.08), 14.0);
        if let Some(layer) = art.layer() {
            layer.setMasksToBounds(true);
        }

        let title = w::label("", 17.0, unsafe { NSFontWeightBold }, &white(0.95), mtm);
        let subtitle = w::label("", 12.0, unsafe { NSFontWeightRegular }, &white(0.6), mtm);
        for text in [&title, &subtitle] {
            text.widthAnchor().constraintLessThanOrEqualToConstant(MUSIC_PROGRESS_WIDTH).setActive(true);
        }

        let track = w::filled(&white(0.15), 1.5, mtm);
        fixed_size(&track, MUSIC_PROGRESS_WIDTH, 3.0);
        let fill = w::filled(&white(0.85), 1.5, mtm);
        track.addSubview(&fill);
        fill.leadingAnchor().constraintEqualToAnchor(&track.leadingAnchor()).setActive(true);
        fill.topAnchor().constraintEqualToAnchor(&track.topAnchor()).setActive(true);
        fill.bottomAnchor().constraintEqualToAnchor(&track.bottomAnchor()).setActive(true);
        let progress = fill.widthAnchor().constraintEqualToConstant(0.0);
        progress.setActive(true);

        let elapsed = w::label("0:00", 11.0, unsafe { NSFontWeightRegular }, &white(0.45), mtm);
        let total = w::label("0:00", 11.0, unsafe { NSFontWeightRegular }, &white(0.45), mtm);
        let times = w::stack(&[&elapsed, &w::spacer(mtm), &total], false, 0.0, mtm);
        times.widthAnchor().constraintEqualToConstant(MUSIC_PROGRESS_WIDTH).setActive(true);

        let button = |symbol: &str, size: f64, action| {
            let button = w::icon_button(symbol, "", size, &white(0.9), self.target(), action, mtm);
            fixed_size(&button, 40.0, 36.0);
            button
        };
        let previous = button("backward.fill", 16.0, sel!(musicPrevious:));
        let toggle = button("play.fill", 22.0, sel!(musicPlayPause:));
        let next = button("forward.fill", 16.0, sel!(musicNext:));
        let controls = w::stack(&[&previous, &toggle, &next], false, 10.0, mtm);

        let column = w::stack(&[&title, &subtitle, &track, &times, &controls], true, 6.0, mtm);
        column.setCustomSpacing_afterView(12.0, &subtitle);
        column.setCustomSpacing_afterView(10.0, &times);
        let now = w::stack(&[&art, &column], false, 20.0, mtm);

        let empty = w::label("Nothing playing.", 12.0, unsafe { NSFontWeightRegular }, &white(0.45), mtm);
        let body = NSView::new(mtm);
        body.setTranslatesAutoresizingMaskIntoConstraints(false);
        for view in [&**now, &**empty] {
            body.addSubview(view);
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            view.centerXAnchor().constraintEqualToAnchor(&body.centerXAnchor()).setActive(true);
            view.centerYAnchor().constraintEqualToAnchor(&body.centerYAnchor()).setActive(true);
        }
        MusicView { body, now, empty, art, title, subtitle, progress, elapsed, total, toggle }
    }

    /// One dark card holding the scrolling text view, with the placeholder and the ⌘↩
    /// confirmation laid over it.
    fn build_notes(&self, mtm: MainThreadMarker) -> NotesView {
        let card = w::filled(&rgba(42, 42, 45, 1.0), 18.0, mtm);
        let me = Weak::new(self);
        let text = NoteView::new(
            Box::new(move || {
                if let Some(controller) = me.load() {
                    controller.note_line_to_task();
                }
            }),
            mtm,
        );
        text.setRichText(false);
        text.setAutomaticQuoteSubstitutionEnabled(false);
        text.setAutomaticDashSubstitutionEnabled(false);
        text.setAutomaticTextReplacementEnabled(false);
        text.setDrawsBackground(false);
        text.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        text.setTextColor(Some(&white(0.9)));
        text.setInsertionPointColor(Some(&white(0.9)));
        text.setTextContainerInset(NSSize::new(0.0, 0.0));
        text.setVerticallyResizable(true);
        text.setHorizontallyResizable(false);
        text.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);

        let scroll = NSScrollView::new(mtm);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDocumentView(Some(&text));
        card.addSubview(&scroll);
        scroll.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), 16.0).setActive(true);
        scroll.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -16.0).setActive(true);
        scroll.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 14.0).setActive(true);
        scroll.bottomAnchor().constraintEqualToAnchor_constant(&card.bottomAnchor(), -14.0).setActive(true);

        let hint = w::label("Jot anything · ⌘↩ turns a line into a task", 13.0, unsafe { NSFontWeightRegular }, &white(0.35), mtm);
        let added = w::label("Added to Today", 12.0, unsafe { NSFontWeightMedium }, &white(0.7), mtm);
        added.setHidden(true);
        for label in [&hint, &added] {
            label.setTranslatesAutoresizingMaskIntoConstraints(false);
            card.addSubview(label);
        }
        hint.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), 16.0).setActive(true);
        hint.topAnchor().constraintEqualToAnchor_constant(&card.topAnchor(), 14.0).setActive(true);
        added.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -16.0).setActive(true);
        added.bottomAnchor().constraintEqualToAnchor_constant(&card.bottomAnchor(), -12.0).setActive(true);

        NotesView { body: card, text, hint, added }
    }

    /// A card holding the terminal view.
    fn build_terminal(&self, mtm: MainThreadMarker) -> TerminalPane {
        let body = w::filled(&rgba(42, 42, 45, 1.0), 18.0, mtm);
        let view = TerminalView::new(mtm);
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        body.addSubview(&view);
        view.leadingAnchor().constraintEqualToAnchor_constant(&body.leadingAnchor(), 14.0).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor_constant(&body.trailingAnchor(), -14.0).setActive(true);
        view.topAnchor().constraintEqualToAnchor_constant(&body.topAnchor(), 12.0).setActive(true);
        view.bottomAnchor().constraintEqualToAnchor_constant(&body.bottomAnchor(), -12.0).setActive(true);
        TerminalPane { body, view }
    }

    /// "+ <input>" row used under every task list.
    fn add_row(&self, input: &NSTextField, tint: &NSColor, mtm: MainThreadMarker) -> Retained<NSStackView> {
        let plus = NSImageView::imageViewWithImage(&w::symbol("plus"), mtm);
        plus.setContentTintColor(Some(tint));
        input.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        w::stack(&[&plus, input], false, 6.0, mtm)
    }
}

/// Rounded label on a tinted fill, returned with the pill that holds it.
fn chip(color: &NSColor, fill: &NSColor, h_pad: f64, v_pad: f64, mtm: MainThreadMarker) -> (Retained<NSTextField>, Retained<NSView>) {
    let label = w::label("", 11.0, unsafe { NSFontWeightMedium }, color, mtm);
    let pill = w::filled(fill, 8.0, mtm);
    pill.addSubview(&label);
    label.setTranslatesAutoresizingMaskIntoConstraints(false);
    label.leadingAnchor().constraintEqualToAnchor_constant(&pill.leadingAnchor(), h_pad).setActive(true);
    label.trailingAnchor().constraintEqualToAnchor_constant(&pill.trailingAnchor(), -h_pad).setActive(true);
    label.topAnchor().constraintEqualToAnchor_constant(&pill.topAnchor(), v_pad).setActive(true);
    label.bottomAnchor().constraintEqualToAnchor_constant(&pill.bottomAnchor(), -v_pad).setActive(true);
    (label, pill)
}

/// `m:ss` for a position or duration in seconds.
fn clock(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Open task scheduled for today or rolled over from an earlier day (Inbox excluded).
fn is_open_today(task: &Task, today: &str) -> bool {
    !task.done && !task.day.is_empty() && task.day.as_str() <= today
}

/// Crossfades everything `view` shows from now to its next drawn state.
fn crossfade(view: &NSView) {
    let fade = CATransition::new();
    fade.setType(unsafe { kCATransitionFade });
    fade.setDuration(0.18);
    if let Some(layer) = view.layer() {
        layer.addAnimation_forKey(&fade, None);
    }
}

/// The row's title, stretched to fill the row, with a dim metadata line under it when the
/// task has a time, priority, project or estimate.
fn with_meta(task: &Task, title: Retained<NSTextField>, color: &NSColor, mtm: MainThreadMarker) -> Retained<NSView> {
    let meta = meta_line(task);
    let view: Retained<NSView> = if meta.is_empty() {
        title.into_super().into_super()
    } else {
        let line = w::label(&meta, 11.0, unsafe { NSFontWeightRegular }, color, mtm);
        let stack = w::stack(&[&title, &line], true, 1.0, mtm);
        // A stack view hugs its content through its own priority, not the NSView one.
        stack.setHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        stack.into_super()
    };
    view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    view
}

/// `5:00 PM · High · Home · 45 min`, skipping parts the task doesn't have.
fn meta_line(task: &Task) -> String {
    let mut parts = Vec::new();
    if let Some((h, m)) = task.time.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m))) {
        let suffix = if h < 12 { "AM" } else { "PM" };
        parts.push(format!("{}:{m} {suffix}", (h + 11) % 12 + 1));
    }
    let mut priority = task.priority.chars();
    if let Some(first) = priority.next() {
        parts.push(first.to_uppercase().chain(priority).collect());
    }
    if !task.project.is_empty() {
        parts.push(task.project.clone());
    }
    if task.minutes > 0 {
        parts.push(format!("{} min", task.minutes));
    }
    parts.join(" · ")
}

/// How many of `shown` fit in a list sized for `max_rows` plain rows. A row with a metadata
/// line is about 1.5 times as tall, so rows are counted in half-row units.
fn rows_that_fit(tasks: &[Task], shown: &[usize], max_rows: usize) -> usize {
    let mut used = 0;
    shown
        .iter()
        .take_while(|&&i| {
            used += if meta_line(&tasks[i]).is_empty() { 2 } else { 3 };
            used <= 2 * max_rows
        })
        .count()
}

fn clear(stack: &NSStackView) {
    for view in stack.arrangedSubviews().iter() {
        stack.removeArrangedSubview(&view);
        view.removeFromSuperview();
    }
}

/// Appends `view` stretched to the stack's width, optionally with a fixed height.
fn add_full_width(stack: &NSStackView, view: &NSView, height: Option<f64>) {
    stack.addArrangedSubview(view);
    view.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);
    if let Some(h) = height {
        view.heightAnchor().constraintEqualToConstant(h).setActive(true);
    }
}

fn fixed_size(view: &NSView, width: f64, height: f64) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.widthAnchor().constraintEqualToConstant(width).setActive(true);
    view.heightAnchor().constraintEqualToConstant(height).setActive(true);
}

/// Returns (collapsed rect over the notch, expanded rect, hardware safe-area height).
fn notch_rects(mtm: MainThreadMarker) -> (NSRect, NSRect, f64) {
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| s.safeAreaInsets().top > 0.0)
        .or_else(|| NSScreen::mainScreen(mtm))
        .expect("no screen available");
    let frame = screen.frame();
    let inset = screen.safeAreaInsets().top;
    let (width, height) = if inset > 0.0 {
        let left = screen.auxiliaryTopLeftArea().size.width;
        let right = screen.auxiliaryTopRightArea().size.width;
        (frame.size.width - left - right, inset)
    } else {
        // No hardware notch (external display): draw one of our own.
        DEFAULT_NOTCH
    };
    let (collapsed, expanded) = frames_at_top(frame, width, height);
    (collapsed, expanded, inset)
}

/// The header fills the menu-bar band on a notched display, so it never sits under the notch.
fn header_band(safe_top: f64) -> f64 {
    safe_top.max(32.0)
}

/// Distance from the panel's centre to where the tabs may start: clear of the hardware
/// notch (the collapsed panel covers it exactly), or nothing on a display without one.
fn notch_gap(collapsed: NSRect, safe_top: f64) -> f64 {
    if safe_top > 0.0 { collapsed.size.width / 2.0 + 8.0 } else { 0.0 }
}

fn frames_at_top(frame: NSRect, width: f64, height: f64) -> (NSRect, NSRect) {
    let top = frame.origin.y + frame.size.height;
    let mid = frame.origin.x + frame.size.width / 2.0;
    // Body plus an ear on each side, extended TOP_SLOP above the screen.
    let rect = |w: f64, h: f64| {
        let ear = panel::ear(h + TOP_SLOP);
        NSRect::new(NSPoint::new(mid - w / 2.0 - ear, top - h), NSSize::new(w + 2.0 * ear, h + TOP_SLOP))
    };
    (rect(width, height), rect(WIDTH, HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_tracks_screen_top_and_center_in_every_arrangement() {
        // Main, above, below, left and right: AppKit coordinates need not start at zero.
        for (x, y, w, h) in [
            (0.0, 0.0, 1800.0, 1169.0),
            (-411.0, 1169.0, 2560.0, 1440.0),
            (0.0, -1440.0, 2560.0, 1440.0),
            (-2560.0, 0.0, 2560.0, 1440.0),
            (1800.0, 200.0, 2560.0, 1440.0),
        ] {
            let screen = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
            for (width, height) in [DEFAULT_NOTCH, (185.0, 38.0)] {
                let (collapsed, expanded) = frames_at_top(screen, width, height);
                for rect in [collapsed, expanded] {
                    assert_eq!(rect.origin.y + rect.size.height, y + h + TOP_SLOP);
                    assert_eq!(rect.origin.x + rect.size.width / 2.0, x + w / 2.0);
                }
                assert_eq!(collapsed.size.height, height + TOP_SLOP);
                assert_eq!(expanded.size.height, HEIGHT + TOP_SLOP);
            }
        }
    }
}
