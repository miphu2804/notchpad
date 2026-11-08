//! Owns the notch window, the tasks and the focus timer, and renders the Workspace and
//! Week tabs. Every change re-renders from `State`; the views hold no state of their own.

use std::cell::{Cell, OnceCell, RefCell};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSBackingStoreType, NSButton, NSCellImagePosition, NSColor, NSEvent, NSFont,
    NSFontWeightBold, NSFontWeightLight, NSFontWeightMedium, NSFontWeightRegular,
    NSFontWeightSemibold, NSImageView, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSMainMenuWindowLevel, NSScreen, NSStackView,
    NSStackViewDistribution, NSStackViewGravity, NSTextAlignment, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer};

use crate::days;
use crate::logo::LogoMark;
use crate::panel::{self, NotchPanel, NotchShape};
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
const PROGRESS_WIDTH: f64 = 150.0;
const MAX_CARD_ROWS: usize = 5;
const MAX_COLUMN_ROWS: usize = 6;
const DURATIONS_MIN: [u64; 4] = [15, 25, 45, 60];

/// Week columns, also used as the add-field tag that picks the new task's day.
const INBOX: isize = 0;
const TODAY: isize = 1;
const TOMORROW: isize = 2;

unsafe extern "C" {
    fn NSBeep();
}

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
    title: Retained<NSTextField>,
    time: Retained<NSTextField>,
    status: Retained<NSTextField>,
    progress: Retained<NSLayoutConstraint>,
    start: Retained<NSButton>,
    set_time: Retained<NSButton>,
}

struct TaskCard {
    todo: Retained<NSButton>,
    done: Retained<NSButton>,
    rows: Retained<NSStackView>,
    input: Retained<NSTextField>,
}

struct Ui {
    panel: Retained<NotchPanel>,
    content: Retained<NSView>,
    workspace: Retained<NSStackView>,
    week: Retained<NSStackView>,
    tab_workspace: Retained<NSButton>,
    tab_week: Retained<NSButton>,
    focus: FocusCard,
    tasks: TaskCard,
    columns: Vec<Column>,
    collapsed: NSRect,
    expanded: NSRect,
}

