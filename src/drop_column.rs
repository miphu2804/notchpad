//! A Week-board column card that accepts task rows dropped onto it.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSColor, NSDragOperation, NSDraggingInfo, NSPasteboardTypeString, NSView};
use objc2_foundation::{NSArray, NSPoint};

use crate::widgets as w;

/// Called with the dragged task's index and the drop point in window coordinates.
pub type OnDrop = Box<dyn Fn(usize, NSPoint)>;

const RADIUS: f64 = 16.0;

pub struct Ivars {
    fill: Retained<NSColor>,
    highlight: Retained<NSColor>,
    on_drop: OnDrop,
}

define_class!(
    /// Lightens while a row hovers over it, then hands the drop to `on_drop`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "DropColumn"]
    pub struct DropColumn;

    impl DropColumn {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, _info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            w::paint(self, &self.ivars().highlight, RADIUS);
            NSDragOperation::Move
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            w::paint(self, &self.ivars().fill, RADIUS);
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            w::paint(self, &self.ivars().fill, RADIUS);
            let text = info.draggingPasteboard().stringForType(unsafe { NSPasteboardTypeString });
            let index = text.and_then(|t| t.to_string().parse::<usize>().ok());
            if let Some(index) = index {
                (self.ivars().on_drop)(index, info.draggingLocation());
            }
            index.is_some()
        }
    }
);

impl DropColumn {
    pub fn new(fill: Retained<NSColor>, highlight: Retained<NSColor>, on_drop: OnDrop, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Ivars { fill, highlight, on_drop });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        w::paint(&this, &this.ivars().fill, RADIUS);
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeString }]));
        this
    }
}
