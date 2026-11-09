//! Open/close motion of the notch panel: a spring on the window frame, with the content
//! fading and scaling in once the panel is mostly open and fading out before it closes.

use std::cell::{Cell, OnceCell};

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize};
use objc2_quartz_core::{CADisplayLink, CATransform3D};

use crate::panel::NotchPanel;
use crate::widgets as w;

/// SwiftUI-style spring: `response` is the period in seconds, `damping` the damping ratio.
struct Spring {
    response: f64,
    damping: f64,
}

/// Slight overshoot on open, none on close: the drawn notch must never dip below the
/// hardware one.
const OPEN: Spring = Spring { response: 0.42, damping: 0.78 };
const CLOSE: Spring = Spring { response: 0.34, damping: 1.0 };
/// Content starts fading in once the frame is this far open.
const FADE_IN_AT: f64 = 0.55;
const FADE_IN_SECS: f64 = 0.18;
const FADE_OUT_SECS: f64 = 0.09;
/// Content scale when fully faded out; it grows to 1 as it fades in.
const MIN_SCALE: f64 = 0.97;

pub struct Ivars {
    panel: Retained<NotchPanel>,
    content: Retained<NSView>,
    collapsed: Cell<NSRect>,
    expanded: Cell<NSRect>,
    link: OnceCell<Retained<CADisplayLink>>,
    open: Cell<bool>,
    /// 0 = collapsed frame, 1 = expanded frame; overshoots past 1 while springing open.
    progress: Cell<f64>,
    velocity: Cell<f64>,
    target: Cell<f64>,
    /// Content opacity before easing, 0..=1.
    fade: Cell<f64>,
    reduce_motion: Cell<bool>,
    /// Extra width of the collapsed frame while a live activity shows; springs like the frame.
    wing: Cell<f64>,
    wing_velocity: Cell<f64>,
    wing_target: Cell<f64>,
    last_frame: Cell<Option<f64>>,
    /// False while the content lives in the dashboard window: fading and hiding it would blank that window.
    drives_content: Cell<bool>,
}

define_class!(
    /// Drives the panel from a display link that runs only while something moves, so an
    /// idle panel costs no CPU. Retargeting mid-flight keeps the current velocity.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "NotchMotion"]
    pub struct NotchMotion;

    unsafe impl NSObjectProtocol for NotchMotion {}

    impl NotchMotion {
        #[unsafe(method(step:))]
        fn step(&self, link: &CADisplayLink) {
            let now = link.timestamp();
            let dt = match self.ivars().last_frame.replace(Some(now)) {
                Some(last) => (now - last).clamp(0.0, 1.0 / 30.0),
                None => 1.0 / 120.0,
            };
            self.advance(dt);
            let settled = self.settled();
            self.apply();
            if settled {
                link.setPaused(true);
                self.ivars().last_frame.set(None);
                if !self.ivars().open.get() && self.ivars().drives_content.get() {
                    self.ivars().content.setHidden(true);
                }
            }
        }
    }
);

