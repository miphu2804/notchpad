//! Owns the notch window, the tasks and the focus timer, and renders the notch's tabs. Every change re-renders from `State`; the views hold no state of their own.

use std::cell::{Cell, OnceCell, RefCell};
use std::time::{Duration, Instant};


use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSClickGestureRecognizer, NSControl, NSControlTextEditingDelegate, NSTextFieldDelegate, NSTextView, NSLineBreakMode, NSPasteboard, NSPasteboardTypeString, NSControlStateValueOn, NSForegroundColorAttributeName, NSStrikethroughStyleAttributeName, NSUnderlineStyle, NSSound, 
    NSAppearance, NSImageScaling, NSAppearanceCustomization, NSAppearanceNameDarkAqua,
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType, NSButton, NSCellImagePosition, NSColor, NSEvent, NSFont,
    NSFontWeightBold, NSFontWeightLight, NSFontWeightMedium, NSFontWeightRegular,
    NSImageView, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSMainMenuWindowLevel, NSMenu, NSMenuItem, NSModalResponseOK, NSSavePanel, NSAutoresizingMaskOptions, NSScreen, NSScrollView, NSWindow, NSWindowDelegate, NSStackView,
    NSStackViewDistribution, NSTextAlignment, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_event_kit::EKEventStoreChangedNotification;
use objc2_foundation::{NSMutableAttributedString, NSNumber, NSRange, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer};
use objc2_quartz_core::{CAMediaTiming, CATransition, kCATransitionFade};

use crate::calendar::{self, Access, Calendar, Event};
use crate::clip_card::{self, ClipCard};
use crate::clips::ClipHistory;
use crate::settings::{self as prefs, Settings, Size};
use crate::settings_window::SettingsWindow;
use crate::days;
use crate::export;
use crate::focus_ring::FocusRing;
use crate::insights::Insights;
use crate::insights_view::InsightsView;
use crate::linear::{self, Linear};
use crate::list_row::{self, ListRow};
use crate::live_activity::{Activity, LiveActivity};
use crate::notch_banner::{NotchBanner, Notice};
use crate::now_playing::{Command, NowPlaying};
use crate::notch_motion::NotchMotion;
use crate::quick_add;
use crate::terminal_view::{self, TerminalView};
use crate::chime::Chime;
use crate::pomodoros::{self, DAILY_GOAL};
use crate::tomato_view::{Tomato, TomatoView};
use crate::panel::{self, FileDrag, NotchPanel, NotchShape};
use crate::store::{self, Task};
use crate::task_alerts;
use crate::widgets::{self as w, SURFACE, rgba};

const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 330.0;
/// The window extends this far above the screen. The pointer pressed against the top
/// edge sits at exactly the screen's max Y, which would otherwise count as outside the
/// window and make the panel close and reopen in a loop.
/// The expanded panel keeps this much of the screen's visible height free.
const SCREEN_MARGIN: f64 = 40.0;
pub const TOP_SLOP: f64 = 1.0;
/// MacBook Pro 14" (M4 Pro) notch, used on displays without one.
const DEFAULT_NOTCH: (f64, f64) = (185.0, 32.0);
const FOCUS_CARD_WIDTH: f64 = 236.0;
const MUSIC_PROGRESS_WIDTH: f64 = 340.0;
const ICON_TAB_WIDTH: f64 = 32.0;
const ARTWORK_SIZE: f64 = 120.0;
const MAX_CARD_ROWS: usize = 5;
/// Calendar events shown above the tasks; the rest of today's stay in the calendar app.
const MAX_EVENT_ROWS: usize = 3;
/// Gap between Today rows; a hairline is drawn in its middle.
const CARD_ROW_GAP: f64 = 15.0;
const DURATIONS_MIN: [u64; 4] = prefs::FOCUS_CHOICES;
/// One step past the presets in `duration_idx`: count up with no deadline.
const STOPWATCH: usize = DURATIONS_MIN.len();
const EXTRA_SECS: u64 = 5 * 60;
const PLUS_WIDTH: f64 = 40.0;
/// A long break follows every this many focus sessions.
const CYCLE: usize = 4;
/// How long the focus card's task line shows what just happened to the session.
/// How long a deleted task can be brought back with Undo.
const UNDO_SECS: f64 = 6.0;
/// How long a task reminder card stays without an answer; the system notification remains.
const REMINDER_SECS: f64 = 60.0;
/// How long a pomodoro card stays: it asks nothing.
const SESSION_NOTICE_SECS: f64 = 8.0;
const SNOOZE_MIN: u32 = 5;
/// A reminder whose minute passed longer ago than this (the Mac slept through it) is dropped;
/// the system notification at the task's time still covers it.
const REMIND_GRACE_MIN: u32 = 5;
const TOMATO_SIZE: f64 = 22.0;
const CORAL: (u8, u8, u8) = (217, 112, 90);
/// Cards and the controls on them, in the near-black, faintly cool greys of Linear's dark theme.
const SURFACE_RAISED: (u8, u8, u8) = (40, 41, 45);

fn tone((r, g, b): (u8, u8, u8)) -> Retained<NSColor> {
    rgba(r, g, b, 1.0)
}

fn white(alpha: f64) -> Retained<NSColor> {
    rgba(255, 255, 255, alpha)
}

struct FocusCard {
    ring: Retained<FocusRing>,
    round: Retained<NSTextField>,
    daily_focus: Retained<NSTextField>,
    count: Retained<NSTextField>,
    tomatoes: Vec<Retained<TomatoView>>,
    task: Retained<NSTextField>,
    badge: Retained<NSTextField>,
    time: Retained<NSTextField>,
    ends: Retained<NSTextField>,
    pause: Retained<NSButton>,
    end: Retained<NSButton>,
    plus: Retained<NSButton>,
}

struct TaskCard {
    todo: Retained<NSButton>,
    done: Retained<NSButton>,
    /// Clears today's completed tasks; shown only on the Completed list when it has any.
    clear: Retained<NSButton>,
    /// The reminder or the "Deleted" line beside the list pills.
    note: Retained<NSTextField>,
    undo: Retained<NSButton>,
    /// Completions for the word being typed, as pills on a strip over the bottom of the list.
    hint_bar: Retained<NSView>,
    hints: Retained<NSStackView>,
    /// One dot per entry of `PRIORITIES`, picking the priority of tasks added here.
    dots: Vec<Retained<NSButton>>,
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

struct TerminalPane {
    body: Retained<NSView>,
    view: Retained<TerminalView>,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Workspace,
    Clips,
    Music,
    Terminal,
    /// Only in the dashboard window; not a notch section, so it has no setting.
    Insights,
}

impl Tab {
    /// The notch sections in header order, like [`crate::settings::SECTIONS`].
    const ALL: [Tab; 4] = [Tab::Workspace, Tab::Clips, Tab::Music, Tab::Terminal];
}

/// The header's views: close, the tab groups left and right of the notch, and each tab.
struct Header {
    quit: Retained<NSButton>,
    left: Retained<NSStackView>,
    right: Retained<NSStackView>,
    workspace: Retained<NSButton>,
    insights: Retained<NSButton>,
    clips: Retained<NSButton>,
    music: Retained<NSButton>,
    terminal: Retained<NSButton>,
}

struct Ui {
    panel: Retained<NotchPanel>,
    content: Retained<NSView>,
    workspace: Retained<NSStackView>,
    tab_workspace: Retained<NSButton>,
    tab_insights: Retained<NSButton>,
    tab_clips: Retained<NSButton>,
    tab_music: Retained<NSButton>,
    tab_terminal: Retained<NSButton>,
    clips: ClipsView,
    music: MusicView,
    terminal: TerminalPane,
    insights: Retained<InsightsView>,
    focus: FocusCard,
    tasks: TaskCard,
    expanded: Cell<NSRect>,
    /// Height of the header band beside the notch; follows the display's safe area.
    header_height: Retained<NSLayoutConstraint>,
    /// Keep the tab groups clear of the hardware notch: half its width plus a margin.
    notch_gaps: [Retained<NSLayoutConstraint>; 2],
    motion: Retained<NotchMotion>,
    activity: LiveActivity,
    banner: NotchBanner,
}

#[derive(Default)]
pub struct State {
    ui: OnceCell<Ui>,
    tasks: RefCell<Vec<Task>>,
    /// Today's calendar events (empty unless the setting is on and macOS allowed access), and the
    /// day they were read for, so the list rolls over at midnight.
    events: RefCell<Vec<Event>>,
    events_day: RefCell<String>,
    calendar: RefCell<Calendar>,
    linear: RefCell<Linear>,
    expanded: Cell<bool>,
    /// Opened by hotkey: stays open until the pointer has visited and left.
    pinned: Cell<bool>,
    /// Set when a Space change folded the panel: a pointer resting over the notch must not
    /// reopen it until it has left and come back.
    space_collapsed: Cell<bool>,
    tab: Cell<Tab>,
    clips: RefCell<ClipHistory>,
    now_playing: RefCell<NowPlaying>,
    /// Wing width the panel was last told to grow by; zero when nothing is live.
    wings: Cell<f64>,
    /// Runs only while the Music tab is visible and a track is playing.
    music_timer: RefCell<Option<Retained<NSTimer>>>,
    show_completed: Cell<bool>,
    /// Priority given to tasks added from the Today card unless the text names one.
    new_priority: Cell<usize>,
    /// Task index chosen with ▶; falls back to the first open task of today.
    focus: Cell<Option<usize>>,
    duration_idx: Cell<usize>,
    /// `Instant` is `CLOCK_UPTIME_RAW` on macOS (checked in std's `sys/time/unix.rs`), which stops
    /// while the Mac sleeps: a countdown therefore pauses across sleep instead of expiring in it.
    deadline: Cell<Option<Instant>>,
    /// Stopwatch only: when the running count started, shifted back by any time already counted.
    since: Cell<Option<Instant>>,
    /// Seconds added with +5 to the current session, so the log records the real length.
    extra_secs: Cell<u64>,
    /// Time left on a paused session (elapsed, for the stopwatch); the ticker is off while this is set.
    paused: Cell<Option<Duration>>,
    on_break: Cell<bool>,
    break_min: Cell<u64>,
    /// The chime that is ringing; held because a released `NSSound` stops mid-note.
    chime: RefCell<Option<Retained<NSSound>>>,
    /// What just happened to the session, shown on the focus card's task line until the panel closes.
    toast: RefCell<Option<String>>,
    /// The task the reminder card asks about, for its Snooze and Done buttons.
    reminding: RefCell<Option<task_alerts::Reminder>>,
    /// Snoozed reminders and the minute of the day they come back.
    snoozed: RefCell<Vec<(task_alerts::Reminder, u32)>>,
    /// When the card showing now goes away by itself.
    banner_until: Cell<Option<Instant>>,
    /// `YYYY-MM-DD HH:MM` of the last reminder check; empty until the first one.
    reminded_at: RefCell<String>,
    /// What the hint strip offers for the word being typed, and which one Tab or Return takes.
    hints: RefCell<Vec<String>>,
    hint: Cell<usize>,
    /// The last deleted task and where it stood, while Undo can still restore it.
    undo: RefCell<Option<(usize, Task, Instant)>>,
    /// Sessions finished today, read from `pomodoros.txt` once per refresh.
    done_today: Cell<usize>,
    focus_today: Cell<u32>,
    ticker: RefCell<Option<Retained<NSTimer>>>,
    /// Modification time of `tasks.txt` as last loaded or saved by this process.
    seen_mtime: Cell<Option<std::time::SystemTime>>,
    /// Made on first use. While it is open it hosts the panel's content view, and hover
    /// must not pull the notch open over an empty frame.
    dashboard: OnceCell<Retained<NSWindow>>,
    dashboard_open: Cell<bool>,
    settings: RefCell<Settings>,
    /// Made on first use and reused.
    settings_window: OnceCell<Retained<SettingsWindow>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSControlTextEditingDelegate for Controller {
        /// Only the task input has this delegate.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _notification: &NSNotification) {
            self.ivars().hint.set(0);
            self.render_hints();
        }

