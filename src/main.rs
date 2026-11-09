mod chime;
mod clip_card;
mod clips;
mod controller;
mod days;
mod export;
mod hotkey;
mod insights;
mod insights_chart;
mod insights_view;
mod list_row;
mod live_activity;
mod notch_banner;
mod now_playing;
mod notch_motion;
mod panel;
mod pill_button;
mod pomodoros;
mod quick_add;
mod settings;
mod settings_window;
mod store;
mod task_alerts;
mod terminal_keys;
mod terminal_palette;
mod terminal_session;
mod terminal_view;
mod tomato_view;
mod week_bars;
mod widgets;

use std::cell::RefCell;
use std::ffi::c_void;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use controller::Controller;

thread_local! {
    /// The hotkey callback is a bare C function, so it reaches the controller through here.
    static CONTROLLER: RefCell<Option<Retained<Controller>>> = const { RefCell::new(None) };
}

extern "C" fn on_hotkey(_next: *mut c_void, _event: *mut c_void, _user: *mut c_void) -> i32 {
    CONTROLLER.with(|c| {
        if let Some(controller) = c.borrow().as_ref() {
            controller.toggle_pinned();
        }
    });
    0
}

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    CONTROLLER.with(|c| *c.borrow_mut() = Some(Controller::new(mtm)));
    if !hotkey::register(on_hotkey) {
        eprintln!("notchpad: could not register Option-N; hover still works");
    }
    app.run();
}
