//! The notch window: a borderless panel and the view that draws the notch shape.

use std::path::PathBuf;

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSDragOperation, NSDraggingInfo, NSEvent, NSEventModifierFlags, NSGlassEffectView, NSPanel,
    NSPasteboardTypeFileURL, NSScreen, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};
use objc2_quartz_core::CAShapeLayer;

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

        // The panel has no menu bar, so ⌘, is caught here and sent up the responder chain
        // (panel, then its delegate, the controller).
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
            let comma = event.charactersIgnoringModifiers().is_some_and(|c| c.to_string() == ",");
            if flags == NSEventModifierFlags::Command && comma {
                let app = NSApplication::sharedApplication(self.mtm());
                unsafe { app.sendAction_to_from(sel!(openSettings:), None, Some(self.as_ref())) }
            } else {
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
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
    glass: Cell<bool>,
    /// Behind everything else; visible only for the glass background.
    effect: Retained<NSView>,
}

define_class!(
    /// Notch silhouette, black or glass: concave ears at the top corners flowing into the bezel,
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
            // Glass draws itself: the effect view below is masked to the same outline.
            if !self.ivars().glass.get() {
                NSColor::blackColor().setFill();
                self.outline().fill();
            }
        }

        // The glass mask follows the panel as it springs open and closed.
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.update_mask();
        }
    }
);

impl NotchShape {
    pub fn new(frame: NSRect, on_file_drag: Box<dyn Fn(FileDrag)>, mtm: MainThreadMarker) -> Retained<Self> {
        let effect = effect_view(mtm);
        effect.setHidden(true);
        let ivars = ShapeIvars { on_file_drag, glass: Cell::new(false), effect };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        let effect = &this.ivars().effect;
        // The frame passed in is in screen coordinates; the effect view fills the shape's own bounds.
        effect.setFrame(this.bounds());
        effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        effect.setWantsLayer(true);
        unsafe { effect.layer().expect("layer-backed view").setMask(Some(&CAShapeLayer::new())) };
        this.addSubview(effect);
        this
    }

    pub fn set_look(&self, glass: bool) {
        let s = self.ivars();
        s.glass.set(glass);
        s.effect.setHidden(!glass);
        self.update_mask();
        self.setNeedsDisplay(true);
    }

    /// The silhouette, from the view's own size. The Bar shape is the same silhouette at a
    /// smaller size, so it reads as a notch on any display.
    fn outline(&self) -> Retained<NSBezierPath> {
        let b = self.bounds();
        outline(b.size.width, b.size.height)
    }

    fn update_mask(&self) {
        let mask = self.ivars().effect.layer().and_then(|layer| layer.mask());
        if let Some(mask) = mask.and_then(|m| m.downcast::<CAShapeLayer>().ok()) {
            mask.setFrame(self.bounds());
            mask.setPath(Some(&self.outline().CGPath()));
        }
    }
}

/// Glass on macOS 26 and later; older systems get a dark behind-window blur. Checked at run time
/// because the class does not exist before 26.
fn effect_view(mtm: MainThreadMarker) -> Retained<NSView> {
    if AnyClass::get(c"NSGlassEffectView").is_some() {
        let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        // Dark tint keeps the white text legible over bright wallpapers.
        glass.setTintColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, 0.5)));
        glass.setContentView(Some(&NSView::new(mtm)));
        Retained::into_super(glass)
    } else {
        let blur = NSVisualEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        blur.setMaterial(NSVisualEffectMaterial::HUDWindow);
        blur.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        // The panel never becomes the active window, which would otherwise leave the blur dimmed.
        blur.setState(NSVisualEffectState::Active);
        Retained::into_super(blur)
    }
}

/// Silhouette in a `w` by `h` view, ears included.
fn outline(w: f64, h: f64) -> Retained<NSBezierPath> {
    // Cubic Bézier factor for a quarter circle.
    const K: f64 = 0.5523;
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
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_reaches_the_view_edges_through_its_ears() {
        for (w, h) in [(160.0, 24.0), (185.0, 32.0), (640.0, 330.0)] {
            let bounds = outline(w, h).bounds();
            assert_eq!((bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height), (0.0, 0.0, w, h));
        }
    }
}