impl NotchMotion {
    /// `content` is a fixed-size view inside the panel's content view; it stays pinned to
    /// the panel's bottom edge and centred, so it slides out from under the notch.
    pub fn new(panel: Retained<NotchPanel>, content: Retained<NSView>, collapsed: NSRect, expanded: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let ivars = Ivars {
            panel,
            content,
            collapsed: Cell::new(collapsed),
            expanded: Cell::new(expanded),
            link: OnceCell::new(),
            open: Cell::new(false),
            progress: Cell::new(0.0),
            velocity: Cell::new(0.0),
            target: Cell::new(0.0),
            fade: Cell::new(0.0),
            reduce_motion: Cell::new(false),
            wing: Cell::new(0.0),
            wing_velocity: Cell::new(0.0),
            wing_target: Cell::new(0.0),
            last_frame: Cell::new(None),
            drives_content: Cell::new(true),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.ivars().content.setWantsLayer(true);
        this.apply();
        this
    }

    /// Replace both animation endpoints without travelling across the desktop or
    /// changing the current open/close progress.
    pub fn set_rects(&self, collapsed: NSRect, expanded: NSRect) {
        self.ivars().collapsed.set(collapsed);
        self.ivars().expanded.set(expanded);
        self.apply();
    }

    /// Hands the content to, or takes it back from, another window. Taking it back
    /// re-applies the current fade, so a collapsed notch gets its content hidden again.
    pub fn drive_content(&self, on: bool) {
        let content = &self.ivars().content;
        self.ivars().drives_content.set(on);
        if on {
            content.setHidden(!self.ivars().open.get());
            self.apply();
        } else {
            content.setHidden(false);
            content.setAlphaValue(1.0);
            if let Some(layer) = content.layer() {
                layer.setTransform(CATransform3D::new_translation(0.0, 0.0, 0.0));
            }
        }
    }

    pub fn set_open(&self, open: bool) {
        let s = self.ivars();
        s.open.set(open);
        s.reduce_motion.set(w::reduce_motion());
        if open && s.drives_content.get() {
            s.content.setHidden(false);
        }
        self.wake();
    }

    /// Grows (or shrinks) the collapsed frame by `width` in total, evenly on both sides.
    pub fn set_wings(&self, width: f64) {
        self.ivars().wing_target.set(width);
        self.ivars().reduce_motion.set(w::reduce_motion());
        self.wake();
    }

    fn wake(&self) {
        let s = self.ivars();
        // Made on first use, once the panel is on screen. The link retains its target;
        // both live as long as the app. While the display sleeps it doesn't fire, and the
        // motion resumes when the display wakes.
        let link = s.link.get_or_init(|| {
            let link = unsafe { s.panel.displayLinkWithTarget_selector(self, sel!(step:)) };
            unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
            link
        });
        link.setPaused(false);
    }

    fn advance(&self, dt: f64) {
        let s = self.ivars();
        let reduce = s.reduce_motion.get();
        // Critically damped, like closing: shrinking must never dip below the hardware notch.
        let (wing, wing_velocity) = if reduce {
            (s.wing_target.get(), 0.0)
        } else {
            integrate(&CLOSE, (s.wing.get(), s.wing_velocity.get()), s.wing_target.get(), dt)
        };
        s.wing.set(wing);
        s.wing_velocity.set(wing_velocity);
        let mut fade = s.fade.get();
        if s.open.get() {
            s.target.set(1.0);
            if reduce || s.progress.get() >= FADE_IN_AT {
                fade += dt / FADE_IN_SECS;
            }
        } else {
            fade -= dt / FADE_OUT_SECS;
            // Closing waits for the content to be gone; until then the frame keeps its course.
            if fade <= 0.0 {
                s.target.set(0.0);
            }
        }
        s.fade.set(fade.clamp(0.0, 1.0));

        let target = s.target.get();
        if reduce {
            s.progress.set(target);
            s.velocity.set(0.0);
            return;
        }
        let spring = if target > 0.5 { &OPEN } else { &CLOSE };
        let (x, v) = integrate(spring, (s.progress.get(), s.velocity.get()), target, dt);
        s.progress.set(x);
        s.velocity.set(v);
    }

    /// At rest: snaps the last sub-pixel of motion and reports true.
    fn settled(&self) -> bool {
        let s = self.ivars();
        let goal = if s.open.get() { 1.0 } else { 0.0 };
        let at_rest = s.target.get() == goal
            && s.fade.get() == goal
            && (s.progress.get() - goal).abs() < 1e-3
            && s.velocity.get().abs() < 1e-2
            && (s.wing.get() - s.wing_target.get()).abs() < 1e-2
            && s.wing_velocity.get().abs() < 1e-2;
        if at_rest {
            s.wing.set(s.wing_target.get());
            s.wing_velocity.set(0.0);
            s.progress.set(goal);
            s.velocity.set(0.0);
        }
        at_rest
    }

    fn apply(&self) {
        let s = self.ivars();
        let frame = self.frame_at(s.progress.get());
        if s.panel.frame() != frame {
            s.panel.setFrame_display(frame, true);
        }
        if !s.drives_content.get() {
            return;
        }
        let size = s.content.frame().size;
        s.content.setFrameOrigin(NSPoint::new(((frame.size.width - size.width) / 2.0).round(), 0.0));

        let f = s.fade.get();
        let eased = f * f * (3.0 - 2.0 * f);
        s.content.setAlphaValue(eased);
        let scale = if s.reduce_motion.get() { 1.0 } else { MIN_SCALE + (1.0 - MIN_SCALE) * eased };
        if let Some(layer) = s.content.layer() {
            layer.setTransform(scale_about(scale, size.width / 2.0, size.height));
        }
    }

    /// Panel frame for `progress`, kept centred under the notch and flush with the top.
    fn frame_at(&self, progress: f64) -> NSRect {
        let (a, b) = (self.ivars().collapsed.get(), self.ivars().expanded.get());
        let lerp = |from: f64, to: f64| from + (to - from) * progress;
        let width = lerp(a.size.width + self.ivars().wing.get(), b.size.width).round();
        let height = lerp(a.size.height, b.size.height).round();
        let mid = a.origin.x + a.size.width / 2.0;
        let top = a.origin.y + a.size.height;
        NSRect::new(NSPoint::new(mid - width / 2.0, top - height), NSSize::new(width, height))
    }
}

/// One frame of the spring towards `target`, from `(position, velocity)`.
fn integrate(spring: &Spring, (mut x, mut v): (f64, f64), target: f64, dt: f64) -> (f64, f64) {
    let omega = std::f64::consts::TAU / spring.response;
    let (stiffness, friction) = (omega * omega, 2.0 * spring.damping * omega);
    // Small fixed sub-steps keep the integration stable at any display rate.
    let steps = (dt * 480.0).ceil().max(1.0);
    let h = dt / steps;
    for _ in 0..steps as usize {
        v += (-stiffness * (x - target) - friction * v) * h;
        x += v * h;
    }
    (x, v)
}

/// Uniform scale about (`x`, `y`) in the layer's coordinates (AppKit layers anchor at 0,0).
fn scale_about(scale: f64, x: f64, y: f64) -> CATransform3D {
    let t = CATransform3D::new_translation(x, y, 0.0);
    let t = t.scale(scale, scale, 1.0);
    t.translate(-x, -y, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critically_damped_spring_settles_without_overshoot() {
        let (mut x, mut v) = (0.0, 0.0);
        for _ in 0..180 {
            (x, v) = integrate(&CLOSE, (x, v), 68.0, 1.0 / 60.0);
            assert!(x <= 68.0 + 1e-6, "overshot to {x}");
        }
        assert!((x - 68.0).abs() < 0.01 && v.abs() < 0.1);
    }
}