        /// While hints show, ↑/↓ move between them, Tab or Return takes one and Esc hides them;
        /// otherwise the field handles the key (Return adds the task).
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(&self, _control: &NSControl, _editor: &NSTextView, command: Sel) -> Bool {
            Bool::new(self.hint_command(command))
        }
    }

    unsafe impl NSTextFieldDelegate for Controller {}

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            self.hide_dashboard();
        }

        /// Typing in the terminal holds the panel open even after the pointer leaves.
        /// Clicking another window ends that, so fold it unless the pointer is still over it.
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, notification: &NSNotification) {
            let panel: &AnyObject = self.ui().panel.as_ref();
            if notification.object().is_some_and(|window| std::ptr::eq(&*window as *const AnyObject, panel)) {
                self.collapse_when_left();
            }
        }
    }

    impl Controller {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.cancel_recheck();
            if self.ivars().space_collapsed.get() {
                return;
            }
            self.ivars().pinned.set(false);
            self.set_expanded(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().space_collapsed.set(false);
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
                self.push_task(text, &days::key(0), PRIORITIES[self.ivars().new_priority.get()]);
            }
            field.setStringValue(&NSString::new());
        }

        #[unsafe(method(toggleTask:))]
        fn toggle_task(&self, sender: &NSButton) {
            if let Some(task) = self.ivars().tasks.borrow_mut().get_mut(sender.tag() as usize) {
                task.set_done(!task.done);
                self.complete_remote(task);
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
                let task = tasks.remove(index);
                *self.ivars().undo.borrow_mut() = Some((index, task, Instant::now()));
            }
            unsafe { NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(UNDO_SECS, self.target(), sel!(undoExpired:), None, false) };
            // The focus card refers to tasks by index, so follow the shift.
            let focus = self.ivars().focus.get().and_then(|f| match f {
                f if f == index => None,
                f if f > index => Some(f - 1),
                f => Some(f),
            });
            self.ivars().focus.set(focus);
            self.save_and_refresh();
        }

        #[unsafe(method(undoDelete:))]
        fn undo_delete(&self, _sender: &AnyObject) {
            let Some((index, task, _)) = self.ivars().undo.take() else { return };
            let index = {
                let mut tasks = self.ivars().tasks.borrow_mut();
                let index = index.min(tasks.len());
                tasks.insert(index, task);
                index
            };
            let focus = self.ivars().focus.get().map(|f| if f >= index { f + 1 } else { f });
            self.ivars().focus.set(focus);
            self.save_and_refresh();
        }

        /// A later delete restarts the clock, so only an Undo that has really run out is dropped.
        #[unsafe(method(undoExpired:))]
        fn undo_expired(&self, _timer: &NSTimer) {
            let stale = self.ivars().undo.borrow().as_ref().is_some_and(|(_, _, at)| at.elapsed().as_secs_f64() >= UNDO_SECS - 0.1);
            if stale {
                self.ivars().undo.take();
                self.render_task_card();
            }
        }

        #[unsafe(method(snoozeReminder:))]
        fn snooze_reminder(&self, _sender: &NSButton) {
            if let (Some(reminder), Some(now)) = (self.ivars().reminding.take(), now_minutes()) {
                self.ivars().snoozed.borrow_mut().push((reminder, now + SNOOZE_MIN));
            }
            self.hide_banner();
        }

        #[unsafe(method(finishReminder:))]
        fn finish_reminder(&self, _sender: &NSButton) {
            if let Some(reminder) = self.ivars().reminding.take() {
                let today = days::key(0);
                let mut tasks = self.ivars().tasks.borrow_mut();
                let task = tasks.iter_mut().find(|t| !t.done && t.day == today && t.text == reminder.text && t.time == reminder.time);
                if let Some(task) = task {
                    task.set_done(true);
                    self.complete_remote(task);
                }
            }
            self.save_and_refresh();
            self.hide_banner();
        }

        #[unsafe(method(dismissBanner:))]
        fn dismiss_banner(&self, _sender: &NSButton) {
            self.hide_banner();
        }

        /// A newer card restarts the clock, so only one that has really run out is hidden.
        #[unsafe(method(bannerExpired:))]
        fn banner_expired(&self, _timer: &NSTimer) {
            if self.ivars().banner_until.get().is_some_and(|until| Instant::now() + Duration::from_millis(100) >= until) {
                self.hide_banner();
            }
        }

        #[unsafe(method(copyMarkdown:))]
        fn copy_markdown(&self, _item: &NSMenuItem) {
            let tasks = self.ivars().tasks.borrow();
            let (todo, done) = today_lists(&tasks, &days::key(0));
            let text = checklist(todo.iter().chain(&done).map(|&i| &tasks[i]));
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            pasteboard.setString_forType(&NSString::from_str(&text), unsafe { NSPasteboardTypeString });
        }

        #[unsafe(method(setPriority:))]
        fn set_priority(&self, item: &NSMenuItem) {
            let level = match item.title().to_string().as_str() {
                "None" => String::new(),
                title => title.to_lowercase(),
            };
            if let Some(task) = self.ivars().tasks.borrow_mut().get_mut(item.tag() as usize) {
                task.priority = level;
            }
            self.save_and_refresh();
        }

        #[unsafe(method(pickPriority:))]
        fn pick_priority(&self, sender: &NSButton) {
            self.ivars().new_priority.set(sender.tag() as usize);
            self.render_priority_dots();
        }

        #[unsafe(method(clearCompleted:))]
        fn clear_completed(&self, _sender: &AnyObject) {
            let today = days::key(0);
            for task in self.ivars().tasks.borrow_mut().iter_mut().filter(|t| t.done && t.day == today) {
                // Off every list, yet `done_on` still counts it in Insights.
                task.day.clear();
            }
            self.save_and_refresh();
        }

        #[unsafe(method(moveToTomorrow:))]
        fn move_to_tomorrow(&self, item: &NSMenuItem) {
            self.reschedule(item.tag() as usize, days::key(1));
        }

        #[unsafe(method(pickHint:))]
        fn pick_hint(&self, sender: &NSButton) {
            self.take_hint(sender.tag() as usize);
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

        #[unsafe(method(showInsights:))]
        fn show_insights(&self, _sender: &AnyObject) {
            self.ivars().tab.set(Tab::Insights);
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
            self.sync_activity();
        }

        /// A Linear fetch finished: new assigned issues become tasks.
        #[unsafe(method(linearSynced:))]
        fn linear_synced(&self, _arg: Option<&AnyObject>) {
            let Some(result) = self.ivars().linear.borrow_mut().take() else { return };
            match result {
                Ok(issues) => {
                    let added = linear::import(&mut self.ivars().tasks.borrow_mut(), &issues, &days::key(0));
                    if added > 0 {
                        self.save_and_refresh();
                    }
                }
                Err(err) => eprintln!("notchpad: Linear sync failed: {err}"),
            }
        }

        /// Called when the user answers the calendar prompt and whenever the calendars change.
        #[unsafe(method(reloadEvents:))]
        fn reload_events(&self, _arg: Option<&AnyObject>) {
            self.load_events();
            self.render_task_card();
        }

        #[unsafe(method(tickMusic:))]
        fn tick_music(&self, _timer: &NSTimer) {
            self.render_music();
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
        /// after the responder change, when `terminal_has_focus` has settled.
        #[unsafe(method(terminalResigned:))]
        fn terminal_resigned(&self, _notification: &NSNotification) {
            let none: Option<&AnyObject> = None;
            let _: () = unsafe { msg_send![self, performSelector: sel!(recheckHover:), withObject: none, afterDelay: 0.0f64] };
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
            if self.running() {
                let held = match (state.deadline.get(), state.since.get()) {
                    (Some(deadline), _) => deadline.saturating_duration_since(Instant::now()),
                    (None, since) => since.map_or(Duration::ZERO, |s| s.elapsed()),
                };
                let extra = state.extra_secs.get();
                self.stop_timer();
                state.paused.set(Some(held));
                state.extra_secs.set(extra);
                self.render_focus();
            } else if self.stopwatch() {
                self.run_stopwatch(state.paused.take().unwrap_or_default());
            } else {
                let secs = state.paused.take().map_or_else(|| self.session_secs(), |left| left.as_secs() + 1);
                self.run_timer(secs);
            }
        }

        /// +5: lengthens the running or paused countdown; the stopwatch has no length.
        #[unsafe(method(addFive:))]
        fn add_five(&self, _sender: &AnyObject) {
            let state = self.ivars();
            let extra = Duration::from_secs(EXTRA_SECS);
            if let Some(deadline) = state.deadline.get() {
                state.deadline.set(Some(deadline + extra));
            } else if let (Some(left), false) = (state.paused.get(), self.stopwatch()) {
                state.paused.set(Some(left + extra));
            } else {
                return;
            }
            state.extra_secs.set(state.extra_secs.get() + EXTRA_SECS);
            self.render_focus();
        }

        /// End: a focus stopped early is logged; a break is simply skipped.
        #[unsafe(method(endTimer:))]
        fn end_timer(&self, _sender: &AnyObject) {
            let state = self.ivars();
            if !state.on_break.get() && self.session_active() {
                let elapsed = if self.stopwatch() {
                    self.shown_secs()
                } else {
                    self.session_secs() - self.remaining_secs().min(self.session_secs())
                };
                if let Some(minutes) = end_log_minutes(self.stopwatch(), elapsed) {
                    pomodoros::append(&days::stamp(), minutes, self.stopwatch());
                    self.reload_done_today();
                }
            }
            state.on_break.set(false);
            self.stop_timer();
        }

        #[unsafe(method(cycleDuration:))]
        fn cycle_duration(&self, _sender: &AnyObject) {
            if self.session_active() {
                return;
            }
            let next = next_duration(self.ivars().duration_idx.get());
            self.ivars().duration_idx.set(next);
            self.render_focus();
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            let Some(deadline) = self.ivars().deadline.get() else {
                self.render_focus(); // stopwatch
                return;
            };
            if Instant::now() >= deadline {
                let state = self.ivars();
                let was_break = state.on_break.replace(false);
                let planned_min = self.session_secs() / 60; // before stop_timer clears the +5 extras
                self.stop_timer();
                let (chime, symbol, title, subtitle) = if was_break {
                    (Chime::BreakEnd, "timer", "Break over".to_owned(), format!("Ready for pomodoro {}", self.done_today() + 1))
                } else {
                    pomodoros::append(&days::stamp(), planned_min, true);
                    self.reload_done_today();
                    let done = self.done_today();
                    let long = done % CYCLE == 0;
                    let settings = state.settings.borrow().clone();
                    let break_min = if long { settings.long_break } else { settings.short_break };
                    state.on_break.set(true);
                    state.break_min.set(break_min);
                    self.run_timer(self.session_secs());
                    let kind = if long { "Long" } else { "Short" };
                    (Chime::FocusEnd, "cup.and.saucer.fill", format!("Pomodoro {done} done"), format!("{kind} break · {break_min} min"))
                };
                *self.ivars().toast.borrow_mut() = Some(format!("{title} · {subtitle}"));
                self.refresh();
                let notice = Notice { symbol, tint: &tone(CORAL), title: &title, subtitle: &subtitle, actions: false };
                self.notify(chime, &notice, SESSION_NOTICE_SECS);
            } else {
                self.render_focus();
            }
        }

        /// Picks up edits made to `tasks.txt` by other processes (e.g. the MCP server).
        #[unsafe(method(reload:))]
        fn reload(&self, _timer: &NSTimer) {
            self.remind_tasks();
            if self.ivars().settings.borrow().calendar && *self.ivars().events_day.borrow() != days::key(0) {
                self.load_events();
                self.render_task_card();
            }
            if self.ivars().settings.borrow().linear && self.ivars().linear.borrow().due() {
                self.ivars().linear.borrow_mut().fetch(self.target(), sel!(linearSynced:));
            }
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
            let (collapsed, expanded, safe_top) = notch_rects(self.mtm(), self.tab_size());
            let ui = self.ui();
            ui.header_height.setConstant(header_band(safe_top));
            for constraint in &ui.notch_gaps {
                constraint.setConstant(notch_gap(collapsed, safe_top));
            }
            self.place(collapsed, expanded);
            self.collapse_when_left();
        }

        /// The screen slides under the stationary panel during a Space switch, so fold it
        /// into the notch instead; `set_expanded` drops the terminal's focus.
        #[unsafe(method(activeSpaceChanged:))]
        fn active_space_changed(&self, _notification: &NSNotification) {
            if !self.ivars().expanded.get() {
                return;
            }
            self.cancel_recheck();
            self.ivars().pinned.set(false);
            self.ivars().space_collapsed.set(true);
            self.set_expanded(false);
        }

        #[unsafe(method(openInsights:))]
        fn open_insights(&self, _sender: &AnyObject) {
            self.show_insights_dashboard();
        }

        #[unsafe(method(openDashboard:))]
        fn open_dashboard(&self, _sender: &AnyObject) {
            self.show_dashboard();
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            self.show_settings();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            // In the dashboard window the same ✕ closes the window, returning the view to the notch.
            if self.ivars().dashboard_open.get() {
                if let Some(window) = self.ivars().dashboard.get() {
                    window.performClose(None);
                }
                return;
            }
            self.ivars().now_playing.borrow_mut().stop();
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        #[unsafe(method(exportBackup:))]
        fn export_backup(&self, _sender: &AnyObject) {
            // The panel is non-activating; without this the save sheet opens behind other windows.
            #[allow(deprecated)] // `activate()` needs macOS 14
            NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
            let panel = NSSavePanel::savePanel(self.mtm());
            panel.setNameFieldStringValue(&NSString::from_str(&format!("notchpad-backup-{}.json", days::key(0))));
            if panel.runModal() != NSModalResponseOK {
                return;
            }
            let Some(path) = panel.URL().and_then(|url| url.path()) else { return };
            if let Err(err) = export::write(&path.to_string()) {
                eprintln!("notchpad: could not export backup: {err}");
            }
        }
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let state = State {
            tasks: RefCell::new(store::load()),
            settings: RefCell::new(Settings::load()),
            duration_idx: Cell::new(focus_index(&Settings::load())),
            new_priority: Cell::new(PRIORITIES.len() - 1), // no priority
            ..State::default()
        };
        let this = mtm.alloc::<Self>().set_ivars(state);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let ui = this.build_ui(mtm);
        this.ivars().ui.set(ui).ok().expect("ui is built once");
        this.ivars().seen_mtime.set(store::modified());
        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            let terminal: &AnyObject = this.ui().terminal.view.as_ref();
            center.addObserver_selector_name_object(
                this.target(), sel!(terminalResigned:), Some(&NSString::from_str(terminal_view::RESIGNED_NOTIFICATION)), Some(terminal),
            );
            center.addObserver_selector_name_object(this.target(), sel!(reloadEvents:), Some(EKEventStoreChangedNotification), None);
            // The controller lives for the process. Re-anchor after hot-plug, resolution,
            // primary-display or arrangement changes instead of keeping startup coordinates.
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                this.target(), sel!(screenParametersChanged:),
                Some(NSApplicationDidChangeScreenParametersNotification), None,
            );
            NSWorkspace::sharedWorkspace().notificationCenter().addObserver_selector_name_object(
                this.target(), sel!(activeSpaceChanged:), Some(NSWorkspaceActiveSpaceDidChangeNotification), None,
            );
            // Both live for the whole process. The tolerance lets macOS fire them together
            // with other wakeups instead of waking the CPU on their own.
            let reload = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(2.0, this.target(), sel!(reload:), None, true);
            reload.setTolerance(1.0);
            // Reading the pasteboard's change count is cheap, so copies show up within half a second.
            let clips = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(0.5, this.target(), sel!(pollClips:), None, true);
            clips.setTolerance(0.25);
        }
        this.sync_now_playing();
        this.load_events();
        this.refresh();
        this.render_clips();
        this.ui().panel.orderFrontRegardless();
        this
    }

    /// A task just done in Notchpad that mirrors a Linear issue is completed there too.
    fn complete_remote(&self, task: &Task) {
        if task.done && self.ivars().settings.borrow().linear {
            Linear::complete(&task.ext);
        }
    }

    /// Reads today's events; empty unless the setting is on and macOS has allowed access.
    fn load_events(&self) {
        let events = if self.ivars().settings.borrow().calendar { self.ivars().calendar.borrow_mut().today() } else { Vec::new() };
        *self.ivars().events.borrow_mut() = events;
        *self.ivars().events_day.borrow_mut() = days::key(0);
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

    /// Panel size of the section being shown.
    fn tab_size(&self) -> Size {
        self.ivars().settings.borrow().sizes.get(self.ivars().tab.get() as usize).copied().unwrap_or_default()
    }

    /// Springs the panel to the current section's height; the content follows at once.
    fn fit_panel(&self) {
        let (collapsed, expanded, _) = notch_rects(self.mtm(), self.tab_size());
        if expanded != self.ui().expanded.get() {
            self.place(collapsed, expanded);
        }
    }

    fn place(&self, collapsed: NSRect, expanded: NSRect) {
        let ui = self.ui();
        ui.expanded.set(expanded);
        // The motion grows or shrinks the content with the panel; in the dashboard window
        // the content follows that window instead.
        ui.motion.set_rects(collapsed, expanded);
    }

    fn ui(&self) -> &Ui {
        self.ivars().ui.get().expect("ui is built in new()")
    }

    fn target(&self) -> &AnyObject {
        self.as_ref()
    }

    fn collapse_when_left(&self) {
        if !self.ivars().expanded.get() || self.ivars().pinned.get() || self.terminal_has_focus() {
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
        if on && self.ivars().dashboard_open.get() {
            return;
        }
        if self.ivars().expanded.replace(on) != on {
            self.ui().motion.set_open(on);
            self.sync_music_timer();
            self.sync_activity();
            if !on && self.terminal_has_focus() {
                self.ui().panel.makeFirstResponder(None);
            }
            // An alert has been seen once the panel closes after it.
            if !on && self.ivars().toast.take().is_some() {
                self.render_focus();
            }
        }
    }

    /// The terminal keeps the panel up while typing, even with the pointer elsewhere.
    fn terminal_has_focus(&self) -> bool {
        let ui = self.ui();
        let same = |a: &AnyObject, b: &AnyObject| std::ptr::eq(a as *const _ as *const u8, b as *const _ as *const u8);
        let Some(responder) = ui.panel.firstResponder() else { return false };
        ui.panel.isKeyWindow() && same(&responder, ui.terminal.view.as_ref())
    }

    /// Adds a typed line as a task; words like `6pm`, `!high` or `tomorrow` fill its fields,
    /// and `priority` applies when the line names none.
    fn push_task(&self, text: &str, default_day: &str, priority: &str) {
        let mut task = quick_add::parse(text, default_day, &days::key(0), &days::key(1));
        if task.priority.is_empty() {
            task.priority = priority.to_owned();
        }
        self.ivars().tasks.borrow_mut().push(task);
        self.save_and_refresh();
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
        task_alerts::sync(&self.ivars().tasks.borrow());
    }

    fn reload_done_today(&self) {
        let data = Insights::build(&pomodoros::load_log(), &[], &days::key(0));
        let today = data.day(data.today());
        self.ivars().done_today.set(today.pomodoros as usize);
        self.ivars().focus_today.set(today.focus_min);
    }

    /// Focus sessions completed today.
    fn done_today(&self) -> usize {
        self.ivars().done_today.get()
    }

    fn refresh(&self) {
        // Re-read here (not per tick) so the count also rolls over at midnight.
        self.reload_done_today();
        self.render_tabs();
        self.render_focus();
        self.render_task_card();
        self.render_music();
        self.render_insights();
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
        if state.on_break.get() {
            state.break_min.get() * 60 + state.extra_secs.get()
        } else {
            DURATIONS_MIN.get(state.duration_idx.get()).map_or(0, |m| m * 60) + state.extra_secs.get()
        }
    }

    fn stopwatch(&self) -> bool {
        self.ivars().duration_idx.get() == STOPWATCH
    }

    fn running(&self) -> bool {
        self.ivars().deadline.get().is_some() || self.ivars().since.get().is_some()
    }

    fn session_active(&self) -> bool {
        self.running() || self.ivars().paused.get().is_some()
    }

    /// What the big clock and the notch wing show: time left, or time counted for the stopwatch.
    fn shown_secs(&self) -> u64 {
        let state = self.ivars();
        if !self.stopwatch() {
            return self.remaining_secs();
        }
        match (state.since.get(), state.paused.get()) {
            (Some(since), _) => since.elapsed().as_secs(),
            (None, paused) => paused.map_or(0, |p| p.as_secs()),
        }
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
        self.start_ticker();
    }

    /// Counts up from `elapsed`; there is no deadline.
    fn run_stopwatch(&self, elapsed: Duration) {
        let now = Instant::now();
        self.ivars().since.set(Some(now.checked_sub(elapsed).unwrap_or(now)));
        self.start_ticker();
    }

    fn start_ticker(&self) {
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(1.0, self.target(), sel!(tick:), None, true)
        };
        *self.ivars().ticker.borrow_mut() = Some(timer);
        self.render_focus();
    }

    /// Drops the deadline, any pause and the ticker, so nothing runs while idle.
    fn stop_timer(&self) {
        self.ivars().deadline.set(None);
        self.ivars().since.set(None);
        self.ivars().extra_secs.set(0);
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
        let settings = self.ivars().settings.borrow();
        let hidden = |tab: Tab| match tab {
            Tab::Insights => !self.ivars().dashboard_open.get(),
            tab => settings.hidden[tab as usize],
        };
        if hidden(self.ivars().tab.get()) {
            self.ivars().tab.set(Tab::ALL[settings.first_visible()]);
        }
        let current = self.ivars().tab.get();
        let tabs: [(&NSView, &NSButton, Tab); 5] = [
            (&ui.workspace, &ui.tab_workspace, Tab::Workspace),
            (ui.insights.body(), &ui.tab_insights, Tab::Insights),
            (&ui.clips.body, &ui.tab_clips, Tab::Clips),
            (&ui.music.body, &ui.tab_music, Tab::Music),
            (&ui.terminal.body, &ui.tab_terminal, Tab::Terminal),
        ];
        if !ui.content.isHidden() && tabs.iter().any(|(body, _, tab)| body.isHidden() == (*tab == current)) {
            // A crossfade snapshots the old picture at its old size. If the panel is about to
            // change height that snapshot sits stranded at the bottom, so only the new body
            // fades in.
            let resizing = notch_rects(self.mtm(), self.tab_size()).1 != ui.expanded.get();
            match tabs.iter().find(|(_, _, tab)| *tab == current) {
                Some((body, _, _)) if resizing => crossfade(body),
                _ => crossfade(&ui.content),
            }
        }
        for (body, pill, tab) in tabs {
            body.setHidden(tab != current);
            pill.setHidden(hidden(tab));
            if tab == current {
                w::style_pill(pill, &white(1.0), &NSColor::blackColor(), 12.0);
            } else {
                w::style_pill(pill, &white(0.1), &white(0.8), 12.0);
            }
        }
        self.fit_panel();
    }

    /// Rebuilds the Insights page while it is the visible tab; no other tab pays for it.
    fn render_insights(&self) {
        if self.ivars().tab.get() == Tab::Insights {
            let data = Insights::build(&pomodoros::load_log(), &self.ivars().tasks.borrow(), &days::key(0));
            self.ui().insights.show(data);
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
                self.ivars().space_collapsed.set(false);
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

    /// Rings `chime` (unless sounds are off) and hangs `notice` from the notch for `seconds`.
    /// The card never takes the keyboard, so it is safe while another app is being typed in.
    fn notify(&self, chime: Chime, notice: &Notice, seconds: f64) {
        if self.ivars().settings.borrow().sound {
            let sound = chime.sound();
            if let Some(sound) = &sound {
                sound.play();
            }
            *self.ivars().chime.borrow_mut() = sound;
        }
        let (collapsed, _, _) = notch_rects(self.mtm(), self.tab_size());
        self.ui().banner.show(collapsed, notice);
        self.ivars().banner_until.set(Some(Instant::now() + Duration::from_secs_f64(seconds)));
        unsafe { NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(seconds, self.target(), sel!(bannerExpired:), None, false) };
    }

    fn hide_banner(&self) {
        self.ivars().banner_until.set(None);
        self.ivars().reminding.take();
        self.ui().banner.hide();
    }

    /// Shows a card for tasks whose reminder minute passed since the last check, and for snoozed
    /// ones coming back. Called on the 2 s reload tick; it does work only when the minute changes.
    fn remind_tasks(&self) {
        let stamp = days::stamp();
        let last = self.ivars().reminded_at.replace(stamp.clone());
        let Some(lead) = self.ivars().settings.borrow().remind else { return };
        if last.is_empty() || last == stamp {
            return;
        }
        let (today, clock) = stamp.split_once(' ').unwrap_or_default();
        let Some(now) = task_alerts::minutes(clock) else { return };
        task_alerts::withdraw_next_minute(&self.ivars().tasks.borrow(), today, now);
        let earliest = now.saturating_sub(REMIND_GRACE_MIN);
        let after = match last.split_once(' ') {
            Some((day, clock)) if day == today => task_alerts::minutes(clock).unwrap_or(now).max(earliest),
            _ => earliest,
        };
        let mut due = task_alerts::reminders_between(&self.ivars().tasks.borrow(), today, lead, after, now);
        self.ivars().snoozed.borrow_mut().retain(|(reminder, back)| {
            let back_now = after < *back && *back <= now;
            if back_now {
                due.push(reminder.clone());
            }
            !back_now
        });
        let Some(first) = due.first().cloned() else { return };
        let when = if first.at > now { format!("in {} min", first.at - now) } else { "now".to_owned() };
        let mut subtitle = format!("{} · {when}", clock_12h(&first.time).unwrap_or_default());
        if due.len() > 1 {
            subtitle.push_str(&format!(" · +{} more", due.len() - 1));
        }
        let notice = Notice { symbol: "bell.fill", tint: &rgba(255, 176, 32, 1.0), title: &first.text, subtitle: &subtitle, actions: true };
        self.notify(Chime::Reminder, &notice, REMINDER_SECS);
        *self.ivars().reminding.borrow_mut() = Some(first.clone());
    }

    fn render_focus(&self) {
        let card = &self.ui().focus;
        let state = self.ivars();
        let set = |field: &NSTextField, text: &str| field.setStringValue(&NSString::from_str(text));
        let running = self.running();
        let active = self.session_active();
        let stopwatch = self.stopwatch();
        let remaining = self.shown_secs();
        let done = self.done_today();
        // The stopwatch has no end to ripen towards, so its tomato stays green.
        let total = self.session_secs();
        let ripeness = if stopwatch || total == 0 { 0.0 } else { 1.0 - remaining as f64 / total as f64 };

        card.ring.set(ripeness, state.on_break.get());
        set(&card.count, &format!("{done} / {DAILY_GOAL} today"));
        set(&card.daily_focus, &format!("{} min focus", state.focus_today.get()));
        let cycle_done = completed_in_cycle(done, state.on_break.get());
        set(&card.round, &format!("{} / {CYCLE}", (cycle_done + usize::from(!state.on_break.get())).min(CYCLE)));
        for (tomato, state) in card.tomatoes.iter().zip(cycle_tomatoes(done, state.on_break.get(), active, ripeness)) {
            tomato.set(state);
        }
        let toast = state.toast.borrow().clone();
        if let Some(toast) = toast {
            set(&card.task, &toast);
            card.task.setTextColor(Some(&tone(CORAL)));
        } else {
            let task = self.focused_task().map(|i| state.tasks.borrow()[i].text.clone());
            set(&card.task, task.as_deref().unwrap_or("Pick a task with ▶"));
            card.task.setTextColor(Some(&white(0.55)));
        }
        set(&card.badge, &if stopwatch {
            "Stopwatch".to_owned()
        } else if !state.on_break.get() {
            "Focus".to_owned()
        } else if done % CYCLE == 0 {
            "Break · long".to_owned()
        } else {
            "Break · short".to_owned()
        });
        set(&card.time, &mmss(remaining));
        // Recomputed on every tick, never cached: `Instant` skips sleep, so the end moves later by
        // the time the Mac slept and the label is only right if it is derived again after wake.
        set(&card.ends, &match (running, active) {
            (true, _) if stopwatch => "counting up".to_owned(),
            (true, _) => format!("ends at {}", days::clock_in(remaining)),
            (false, true) => "paused".to_owned(),
            (false, false) if stopwatch => "∞ no limit".to_owned(),
            (false, false) => format!("{} min", remaining / 60),
        });
        card.pause.setTitle(&NSString::from_str(match (running, active) {
            (true, _) => "Pause",
            (false, true) => "Resume",
            (false, false) => "Start",
        }));
        card.end.setEnabled(active);
        card.end.setAlphaValue(if active { 1.0 } else { 0.4 });
        card.plus.setEnabled(active && !stopwatch);
        card.plus.setAlphaValue(if active && !stopwatch { 1.0 } else { 0.4 });
        let hint = (!active).then(|| NSString::from_str("Click to change the length"));
        card.time.setToolTip(hint.as_deref());
        self.sync_activity();
    }

    /// Shows what is live in the collapsed notch's wings and widens the panel to fit.
    fn sync_activity(&self) {
        let state = self.ivars();
        let ui = self.ui();
        let settings = state.settings.borrow();
        let countdown = (settings.wing_focus && self.session_active()).then(|| mmss(self.shown_secs()));
        let activity = Activity {
            music: state.now_playing.borrow().track.as_ref().filter(|t| t.playing && settings.wing_music).map(|t| t.artwork.clone()),
            countdown: countdown.as_deref().map(|text| (text, state.on_break.get())),
        };
        ui.activity.show(&activity, !state.expanded.get());
        let wings = activity.wing();
        if state.wings.replace(wings) != wings {
            ui.motion.set_wings(2.0 * wings);
        }
    }

    fn render_task_card(&self) {
        let card = &self.ui().tasks;
        let today = days::key(0);
        let tasks = self.ivars().tasks.borrow();
        let (todo, done) = today_lists(&tasks, &today);

        let show_completed = self.ivars().show_completed.get();
        card.todo.setTitle(&NSString::from_str(&format!("To do  {}", todo.len())));
        card.done.setTitle(&NSString::from_str(&format!("Completed  {}", done.len())));
        let (on, off) = if show_completed { (&card.done, &card.todo) } else { (&card.todo, &card.done) };
        w::style_pill(on, &white(0.1), &white(0.95), 7.0);
        w::style_pill(off, &white(0.0), &white(0.55), 7.0);
        w::fit_pill(&card.todo, 10.0);
        w::fit_pill(&card.done, 10.0);
        card.clear.setHidden(!show_completed || done.is_empty());
        let undo = self.ivars().undo.borrow().as_ref().map(|(_, task, _)| format!("Deleted “{}”", task.text));
        card.undo.setHidden(undo.is_none());
        card.note.setStringValue(&NSString::from_str(undo.as_deref().unwrap_or_default()));
        self.render_priority_dots();

        let before = list_row::snapshot(&card.rows);
        clear(&card.rows);
        let shown = if show_completed { &done } else { &todo };
        let events: Vec<Event> = if show_completed { Vec::new() } else { self.ivars().events.borrow().iter().take(MAX_EVENT_ROWS).cloned().collect() };
        for (k, event) in events.iter().enumerate() {
            let row = ListRow::new(&event.title, &self.event_row(event), &white(0.07), self.mtm());
            if k > 0 {
                row.add_rule(&white(0.08), CARD_ROW_GAP, self.mtm());
            }
            add_full_width(&card.rows, &row, None);
        }
        let fit = rows_that_fit(&tasks, shown, MAX_CARD_ROWS - events.len());
        for (k, &i) in shown.iter().take(fit).enumerate() {
            let row = ListRow::new(&tasks[i].text, &self.card_row(i, &tasks[i]), &white(0.07), self.mtm());
            unsafe { row.setMenu(Some(&self.task_menu(i, &tasks[i].priority))) };
            if k > 0 || !events.is_empty() {
                row.add_rule(&white(0.08), CARD_ROW_GAP, self.mtm());
            }
            add_full_width(&card.rows, &row, None);
        }
        let empty = if show_completed { "Nothing completed yet today" } else { "All clear. Add a task below." };
        self.add_overflow_note(&card.rows, shown.len(), fit, empty, &white(0.5));
        list_row::animate_changes(&card.rows, before);
    }

    /// Handles a key the task input passes on while hints show; false lets the field handle it.
    fn hint_command(&self, command: Sel) -> bool {
        let count = self.ivars().hints.borrow().len();
        if count == 0 {
            return false;
        }
        let hint = self.ivars().hint.get();
        if command == sel!(moveDown:) {
            self.ivars().hint.set((hint + 1) % count);
        } else if command == sel!(moveUp:) {
            self.ivars().hint.set((hint + count - 1) % count);
        } else if command == sel!(insertTab:) || command == sel!(insertNewline:) {
            self.take_hint(hint);
            return true;
        } else if command == sel!(cancelOperation:) {
            self.ivars().hints.borrow_mut().clear();
        } else {
            return false;
        }
        self.show_hints();
        true
    }

    /// Offers completions for the word at the end of the task input; projects most recent first.
    fn render_hints(&self) {
        let text = self.ui().tasks.input.stringValue().to_string();
        let mut projects: Vec<String> = Vec::new();
        for task in self.ivars().tasks.borrow().iter().rev() {
            if !task.project.is_empty() && !projects.contains(&task.project) {
                projects.push(task.project.clone());
            }
        }
        *self.ivars().hints.borrow_mut() = quick_add::suggestions(&text, &projects);
        self.show_hints();
    }

    fn show_hints(&self) {
        let card = &self.ui().tasks;
        let hints = self.ivars().hints.borrow();
        clear(&card.hints);
        for (i, hint) in hints.iter().enumerate() {
            let pill = w::pill(hint, 22.0, self.target(), sel!(pickHint:), self.mtm());
            pill.setTag(i as isize);
            w::fit_pill(&pill, 10.0);
            let chosen = i == self.ivars().hint.get();
            w::style_pill(&pill, &white(if chosen { 0.18 } else { 0.06 }), &white(if chosen { 0.95 } else { 0.65 }), 7.0);
            card.hints.addArrangedSubview(&pill);
        }
        if !hints.is_empty() {
            card.hints.addArrangedSubview(&w::spacer(self.mtm()));
            card.hints.addArrangedSubview(&w::label("↑↓ · Tab", 11.0, unsafe { NSFontWeightMedium }, &white(0.35), self.mtm()));
        }
        card.hint_bar.setHidden(hints.is_empty());
    }

    /// Puts hint `index` in place of the word being typed and keeps typing after it.
    fn take_hint(&self, index: usize) {
        let Some(choice) = self.ivars().hints.borrow().get(index).cloned() else { return };
        let input = &self.ui().tasks.input;
        let text = quick_add::complete(&input.stringValue().to_string(), &choice);
        input.setStringValue(&NSString::from_str(&text));
        // A clicked pill may have taken the focus; refocusing selects all, so the caret goes last.
        if input.currentEditor().is_none() {
            self.ui().panel.makeFirstResponder(Some(input));
        }
        if let Some(editor) = input.currentEditor() {
            editor.setSelectedRange(NSRange::new(text.encode_utf16().count(), 0));
        }
        self.render_hints();
    }

    /// Rings the dot of the priority new tasks get and dims the others.
    fn render_priority_dots(&self) {
        let chosen = self.ivars().new_priority.get();
        for (i, dot) in self.ui().tasks.dots.iter().enumerate() {
            dot.setImage(Some(&w::symbol(if i == chosen { "record.circle" } else { "circle.fill" })));
            dot.setAlphaValue(if i == chosen { 1.0 } else { 0.4 });
        }
    }

    /// A calendar event: no checkbox, since it cannot be completed here.
    fn event_row(&self, event: &Event) -> Retained<NSView> {
        let mtm = self.mtm();
        let icon = NSImageView::imageViewWithImage(&w::symbol("calendar"), mtm);
        icon.setContentTintColor(Some(&white(0.45)));
        let when = clock_12h(&event.time).unwrap_or_else(|| "All day".to_owned());
        let label = w::label(&format!("{when}  {}", event.title), 13.0, unsafe { NSFontWeightRegular }, &white(0.6), mtm);
        w::stack(&[&icon, &label], false, 10.0, mtm).into_super()
    }

    fn card_row(&self, index: usize, task: &Task) -> Retained<NSView> {
        let mtm = self.mtm();
        let (circle, tint) = if task.done { ("checkmark.circle.fill", white(0.45)) } else { ("circle", priority_color(&task.priority)) };
        let check = w::icon_button(circle, "", 15.0, &tint, self.target(), sel!(toggleTask:), mtm);
        check.setTag(index as isize);
        let alpha = if task.done { 0.45 } else { 0.9 };
        let label = w::label(&task.text, 13.0, unsafe { NSFontWeightRegular }, &white(alpha), mtm);
        if task.done {
            strike(&label);
        }
        let title = with_meta(task, label, &white(0.5), mtm);
        let play = w::icon_button("play.fill", "", 10.0, &white(0.45), self.target(), sel!(focusTask:), mtm);
        play.setTag(index as isize);
        play.setToolTip(Some(&NSString::from_str("Focus on this in the pomodoro")));
        play.setHidden(task.done);
        w::stack(&[&check, &title, &play], false, 10.0, mtm).into_super()
    }

    /// Right-click menu of the header's empty area.
    fn header_menu(&self) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::new());
        menu.setAutoenablesItems(false);
        let items = [
            ("Open Insights", sel!(openInsights:), ""),
            ("Open dashboard window", sel!(openDashboard:), ""),
            ("Export backup (JSON)…", sel!(exportBackup:), ""),
            ("Copy today as Markdown", sel!(copyMarkdown:), ""),
            ("Settings…", sel!(openSettings:), ","),
        ];
        for (title, action, key) in items {
            let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(title), Some(action), &NSString::from_str(key)) };
            unsafe { item.setTarget(Some(self.target())) };
            menu.addItem(&item);
        }
        menu
    }

    fn show_settings(&self) {
        let window = self.ivars().settings_window.get_or_init(|| {
            let me = Weak::new(self);
            let on_change = Box::new(move |settings: &Settings| {
                if let Some(controller) = me.load() {
                    controller.apply_settings(settings.clone());
                }
            });
            let me = Weak::new(self);
            let on_insights = Box::new(move || {
                if let Some(controller) = me.load() {
                    controller.show_insights_dashboard();
                }
            });
            SettingsWindow::new(self.ivars().settings.borrow().clone(), on_change, on_insights, self.mtm())
        });
        window.show();
    }

    /// Live: pills, tab fallback and wings follow from a re-render.
    fn apply_settings(&self, settings: Settings) {
        let focus_changed = self.ivars().settings.borrow().focus != settings.focus;
        // The one place macOS is asked for calendar access: the user just turned the switch on.
        let calendar_turned_on = settings.calendar && !self.ivars().settings.borrow().calendar;
        let linear_changed = settings.linear != self.ivars().settings.borrow().linear;
        if focus_changed && !self.session_active() {
            self.ivars().duration_idx.set(focus_index(&settings));
        }
        *self.ivars().settings.borrow_mut() = settings;
        if linear_changed {
            self.ivars().linear.borrow_mut().reset();
        }
        if calendar_turned_on && calendar::access() == Access::Undecided {
            self.ivars().calendar.borrow_mut().request_access(self.target(), sel!(reloadEvents:));
        }
        self.load_events();
        self.sync_now_playing();
        self.refresh();
    }

    /// The helper blocks on notifications, so it costs nothing while idle. It runs from launch
    /// so the collapsed notch can show music without the Music tab being opened, and stops only
    /// when neither the Music section nor the music wing wants it.
    fn sync_now_playing(&self) {
        let mut now_playing = self.ivars().now_playing.borrow_mut();
        if self.ivars().settings.borrow().needs_now_playing() {
            now_playing.start(self.target(), sel!(nowPlayingChanged:));
        } else {
            now_playing.stop();
        }
    }

    fn show_insights_dashboard(&self) {
        self.show_dashboard();
        self.ivars().tab.set(Tab::Insights);
        self.refresh();
    }

    /// Hosts the notch's content view in a normal window for when the panel feels too tight.
    fn show_dashboard(&self) {
        let mtm = self.mtm();
        let ui = self.ui();
        let window = self.ivars().dashboard.get_or_init(|| {
            let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable | NSWindowStyleMask::Miniaturizable;
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    mtm.alloc(), NSRect::new(NSPoint::ZERO, NSSize::new(900.0, 600.0)), style, NSBackingStoreType::Buffered, false,
                )
            };
            unsafe { window.setReleasedWhenClosed(false) };
            window.setTitle(&NSString::from_str("Notchpad"));
            window.setContentMinSize(NSSize::new(WIDTH, HEIGHT));
            // The content is drawn for the notch's black and dark appearance.
            window.setBackgroundColor(Some(&NSColor::blackColor()));
            window.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.center();
            window
        });
        if !self.ivars().dashboard_open.replace(true) {
            // The motion would otherwise fade the content out as the notch collapses.
            ui.motion.drive_content(false);
            // A frame-based host: as the window's own content view, AppKit would switch the
            // view to constraints and it would stretch with the notch on the way back.
            let host = window.contentView().expect("window has a content view");
            ui.content.setFrame(host.bounds());
            ui.content.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            host.addSubview(&ui.content);
            self.ivars().pinned.set(false);
            self.set_expanded(false);
            // The Insights tab shows only here.
            self.refresh();
        }
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }

    /// Puts the content view back in the notch exactly as `build_ui` left it.
    fn hide_dashboard(&self) {
        let ui = self.ui();
        attach_to_notch(&ui.content, &ui.panel.contentView().expect("panel has a content view"), ui.expanded.get().size.height - TOP_SLOP);
        self.ivars().dashboard_open.set(false);
        ui.motion.drive_content(true);
        // Back in the notch: the Insights tab goes, and a view left on it falls back to a section.
        self.refresh();
    }

    /// Right-click menu for the task at `index`; each item carries the index in its tag.
    fn task_menu(&self, index: usize, priority: &str) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let item = |title: &str, action| {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(title), Some(action), &NSString::new())
            };
            unsafe { item.setTarget(Some(self.target())) };
            item.setTag(index as isize);
            item
        };
        let levels = NSMenu::initWithTitle(mtm.alloc(), &NSString::new());
        levels.setAutoenablesItems(false);
        for title in ["High", "Medium", "Low", "None"] {
            let level = item(title, sel!(setPriority:));
            if title.to_lowercase() == priority || (title == "None" && priority.is_empty()) {
                level.setState(NSControlStateValueOn);
            }
            levels.addItem(&level);
        }
        let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::new());
        menu.setAutoenablesItems(false);
        let add = |title: &str, action| menu.addItem(&item(title, action));
        let parent = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str("Priority"), None, &NSString::new()) };
        parent.setSubmenu(Some(&levels));
        menu.addItem(&parent);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Move to Tomorrow", sel!(moveToTomorrow:));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Delete", sel!(deleteTask:));
        menu
    }

    /// Sets the day of the task at `index`, keeping its place in the list.
    fn reschedule(&self, index: usize, day: String) {
        if let Some(task) = self.ivars().tasks.borrow_mut().get_mut(index) {
            task.day = day;
        }
        self.save_and_refresh();
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
        let (collapsed, expanded, safe_top) = notch_rects(mtm, self.tab_size());

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
        // The delegate is on the action responder chain, which routes ⌘, to `openSettings:`.
        panel.setDelegate(Some(ProtocolObject::from_ref(self)));

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
        let content = NSView::new(mtm);
        content.setHidden(true);
        attach_to_notch(&content, &background, expanded.size.height - TOP_SLOP);

        // The header sits in the menu-bar band: close top-left as on macOS windows, then two
        // tab groups, one each side of the hardware notch.
        let head = self.build_header(mtm);
        let header = NSView::new(mtm);
        header.setTranslatesAutoresizingMaskIntoConstraints(false);
        unsafe { header.setMenu(Some(&self.header_menu())) };
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
        let clips = self.build_clips(mtm);
        let music = self.build_music(mtm);
        let terminal = self.build_terminal(mtm);
        let insights = InsightsView::new(mtm);
        for body in [&*workspace, &*clips.body, &*music.body, &*terminal.body, insights.body()] {
            content.addSubview(body);
            body.leadingAnchor().constraintEqualToAnchor_constant(&content.leadingAnchor(), 12.0).setActive(true);
            body.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), -12.0).setActive(true);
            body.topAnchor().constraintEqualToAnchor_constant(&header.bottomAnchor(), 8.0).setActive(true);
            body.bottomAnchor().constraintEqualToAnchor_constant(&content.bottomAnchor(), -12.0).setActive(true);
        }
        for card in [&focus_view, &task_view] {
            card.heightAnchor().constraintEqualToAnchor(&workspace.heightAnchor()).setActive(true);
        }

        let activity = LiveActivity::new(collapsed, mtm);
        // Under the content, so the open panel never shows the wings through it.
        background.addSubview_positioned_relativeTo(&activity.view, NSWindowOrderingMode::Below, Some(&content));
        let motion = NotchMotion::new(panel.clone(), content.clone(), collapsed, expanded, mtm);
        Ui {
            panel,
            content,
            workspace,
            tab_workspace: head.workspace,
            tab_insights: head.insights,
            tab_clips: head.clips,
            tab_music: head.music,
            tab_terminal: head.terminal,
            clips,
            music,
            terminal,
            insights,
            focus,
            tasks,
            expanded: Cell::new(expanded),
            header_height,
            notch_gaps,
            motion,
            activity,
            banner: NotchBanner::new(self.target(), sel!(snoozeReminder:), sel!(finishReminder:), sel!(dismissBanner:), mtm),
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
        let insights = pill("Insights", sel!(showInsights:));
        let clips = icon_pill("doc.on.clipboard", sel!(showClips:));
        let music = icon_pill("play.circle", sel!(showMusic:));
        let terminal = pill("Terminal", sel!(showTerminal:));
        // Settings is otherwise only reachable through ⌘, or the header's right-click menu.
        let settings = w::icon_button("gearshape", "", 11.0, &white(0.55), self.target(), sel!(openSettings:), mtm);
        settings.setToolTip(Some(&NSString::from_str("Settings (⌘,)")));
        Header {
            quit: w::icon_button("xmark", "", 10.0, &white(0.55), self.target(), sel!(quit:), mtm),
            left: w::stack(&[&settings, &workspace, &insights], false, 8.0, mtm),
            right: w::stack(&[&clips, &music, &terminal], false, 6.0, mtm),
            workspace,
            insights,
            clips,
            music,
            terminal,
        }
    }

    fn build_focus_card(&self, mtm: MainThreadMarker) -> (Retained<NSView>, FocusCard) {
        let card = w::filled(&tone(SURFACE), 18.0, mtm);
        card.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) }.as_deref());
        card.widthAnchor().constraintEqualToConstant(FOCUS_CARD_WIDTH).setActive(true);
        let inner = FOCUS_CARD_WIDTH - 28.0;
        let coral = rgba(CORAL.0, CORAL.1, CORAL.2, 1.0);

        let (count, count_chip) = chip(&white(0.9), &rgba(0, 0, 0, 0.4), 7.0, 3.0, mtm);
        count_chip.setToolTip(Some(&NSString::from_str("Completed pomodoros count toward today; ended early are logged, not counted.")));
        count.setContentCompressionResistancePriority_forOrientation(751.0, NSLayoutConstraintOrientation::Horizontal);
        count.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
        let daily_focus = w::label("", 10.0, unsafe { NSFontWeightMedium }, &white(0.5), mtm);
        let footer = w::stack(&[&count_chip, &w::spacer(mtm), &daily_focus], false, 4.0, mtm);

        let tomatoes: Vec<_> = (0..CYCLE).map(|_| TomatoView::new(mtm)).collect();
        let views: Vec<&NSView> = tomatoes.iter().map(|t| &***t).collect();
        for view in &views {
            fixed_size(view, TOMATO_SIZE, TOMATO_SIZE);
        }
        let grid = w::stack(&views, false, 10.0, mtm);

        let task = w::label("", 11.0, unsafe { NSFontWeightRegular }, &white(0.55), mtm);
        task.setAlignment(NSTextAlignment::Center);
        let (badge, badge_pill) = chip(&rgba(CORAL.0, CORAL.1, CORAL.2, 1.0), &rgba(CORAL.0, CORAL.1, CORAL.2, 0.22), 6.0, 3.0, mtm);
        badge.setFont(Some(&NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
        let round = w::label("", 11.0, unsafe { NSFontWeightMedium }, &white(0.55), mtm);
        let header = w::stack(&[&badge_pill, &w::spacer(mtm), &round], false, 8.0, mtm);
        let time = w::label("", 36.0, unsafe { NSFontWeightLight }, &white(0.95), mtm);
        time.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(36.0, unsafe { NSFontWeightLight })));
        // Clicking the idle countdown cycles focus length, including the stopwatch.
        let cycle = unsafe { NSClickGestureRecognizer::initWithTarget_action(mtm.alloc(), Some(self.target()), Some(sel!(cycleDuration:))) };
        time.addGestureRecognizer(&cycle);
        let ends = w::label("", 10.0, unsafe { NSFontWeightRegular }, &white(0.55), mtm);
        let ring = FocusRing::new(mtm);
        ring.heightAnchor().constraintEqualToAnchor(&ring.widthAnchor()).setActive(true);
        ring.widthAnchor().constraintGreaterThanOrEqualToConstant(120.0).setActive(true);
        let preferred = ring.widthAnchor().constraintEqualToConstant(164.0);
        // In the compact notch the ring must shrink before any text row is compressed.
        preferred.setPriority(200.0);
        preferred.setActive(true);
        let clock = w::stack(&[&time, &ends], true, 0.0, mtm);
        clock.setAlignment(NSLayoutAttribute::CenterX);
        ring.addSubview(&clock);
        clock.centerXAnchor().constraintEqualToAnchor(&ring.centerXAnchor()).setActive(true);
        clock.centerYAnchor().constraintEqualToAnchor(&ring.centerYAnchor()).setActive(true);

        let pause = w::pill("Pause", 28.0, self.target(), sel!(toggleTimer:), mtm);
        w::style_pill(&pause, &coral, &white(1.0), 14.0);
        let button_width = (inner - 16.0 - PLUS_WIDTH) / 2.0;
        pause.widthAnchor().constraintEqualToConstant(button_width).setActive(true);
        let end = w::pill("End", 28.0, self.target(), sel!(endTimer:), mtm);
        w::style_pill(&end, &tone(SURFACE_RAISED), &white(0.85), 14.0);
        end.widthAnchor().constraintEqualToConstant(button_width).setActive(true);
        let plus = w::pill("+5", 28.0, self.target(), sel!(addFive:), mtm);
        w::style_pill(&plus, &tone(SURFACE_RAISED), &white(0.85), 14.0);
        plus.widthAnchor().constraintEqualToConstant(PLUS_WIDTH).setActive(true);
        let buttons = w::stack(&[&pause, &end, &plus], false, 8.0, mtm);

        let rule = w::filled(&white(0.08), 0.0, mtm);
        rule.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
        let column = w::stack(&[&header, &task, &ring, &grid, &buttons, &rule, &footer], true, 4.0, mtm);
        column.setAlignment(NSLayoutAttribute::CenterX);
        card.addSubview(&column);
        column.centerXAnchor().constraintEqualToAnchor(&card.centerXAnchor()).setActive(true);
        column.centerYAnchor().constraintEqualToAnchor(&card.centerYAnchor()).setActive(true);
        column.topAnchor().constraintGreaterThanOrEqualToAnchor_constant(&card.topAnchor(), 10.0).setActive(true);
        column.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&card.bottomAnchor(), -10.0).setActive(true);
        let full_width: [&NSView; 5] = [&header, &buttons, &task, &rule, &footer];
        for view in full_width {
            view.widthAnchor().constraintEqualToConstant(inner).setActive(true);
        }

        (card, FocusCard { ring, round, daily_focus, count, tomatoes, task, badge, time, ends, pause, end, plus })
    }

    fn build_task_card(&self, mtm: MainThreadMarker) -> (Retained<NSView>, TaskCard) {
        let card = w::filled(&tone(SURFACE), 18.0, mtm);

        let title = w::label("Today’s tasks", 15.0, unsafe { NSFontWeightBold }, &white(0.9), mtm);
        let date = w::label(&days::label(0), 12.0, unsafe { NSFontWeightMedium }, &white(0.55), mtm);
        let head = w::stack(&[&title, &w::spacer(mtm), &date], false, 8.0, mtm);
        let todo = w::pill("To do", 22.0, self.target(), sel!(showTodo:), mtm);
        let done = w::pill("Completed", 22.0, self.target(), sel!(showCompleted:), mtm);
        let clear = w::pill("Clear", 22.0, self.target(), sel!(clearCompleted:), mtm);
        w::fit_pill(&clear, 10.0);
        w::style_pill(&clear, &white(0.0), &white(0.55), 7.0);
        let note = w::label("", 11.0, unsafe { NSFontWeightMedium }, &tone(CORAL), mtm);
        note.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        note.setContentCompressionResistancePriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        note.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        let undo = w::pill("Undo", 22.0, self.target(), sel!(undoDelete:), mtm);
        w::fit_pill(&undo, 10.0);
        w::style_pill(&undo, &white(0.1), &white(0.95), 7.0);
        undo.setHidden(true);
        let pills = w::stack(&[&todo, &done, &note, &undo, &clear], false, 4.0, mtm);
        let rows = w::stack(&[], true, CARD_ROW_GAP, mtm);
        let top = w::stack(&[&head, &pills, &rows], true, 10.0, mtm);
        card.addSubview(&top);
        w::pin(&top, &card, 14.0, None);
        for view in [&*head, &*pills, &*rows] {
            view.widthAnchor().constraintEqualToAnchor(&top.widthAnchor()).setActive(true);
        }

        let input = w::input("Add a task · type ! # or / for hints", &white(0.9), 0, self.target(), sel!(addTask:), mtm);
        unsafe { input.setDelegate(Some(ProtocolObject::from_ref(self))) };
        let bottom = self.add_row(&input, &white(0.5), mtm);
        let dots: Vec<_> = PRIORITIES
            .iter()
            .enumerate()
            .map(|(i, level)| {
                let dot = w::icon_button("circle.fill", "", 9.0, &priority_color(level), self.target(), sel!(pickPriority:), mtm);
                dot.setTag(i as isize);
                dot.setToolTip(Some(&NSString::from_str(&format!("New tasks: {}", if level.is_empty() { "no priority".to_owned() } else { meta_priority(level) }))));
                dot
            })
            .collect();
        bottom.addArrangedSubview(&w::stack(&dots.iter().map(|d| &**d as &NSView).collect::<Vec<_>>(), false, 6.0, mtm));
        let line = w::filled(&white(0.1), 0.0, mtm);
        card.addSubview(&line);
        card.addSubview(&bottom);
        for view in [&*line, &*bottom] {
            view.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), 14.0).setActive(true);
            view.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -14.0).setActive(true);
        }
        line.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
        line.bottomAnchor().constraintEqualToAnchor_constant(&bottom.topAnchor(), -8.0).setActive(true);
        bottom.bottomAnchor().constraintEqualToAnchor_constant(&card.bottomAnchor(), -10.0).setActive(true);

        // Over the bottom of the list, opaque, so it reads cleanly above the input while typing.
        let hints = w::stack(&[], false, 4.0, mtm);
        let hint_bar = w::filled(&tone(SURFACE), 0.0, mtm);
        hint_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        hint_bar.addSubview(&hints);
        w::pin(&hints, &hint_bar, 0.0, None);
        hint_bar.setHidden(true);
        card.addSubview(&hint_bar);
        hint_bar.leadingAnchor().constraintEqualToAnchor_constant(&card.leadingAnchor(), 14.0).setActive(true);
        hint_bar.trailingAnchor().constraintEqualToAnchor_constant(&card.trailingAnchor(), -14.0).setActive(true);
        hint_bar.bottomAnchor().constraintEqualToAnchor_constant(&line.topAnchor(), -6.0).setActive(true);
        hint_bar.heightAnchor().constraintEqualToConstant(28.0).setActive(true);

        (card, TaskCard { todo, done, clear, note, undo, hint_bar, hints, dots, rows, input })
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

    /// A card holding the terminal view.
    fn build_terminal(&self, mtm: MainThreadMarker) -> TerminalPane {
        let body = w::filled(&tone(SURFACE), 18.0, mtm);
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
/// The content view sits at its fixed size inside the notch shape; `NotchMotion` centres it.
fn attach_to_notch(content: &NSView, background: &NSView, height: f64) {
    let ear = panel::ear(height + TOP_SLOP);
    content.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
    content.setFrame(NSRect::new(NSPoint::new(ear, 0.0), NSSize::new(WIDTH, height)));
    background.addSubview(content);
}

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

/// `23:57`, for the focus card and the collapsed notch.
fn completed_in_cycle(done: usize, on_break: bool) -> usize {
    if on_break && done > 0 && done % CYCLE == 0 { CYCLE } else { done % CYCLE }
}

fn cycle_tomatoes(done: usize, on_break: bool, active: bool, ripeness: f64) -> [Tomato; CYCLE] {
    let completed = completed_in_cycle(done, on_break);
    std::array::from_fn(|i| {
        if i < completed { Tomato::Ripe }
        else if i == completed && active && !on_break { Tomato::Growing(ripeness) }
        else { Tomato::Empty }
    })
}

/// The preset after `idx`, then the stopwatch, then back to the first.
fn next_duration(idx: usize) -> usize {
    (idx + 1) % (STOPWATCH + 1)
}

/// Minutes to log when a session is ended early. A countdown always logs; a stopwatch only from one minute.
fn end_log_minutes(stopwatch: bool, elapsed_secs: u64) -> Option<u64> {
    (!stopwatch || elapsed_secs >= 60).then_some(elapsed_secs / 60)
}

fn mmss(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// `m:ss` for a position or duration in seconds.
fn clock(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Open task scheduled for today or rolled over from an earlier day (Inbox excluded).
/// Indices of today's open tasks, most urgent first (within a priority, timed tasks in clock
/// order, then untimed ones in stored order), and of today's completed tasks.
fn today_lists(tasks: &[Task], today: &str) -> (Vec<usize>, Vec<usize>) {
    let mut todo: Vec<usize> = (0..tasks.len()).filter(|&i| is_open_today(&tasks[i], today)).collect();
    todo.sort_by_key(|&i| (priority_rank(&tasks[i].priority), tasks[i].time.is_empty(), tasks[i].time.as_str()));
    let done = (0..tasks.len()).filter(|&i| tasks[i].done && tasks[i].day == today).collect();
    (todo, done)
}

/// A Markdown checklist in the quick-add syntax, so a line pasted back into the input keeps
/// its time, priority and project.
fn checklist<'a>(tasks: impl Iterator<Item = &'a Task>) -> String {
    tasks
        .map(|t| {
            let mut line = format!("- [{}] ", if t.done { 'x' } else { ' ' });
            if !t.time.is_empty() {
                line += &format!("{} ", t.time);
            }
            line += &t.text;
            if !t.priority.is_empty() {
                line += &format!(" !{}", t.priority);
            }
            if !t.project.is_empty() {
                line += &format!(" #{}", t.project);
            }
            line + "\n"
        })
        .collect()
}

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
        if !task.priority.is_empty() {
            let tint = priority_color(&task.priority);
            // Tints only the priority word, so the rest of the line stays quiet.
            let word = NSString::from_str(&meta_priority(&task.priority));
            let range = NSString::from_str(&meta).rangeOfString(&word);
            let text = NSMutableAttributedString::initWithAttributedString(NSMutableAttributedString::alloc(), &line.attributedStringValue());
            unsafe { text.addAttribute_value_range(NSForegroundColorAttributeName, &tint, range) };
            line.setAttributedStringValue(&text);
        }
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
    if let Some(time) = clock_12h(&task.time) {
        parts.push(time);
    }
    if !task.priority.is_empty() {
        parts.push(meta_priority(&task.priority));
    }
    if !task.project.is_empty() {
        parts.push(task.project.clone());
    }
    if task.minutes > 0 {
        parts.push(format!("{} min", task.minutes));
    }
    parts.join(" · ")
}

/// `17:00` → `5:00 PM`.
fn clock_12h(time: &str) -> Option<String> {
    let (h, m) = time.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    Some(format!("{}:{m} {}", (h + 11) % 12 + 1, if h < 12 { "AM" } else { "PM" }))
}

/// Minute of the day now.
fn now_minutes() -> Option<u32> {
    task_alerts::minutes(days::stamp().split_once(' ')?.1)
}

/// The timer preset a new session starts on.
fn focus_index(settings: &Settings) -> usize {
    DURATIONS_MIN.iter().position(|m| *m == settings.focus).unwrap_or(1)
}

/// `high` → `High`, as the metadata line shows it.
fn meta_priority(priority: &str) -> String {
    let mut chars = priority.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Sort order, most urgent first: high, medium, low, then no priority.
fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

/// Priorities in the order of the Today card's dots: most urgent first, then none.
const PRIORITIES: [&str; 4] = ["high", "medium", "low", ""];

/// Colour of a priority's check circle, dot and metadata word.
fn priority_color(priority: &str) -> Retained<NSColor> {
    match priority {
        "high" => rgba(235, 87, 87, 1.0),
        "medium" => rgba(242, 153, 74, 1.0),
        "low" => rgba(113, 124, 230, 1.0),
        _ => white(0.6),
    }
}

/// Strikes through a done task's title.
fn strike(label: &NSTextField) {
    let text = NSMutableAttributedString::initWithAttributedString(NSMutableAttributedString::alloc(), &label.attributedStringValue());
    let range = NSRange::new(0, text.length());
    let style = NSNumber::new_isize(NSUnderlineStyle::Single.0);
    unsafe { text.addAttribute_value_range(NSStrikethroughStyleAttributeName, &style, range) };
    label.setAttributedStringValue(&text);
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

/// Returns (collapsed rect over the notch, expanded rect for `size`, hardware safe-area height).
fn notch_rects(mtm: MainThreadMarker, size: Size) -> (NSRect, NSRect, f64) {
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
    let body = body_height(size, screen.visibleFrame().size.height);
    let (collapsed, expanded) = frames_at_top(frame, width, height, body);
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

/// Height of the expanded body for `size`, kept clear of the screen's edges.
fn body_height(size: Size, visible_height: f64) -> f64 {
    (HEIGHT * size.scale()).min(visible_height - SCREEN_MARGIN)
}

fn frames_at_top(frame: NSRect, width: f64, height: f64, body: f64) -> (NSRect, NSRect) {
    let top = frame.origin.y + frame.size.height;
    let mid = frame.origin.x + frame.size.width / 2.0;
    // Body plus an ear on each side, extended TOP_SLOP above the screen.
    let rect = |w: f64, h: f64| {
        let ear = panel::ear(h + TOP_SLOP);
        NSRect::new(NSPoint::new(mid - w / 2.0 - ear, top - h), NSSize::new(w + 2.0 * ear, h + TOP_SLOP))
    };
    (rect(width, height), rect(WIDTH, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_tomatoes_follow_the_current_cycle() {
        assert_eq!(cycle_tomatoes(0, false, false, 0.0), [Tomato::Empty; CYCLE]);
        assert_eq!(cycle_tomatoes(1, false, true, 0.5), [Tomato::Ripe, Tomato::Growing(0.5), Tomato::Empty, Tomato::Empty]);
        assert_eq!(cycle_tomatoes(1, true, true, 0.5), [Tomato::Ripe, Tomato::Empty, Tomato::Empty, Tomato::Empty]);
        assert_eq!(cycle_tomatoes(4, true, true, 0.5), [Tomato::Ripe; CYCLE]);
        assert_eq!(cycle_tomatoes(4, false, true, 0.0), [Tomato::Growing(0.0), Tomato::Empty, Tomato::Empty, Tomato::Empty]);
        assert_eq!(cycle_tomatoes(9, false, false, 0.0), [Tomato::Ripe, Tomato::Empty, Tomato::Empty, Tomato::Empty]);
    }

    #[test]
    fn checklist_keeps_time_priority_and_project() {
        let open = Task { text: "Gym".into(), time: "18:00".into(), priority: "high".into(), project: "Health".into(), ..Task::default() };
        let done = Task { text: "Mail".into(), done: true, ..Task::default() };
        assert_eq!(checklist([&open, &done].into_iter()), "- [ ] 18:00 Gym !high #Health\n- [x] Mail\n");
    }

    #[test]
    fn today_lists_sort_open_and_keep_done_apart() {
        let task = |text: &str, day: &str, time: &str, priority: &str, done: bool| Task {
            text: text.into(), day: day.into(), time: time.into(), priority: priority.into(), done, ..Task::default()
        };
        let tasks = [
            task("later", "D", "", "", false),
            task("tomorrow", "E", "", "high", false),
            task("evening", "D", "18:00", "low", false),
            task("urgent", "D", "", "high", false),
            task("done", "D", "", "", true),
        ];
        assert_eq!(today_lists(&tasks, "D"), (vec![3, 2, 0], vec![4]));
    }

    #[test]
    fn priorities_sort_most_urgent_first_and_none_last() {
        let mut levels = ["", "low", "high", "medium"];
        levels.sort_by_key(|p| priority_rank(p));
        assert_eq!(levels, ["high", "medium", "low", ""]);
        assert_eq!(meta_priority("medium"), "Medium");
    }

    #[test]
    fn duration_cycle_visits_the_stopwatch_then_wraps() {
        let cycle: Vec<usize> = std::iter::successors(Some(0), |&i| Some(next_duration(i))).take(6).collect();
        assert_eq!(cycle, vec![0, 1, 2, 3, STOPWATCH, 0]);
    }

    #[test]
    fn stopwatch_logs_only_from_one_minute() {
        assert_eq!((end_log_minutes(true, 59), end_log_minutes(true, 60), end_log_minutes(true, 185)), (None, Some(1), Some(3)));
        assert_eq!((end_log_minutes(false, 20), end_log_minutes(false, 600)), (Some(0), Some(10)));
    }

    #[test]
    fn elapsed_clock_keeps_counting_past_an_hour() {
        assert_eq!((mmss(0), mmss(3725)), ("00:00".to_owned(), "62:05".to_owned()));
    }

    #[test]
    fn countdown_pads_minutes_and_seconds() {
        assert_eq!((mmss(0), mmss(59), mmss(1437), mmss(3600)), ("00:00".into(), "00:59".into(), "23:57".into(), "60:00".into()));
    }

    #[test]
    fn body_height_scales_and_stays_on_screen() {
        assert_eq!(body_height(Size::Compact, 1000.0), HEIGHT);
        assert_eq!(body_height(Size::Tall, 1000.0), HEIGHT * 1.6);
        assert_eq!(body_height(Size::ExtraTall, 1200.0), HEIGHT * 2.5);
        assert_eq!(body_height(Size::ExtraTall, 700.0), 700.0 - SCREEN_MARGIN);
    }

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
                let (collapsed, expanded) = frames_at_top(screen, width, height, HEIGHT);
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
