//! A task-list row that tints under the pointer and animates in and out of its stack.

use std::cell::Cell;
use std::collections::HashSet;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSColor, NSEvent, NSStackView, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect};
use objc2_quartz_core::CAMediaTimingFunction;

use crate::widgets as w;

/// How far the hover tint reaches past the row on each side.
const TINT_OUTSET: (f64, f64) = (6.0, 3.0);
const FADE_OUT_SECS: f64 = 0.14;
const SLIDE_SECS: f64 = 0.28;

pub struct Ivars {
    /// Identifies the task across re-renders, which rebuild every row.
    key: String,
    tint: Retained<NSView>,
    /// A removed row kept on screen only to animate out: ignores the pointer.
    leaving: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "ListRow"]
    pub struct ListRow;

    unsafe impl NSObjectProtocol for ListRow {}

    impl ListRow {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            if !self.ivars().leaving.get() {
                w::fade(&self.ivars().tint, 1.0, 0.12);
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            w::fade(&self.ivars().tint, 0.0, 0.22);
        }

        // Raw pointer because hitTest: returns +0; the hit view is kept alive by the tree.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            if self.ivars().leaving.get() {
                return std::ptr::null_mut();
            }
            let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
            hit.map_or(std::ptr::null_mut(), |view| Retained::as_ptr(&view).cast_mut())
        }

        /// Second half of leaving: once faded out, the row's slot closes smoothly.
        #[unsafe(method(closeSlot:))]
        fn close_slot(&self, _arg: Option<&AnyObject>) {
            let Some(root) = self.window().and_then(|window| window.contentView()) else { return };
            begin_animation(0.22);
            self.setHidden(true);
            root.layoutSubtreeIfNeeded();
            NSAnimationContext::endGrouping();
        }
    }
);

impl ListRow {
    /// Wraps `content`; `tint` fills the rounded hover highlight behind it.
    pub fn new(key: &str, content: &NSView, tint: &NSColor, mtm: MainThreadMarker) -> Retained<Self> {
        let highlight = w::filled(tint, 7.0, mtm);
        highlight.setAlphaValue(0.0);
        let this = mtm.alloc::<Self>().set_ivars(Ivars {
            key: key.to_owned(),
            tint: highlight,
            leaving: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);

        let tint = &this.ivars().tint;
        this.addSubview(tint);
        let (dx, dy) = TINT_OUTSET;
        tint.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), -dx).setActive(true);
        tint.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), dx).setActive(true);
        tint.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), -dy).setActive(true);
        tint.bottomAnchor().constraintEqualToAnchor_constant(&this.bottomAnchor(), dy).setActive(true);
        this.addSubview(content);
        w::pin(content, &this, 0.0, Some(0.0));

        let tracking = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                mtm.alloc(),
                NSRect::ZERO,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&this),
                None,
            )
        };
        this.addTrackingArea(&tracking);
        this
    }

    /// Hairline centred in the `gap` above the row. It belongs to the row, so it leaves
    /// with it and the list's layout doesn't depend on separators.
    pub fn add_rule(&self, color: &NSColor, gap: f64, mtm: MainThreadMarker) {
        let line = w::filled(color, 0.0, mtm);
        self.addSubview(&line);
        line.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()).setActive(true);
        line.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()).setActive(true);
        line.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
        line.bottomAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), -((gap - 1.0) / 2.0).round()).setActive(true);
    }

    fn key(&self) -> &str {
        &self.ivars().key
    }
}

/// The rows `stack` shows now, with their positions. Take this before rebuilding the
/// stack, then hand it to `animate_changes`.
pub fn snapshot(stack: &NSStackView) -> Vec<(usize, Retained<ListRow>)> {
    stack
        .arrangedSubviews()
        .iter()
        .enumerate()
        .filter_map(|(i, view)| view.downcast::<ListRow>().ok().map(|row| (i, row)))
        .filter(|(_, row)| !row.ivars().leaving.get())
        .collect()
}

/// Animates a rebuilt `stack` from the rows in `before`: rows that left fade out at their
/// old place and then their slot closes; new rows fade in while the others slide to make
/// room. Does nothing while the stack is off screen or Reduce Motion is on.
pub fn animate_changes(stack: &NSStackView, before: Vec<(usize, Retained<ListRow>)>) {
    if stack.isHiddenOrHasHiddenAncestor() || w::reduce_motion() {
        return;
    }
    let Some(root) = stack.window().and_then(|window| window.contentView()) else { return };
    let after = snapshot(stack);
    let old_keys: HashSet<&str> = before.iter().map(|(_, row)| row.key()).collect();
    let new_keys: HashSet<&str> = after.iter().map(|(_, row)| row.key()).collect();
    let leaving: Vec<_> = before.iter().filter(|(_, row)| !new_keys.contains(row.key())).collect();
    let arriving: Vec<_> = after.iter().map(|(_, row)| row).filter(|row| !old_keys.contains(row.key())).collect();
    if leaving.is_empty() && arriving.is_empty() {
        return;
    }

    for (index, row) in &leaving {
        row.ivars().leaving.set(true);
        row.ivars().tint.setAlphaValue(0.0);
        let count = stack.arrangedSubviews().count();
        stack.insertArrangedSubview_atIndex(row, (*index).min(count) as isize);
        row.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);
    }
    // Lay out the end state to learn where new rows land, then the start state (new rows
    // hidden), and pin new rows at their landing spot so they fade in place rather than
    // grow out of a corner.
    root.layoutSubtreeIfNeeded();
    let landing: Vec<NSRect> = arriving.iter().map(|row| row.frame()).collect();
    for row in &arriving {
        row.setHidden(true);
        row.setAlphaValue(0.0);
    }
    root.layoutSubtreeIfNeeded();
    for (row, frame) in arriving.iter().zip(landing) {
        row.setFrame(frame);
    }

    begin_animation(SLIDE_SECS);
    for row in &arriving {
        row.setHidden(false);
        row.animator().setAlphaValue(1.0);
    }
    root.layoutSubtreeIfNeeded();
    NSAnimationContext::endGrouping();

    let none: Option<&AnyObject> = None;
    for (_, row) in leaving {
        w::fade(row, 0.0, FADE_OUT_SECS);
        let _: () = unsafe { msg_send![&**row, performSelector: sel!(closeSlot:), withObject: none, afterDelay: FADE_OUT_SECS] };
    }
}

/// Opens an animation group in which layout changes animate too; close it with
/// `NSAnimationContext::endGrouping`.
fn begin_animation(seconds: f64) {
    NSAnimationContext::beginGrouping();
    let context = NSAnimationContext::currentContext();
    context.setDuration(seconds);
    context.setAllowsImplicitAnimation(true);
    // Ease-out: quick start, long gentle settle.
    context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(0.2, 0.9, 0.3, 1.0)));
}