#[derive(Default)]
pub struct State {
    ui: OnceCell<Ui>,
    tasks: RefCell<Vec<Task>>,
    expanded: Cell<bool>,
    /// Opened by hotkey: stays open until the pointer has visited and left.
    pinned: Cell<bool>,
    week_tab: Cell<bool>,
    show_completed: Cell<bool>,
    /// Task index chosen with ▶; falls back to the first open task of today.
    focus: Cell<Option<usize>>,
    duration_idx: Cell<usize>,
    deadline: Cell<Option<Instant>>,
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
                let day = match field.tag() {
                    INBOX => String::new(),
                    TOMORROW => days::key(1),
                    _ => days::key(0),
                };
                self.ivars().tasks.borrow_mut().push(Task { text: text.to_owned(), done: false, day, ext: String::new() });
                self.save_and_refresh();
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
            self.ivars().week_tab.set(false);
            self.refresh();
        }

        #[unsafe(method(showWeek:))]
        fn show_week(&self, _sender: &AnyObject) {
            self.ivars().week_tab.set(true);
            self.refresh();
        }

        #[unsafe(method(toggleTimer:))]
        fn toggle_timer(&self, _sender: &AnyObject) {
            if self.ivars().deadline.get().is_some() {
                self.stop_timer();
            } else {
                self.start_timer();
            }
        }

        /// ✓ on the focus card: the focused task is done.
        #[unsafe(method(finishFocus:))]
        fn finish_focus(&self, _sender: &AnyObject) {
            if let Some(i) = self.focused_task() {
                self.ivars().tasks.borrow_mut()[i].done = true;
                self.ivars().focus.set(None);
            }
            self.stop_timer();
            self.save_and_refresh();
        }

        #[unsafe(method(cycleDuration:))]
        fn cycle_duration(&self, _sender: &AnyObject) {
            let next = (self.ivars().duration_idx.get() + 1) % DURATIONS_MIN.len();
            self.ivars().duration_idx.set(next);
            self.render_focus();
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            let Some(deadline) = self.ivars().deadline.get() else { return };
            if Instant::now() >= deadline {
                self.stop_timer();
                unsafe { NSBeep() };
                self.ivars().week_tab.set(false);
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

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let state = State {
            tasks: RefCell::new(store::load()),
            duration_idx: Cell::new(1), // 25 min
            ..State::default()
        };
        let this = mtm.alloc::<Self>().set_ivars(state);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let ui = this.build_ui(mtm);
        this.ivars().ui.set(ui).ok().expect("ui is built once");
        this.ivars().seen_mtime.set(store::modified());
        unsafe {
            // Lives for the whole process, so the returned timer handle is not kept.
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(2.0, this.target(), sel!(reload:), None, true);
        }
        this.refresh();
        this.ui().panel.orderFrontRegardless();
        this
    }

    /// Hotkey entry point: open and focus today's input, or close if already open.
    pub fn toggle_pinned(&self) {
        if self.ivars().expanded.get() {
            self.ivars().pinned.set(false);
            self.set_expanded(false);
        } else {
            self.ivars().week_tab.set(false);
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
        if !self.ivars().expanded.get() || self.ivars().pinned.get() {
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
    fn pointer_over_panel(&self) -> bool {
        let p = NSEvent::mouseLocation();
        let f = self.ui().panel.frame();
        p.x >= f.origin.x
            && p.x <= f.origin.x + f.size.width
            && p.y >= f.origin.y
            && p.y <= f.origin.y + f.size.height
    }

    fn set_expanded(&self, on: bool) {
        let ui = self.ui();
        if self.ivars().expanded.replace(on) == on {
            return;
        }
        // Content shows only at full size so it never squeezes during the animation.
        if on {
            ui.panel.setFrame_display_animate(ui.expanded, true, true);
            ui.content.setHidden(false);
        } else {
            ui.content.setHidden(true);
            ui.panel.setFrame_display_animate(ui.collapsed, true, true);
        }
    }

    fn save_and_refresh(&self) {
        if let Err(err) = store::save(&self.ivars().tasks.borrow()) {
            eprintln!("notchpad: could not save tasks: {err}");
        }
        self.ivars().seen_mtime.set(store::modified());
        self.refresh();
    }

    fn refresh(&self) {
        self.render_tabs();
        self.render_focus();
        self.render_task_card();
        self.render_week();
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

    fn start_timer(&self) {
        let secs = DURATIONS_MIN[self.ivars().duration_idx.get()] * 60;
        self.ivars().deadline.set(Some(Instant::now() + Duration::from_secs(secs)));
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(1.0, self.target(), sel!(tick:), None, true)
        };
        *self.ivars().ticker.borrow_mut() = Some(timer);
        self.render_focus();
    }

    fn stop_timer(&self) {
        self.ivars().deadline.set(None);
        if let Some(timer) = self.ivars().ticker.borrow_mut().take() {
            timer.invalidate();
        }
        self.render_focus();
    }

    // ---- rendering -----------------------------------------------------------------

    fn render_tabs(&self) {
        let ui = self.ui();
        let week = self.ivars().week_tab.get();
        ui.workspace.setHidden(week);
        ui.week.setHidden(!week);
        let (on, off) = if week { (&ui.tab_week, &ui.tab_workspace) } else { (&ui.tab_workspace, &ui.tab_week) };
        w::style_pill(on, &white(1.0), &NSColor::blackColor(), 12.0);
        w::style_pill(off, &white(0.1), &white(0.8), 12.0);
    }

    fn render_focus(&self) {
        let card = &self.ui().focus;
        let state = self.ivars();
        let total = DURATIONS_MIN[state.duration_idx.get()] * 60;
        let (remaining, running) = match state.deadline.get() {
            Some(deadline) => (deadline.saturating_duration_since(Instant::now()).as_secs() + 1, true),
            None => (total, false),
        };
        let title = match self.focused_task() {
            Some(i) => state.tasks.borrow()[i].text.clone(),
            None => "Pick a task with ▶".to_owned(),
        };
        card.title.setStringValue(&NSString::from_str(&title));
        card.time.setStringValue(&NSString::from_str(&format!("{:02}:{:02}", remaining / 60, remaining % 60)));
        card.status.setStringValue(&NSString::from_str(if running { "Focusing" } else { "Ready" }));
        let elapsed = total.saturating_sub(remaining) as f64 / total as f64;
        card.progress.setConstant(PROGRESS_WIDTH * if running { elapsed } else { 0.0 });
        card.start.setTitle(&NSString::from_str(if running { "  Stop" } else { "  Start" }));
        card.start.setImage(Some(&w::symbol(if running { "stop.fill" } else { "play.fill" })));
        card.set_time.setTitle(&NSString::from_str(&format!(" Set time · {}m", DURATIONS_MIN[state.duration_idx.get()])));
        card.set_time.setEnabled(!running);
    }

    fn render_task_card(&self) {
        let card = &self.ui().tasks;
        let today = days::key(0);
        let tasks = self.ivars().tasks.borrow();
        let todo: Vec<usize> = (0..tasks.len()).filter(|&i| is_open_today(&tasks[i], &today)).collect();
        let done: Vec<usize> = (0..tasks.len()).filter(|&i| tasks[i].done && tasks[i].day == today).collect();

        let show_completed = self.ivars().show_completed.get();
        card.todo.setTitle(&NSString::from_str(&format!("To do  {}", todo.len())));
        card.done.setTitle(&NSString::from_str(&format!("Completed  {}", done.len())));
        let (on, off) = if show_completed { (&card.done, &card.todo) } else { (&card.todo, &card.done) };
        w::style_pill(on, &white(0.45), &ink(0.9), 7.0);
        w::style_pill(off, &white(0.0), &ink(0.55), 7.0);
        w::fit_pill(&card.todo, 10.0);
        w::fit_pill(&card.done, 10.0);

        clear(&card.rows);
        let shown = if show_completed { &done } else { &todo };
        for (k, &i) in shown.iter().take(MAX_CARD_ROWS).enumerate() {
            if k > 0 {
                add_full_width(&card.rows, &w::filled(&ink(0.08), 0.0, self.mtm()), Some(1.0));
            }
            add_full_width(&card.rows, &self.card_row(i, &tasks[i]), None);
        }
        let empty = if show_completed { "Nothing completed yet today" } else { "All clear. Add a task below." };
        self.add_overflow_note(&card.rows, shown.len(), MAX_CARD_ROWS, empty, &ink(0.5));
    }

    fn card_row(&self, index: usize, task: &Task) -> Retained<NSView> {
        let mtm = self.mtm();
        let circle = if task.done { "checkmark.circle.fill" } else { "circle" };
        let check = w::icon_button(circle, "", 15.0, &ink(0.7), self.target(), sel!(toggleTask:), mtm);
        check.setTag(index as isize);
        let alpha = if task.done { 0.45 } else { 0.9 };
        let title = w::label(&task.text, 13.0, unsafe { NSFontWeightRegular }, &ink(alpha), mtm);
        title.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
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
            clear(&ui.list);
            for &i in shown.iter().take(MAX_COLUMN_ROWS) {
                add_full_width(&ui.list, &self.week_row(i, &tasks[i]), None);
            }
            self.add_overflow_note(&ui.list, shown.len(), MAX_COLUMN_ROWS, "", &white(0.4));
        }
    }

    fn week_row(&self, index: usize, task: &Task) -> Retained<NSView> {
        let mtm = self.mtm();
        let check = w::icon_button("circle", "", 14.0, &white(0.55), self.target(), sel!(toggleTask:), mtm);
        check.setTag(index as isize);
        let title = w::label(&task.text, 13.0, unsafe { NSFontWeightRegular }, &white(0.9), mtm);
        title.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        w::stack(&[&check, &title], false, 8.0, mtm).into_super()
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
        let (collapsed, expanded, notch_height) = notch_rects(mtm);

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

        let background_alloc = mtm.alloc::<NotchShape>().set_ivars(());
        let background: Retained<NotchShape> = unsafe { msg_send![super(background_alloc), initWithFrame: collapsed] };
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
        // Sits between the notch's ears, below the off-screen slop.
        let ear = panel::ear(HEIGHT + TOP_SLOP);
        let content = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::new(ear, 0.0), NSSize::new(WIDTH, HEIGHT)));
        content.setHidden(true);
        background.addSubview(&content);

        let (header, tab_workspace, tab_week) = self.build_header(mtm);
        content.addSubview(&header);
        header.leadingAnchor().constraintEqualToAnchor_constant(&content.leadingAnchor(), 18.0).setActive(true);
        header.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), -14.0).setActive(true);
        header.topAnchor().constraintEqualToAnchor(&content.topAnchor()).setActive(true);
        header.heightAnchor().constraintEqualToConstant(notch_height.max(32.0)).setActive(true);

        let (focus_view, focus) = self.build_focus_card(mtm);
        let (task_view, tasks) = self.build_task_card(mtm);
        let workspace = w::stack(&[&focus_view, &task_view], false, 10.0, mtm);
        workspace.setDistribution(NSStackViewDistribution::Fill);
        task_view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        let (week, columns) = self.build_week(mtm);
        for body in [&workspace, &week] {
            content.addSubview(body);
            body.leadingAnchor().constraintEqualToAnchor_constant(&content.leadingAnchor(), 12.0).setActive(true);
            body.trailingAnchor().constraintEqualToAnchor_constant(&content.trailingAnchor(), -12.0).setActive(true);
            body.topAnchor().constraintEqualToAnchor_constant(&header.bottomAnchor(), 8.0).setActive(true);
            body.bottomAnchor().constraintEqualToAnchor_constant(&content.bottomAnchor(), -12.0).setActive(true);
        }
        for card in [&focus_view, &task_view] {
            card.heightAnchor().constraintEqualToAnchor(&workspace.heightAnchor()).setActive(true);
        }

        Ui { panel, content, workspace, week, tab_workspace, tab_week, focus, tasks, columns, collapsed, expanded }
    }

    fn build_header(&self, mtm: MainThreadMarker) -> (Retained<NSStackView>, Retained<NSButton>, Retained<NSButton>) {
        let logo = LogoMark::new(20.0, mtm);
        fixed_size(&logo, 20.0, 20.0);
        let name = w::label("Notchpad", 14.0, unsafe { NSFontWeightSemibold }, &white(1.0), mtm);
        let tab_workspace = w::pill("Workspace", 24.0, self.target(), sel!(showWorkspace:), mtm);
        let tab_week = w::pill("Week", 24.0, self.target(), sel!(showWeek:), mtm);
        w::fit_pill(&tab_workspace, 12.0);
        w::fit_pill(&tab_week, 12.0);
        let quit = w::icon_button("xmark", "", 10.0, &white(0.55), self.target(), sel!(quit:), mtm);

        let header = w::stack(&[], false, 8.0, mtm);
        header.addView_inGravity(&logo, NSStackViewGravity::Leading);
        header.addView_inGravity(&name, NSStackViewGravity::Leading);
        header.addView_inGravity(&tab_workspace, NSStackViewGravity::Trailing);
        header.addView_inGravity(&tab_week, NSStackViewGravity::Trailing);
        header.addView_inGravity(&quit, NSStackViewGravity::Trailing);
        (header, tab_workspace, tab_week)
    }

    fn build_focus_card(&self, mtm: MainThreadMarker) -> (Retained<NSView>, FocusCard) {
        let card = w::filled(&rgba(169, 165, 203, 1.0), 18.0, mtm);
        card.setAppearance(unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) }.as_deref());
        card.widthAnchor().constraintEqualToConstant(FOCUS_CARD_WIDTH).setActive(true);

        let title = w::label("", 13.0, unsafe { NSFontWeightSemibold }, &ink(0.85), mtm);
        title.setAlignment(NSTextAlignment::Center);
        let time = w::label("", 46.0, unsafe { NSFontWeightLight }, &ink(0.75), mtm);
        time.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(46.0, unsafe { NSFontWeightLight })));
        let status = w::label("", 11.0, unsafe { NSFontWeightMedium }, &ink(0.5), mtm);

        let track = w::filled(&ink(0.12), 1.5, mtm);
        track.widthAnchor().constraintEqualToConstant(PROGRESS_WIDTH).setActive(true);
        track.heightAnchor().constraintEqualToConstant(3.0).setActive(true);
        let fill = w::filled(&ink(0.6), 1.5, mtm);
        track.addSubview(&fill);
        fill.leadingAnchor().constraintEqualToAnchor(&track.leadingAnchor()).setActive(true);
        fill.topAnchor().constraintEqualToAnchor(&track.topAnchor()).setActive(true);
        fill.bottomAnchor().constraintEqualToAnchor(&track.bottomAnchor()).setActive(true);
        let progress = fill.widthAnchor().constraintEqualToConstant(0.0);
        progress.setActive(true);

        let start = w::icon_button("play.fill", "  Start", 12.0, &white(1.0), self.target(), sel!(toggleTimer:), mtm);
        start.setImagePosition(NSCellImagePosition::ImageLeading);
        start.setImageHugsTitle(true);
        w::paint(&start, &ink(0.38), 15.0);
        fixed_size(&start, 150.0, 30.0);
        let check = w::icon_button("checkmark", "", 12.0, &ink(0.7), self.target(), sel!(finishFocus:), mtm);
        w::paint(&check, &ink(0.12), 15.0);
        fixed_size(&check, 30.0, 30.0);
        let buttons = w::stack(&[&start, &check], false, 8.0, mtm);
        let set_time = w::icon_button("slider.horizontal.3", "", 11.0, &ink(0.55), self.target(), sel!(cycleDuration:), mtm);
        set_time.setImagePosition(NSCellImagePosition::ImageLeading);

        let column = w::stack(&[&title, &time, &status, &track, &buttons, &set_time], true, 8.0, mtm);
        column.setAlignment(NSLayoutAttribute::CenterX);
        column.setCustomSpacing_afterView(14.0, &track);
        card.addSubview(&column);
        column.centerXAnchor().constraintEqualToAnchor(&card.centerXAnchor()).setActive(true);
        column.centerYAnchor().constraintEqualToAnchor(&card.centerYAnchor()).setActive(true);
        title.widthAnchor().constraintLessThanOrEqualToConstant(FOCUS_CARD_WIDTH - 32.0).setActive(true);

        (card, FocusCard { title, time, status, progress, start, set_time })
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
        let rows = w::stack(&[], true, 7.0, mtm);
        let top = w::stack(&[&head, &pills, &rows], true, 10.0, mtm);
        card.addSubview(&top);
        w::pin(&top, &card, 14.0, None);
        for view in [&*head, &*rows] {
            view.widthAnchor().constraintEqualToAnchor(&top.widthAnchor()).setActive(true);
        }

        let input = w::input("Add a task…", &ink(0.9), TODAY, self.target(), sel!(addTask:), mtm);
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
            let card = w::filled(&rgba(42, 42, 45, 1.0), 16.0, mtm);
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

    /// "+ <input>" row used under every task list.
    fn add_row(&self, input: &NSTextField, tint: &NSColor, mtm: MainThreadMarker) -> Retained<NSStackView> {
        let plus = NSImageView::imageViewWithImage(&w::symbol("plus"), mtm);
        plus.setContentTintColor(Some(tint));
        input.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        w::stack(&[&plus, input], false, 6.0, mtm)
    }
}

/// Open task scheduled for today or rolled over from an earlier day (Inbox excluded).
fn is_open_today(task: &Task, today: &str) -> bool {
    !task.done && !task.day.is_empty() && task.day.as_str() <= today
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

/// Returns (collapsed rect over the notch, expanded rect, notch height).
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
    let top = frame.origin.y + frame.size.height;
    let mid = frame.origin.x + frame.size.width / 2.0;
    // Body plus an ear on each side, extended TOP_SLOP above the screen.
    let rect = |w: f64, h: f64| {
        let ear = panel::ear(h + TOP_SLOP);
        NSRect::new(NSPoint::new(mid - w / 2.0 - ear, top - h), NSSize::new(w + 2.0 * ear, h + TOP_SLOP))
    };
    (rect(width, height), rect(WIDTH, HEIGHT), height)
}
