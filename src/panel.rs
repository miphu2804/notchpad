//! The notch window: a borderless panel and the view that draws the notch shape.

use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSDragOperation, NSDraggingInfo, NSPanel, NSPasteboardTypeFileURL, NSScreen, NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect};

use crate::clips;

/// Width of the concave "ears" where the notch meets the top bezel, for a given height.
/// Matches the MacBook Pro (M4 Pro) notch when collapsed and grows a little when expanded.
pub fn ear(height: f64) -> f64 {
    (height * 0.04).clamp(6.0, 10.0)
}

/// Radius of the convex bottom corners for a given height.
fn bottom_radius(height: f64) -> f64 {
    (height * 0.3).clamp(9.0, 24.0)
}

define_class!(
    /// Borderless panel that can take keyboard focus (for the add-task fields) without
    /// activating the app, and may sit over the menu bar / notch area.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "NotchPanel"]
    pub struct NotchPanel;

    impl NotchPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        // The default implementation pushes the frame below the menu bar.
        #[unsafe(method(constrainFrameRect:toScreen:))]
        fn constrain(&self, rect: NSRect, _screen: Option<&NSScreen>) -> NSRect {
            rect
        }
    }
);

/// A file drag over the notch, reported to the controller.
pub enum FileDrag {
    Entered,
    Exited,
    Dropped(Vec<PathBuf>),
}

pub struct ShapeIvars {
    on_file_drag: Box<dyn Fn(FileDrag)>,
}

define_class!(
    /// Black notch silhouette: concave ears at the top corners flowing into the bezel,
    /// rounded bottom corners. Radii follow the current height, so the shape morphs
    /// smoothly while the window animates between collapsed and expanded.
    /// Also the drop target for files dragged onto the notch.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ShapeIvars]
    #[name = "NotchShape"]
    pub struct NotchShape;

    impl NotchShape {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, _info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            (self.ivars().on_file_drag)(FileDrag::Entered);
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            (self.ivars().on_file_drag)(FileDrag::Exited);
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let paths = clips::file_paths(&info.draggingPasteboard());
            let accepted = !paths.is_empty();
            (self.ivars().on_file_drag)(FileDrag::Dropped(paths));
            accepted
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            // Cubic Bézier factor for a quarter circle.
            const K: f64 = 0.5523;
            let b = self.bounds();
            let (w, h) = (b.size.width, b.size.height);
            let e = ear(h);
            let r = bottom_radius(h).min((w - 2.0 * e) / 2.0);
            let p = |x: f64, y: f64| NSPoint::new(x, y);

            let path = NSBezierPath::bezierPath();
            path.moveToPoint(p(0.0, h));
            path.curveToPoint_controlPoint1_controlPoint2(p(e, h - e), p(K * e, h), p(e, h - e + K * e));
            path.lineToPoint(p(e, r));
            path.curveToPoint_controlPoint1_controlPoint2(p(e + r, 0.0), p(e, r - K * r), p(e + r - K * r, 0.0));
            path.lineToPoint(p(w - e - r, 0.0));
            path.curveToPoint_controlPoint1_controlPoint2(p(w - e, r), p(w - e - r + K * r, 0.0), p(w - e, r - K * r));
            path.lineToPoint(p(w - e, h - e));
            path.curveToPoint_controlPoint1_controlPoint2(p(w, h), p(w - e, h - e + K * e), p(w - K * e, h));
            path.closePath();
            NSColor::blackColor().setFill();
            path.fill();
        }
    }
);

impl NotchShape {
    pub fn new(frame: NSRect, on_file_drag: Box<dyn Fn(FileDrag)>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ShapeIvars { on_file_drag });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        this
    }
}
