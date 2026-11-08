//! A Week-board task row the user can drag to another column, like a kanban card.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSButton, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSEvent, NSPasteboardItem, NSPasteboardTypeString, NSView,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSString};

pub struct Ivars {
    index: usize,
}

define_class!(
    /// Wraps a row's content and carries the task's index in the store. Dragging anywhere
    /// except the checkbox starts a drag whose pasteboard string is that index.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "TaskRow"]
    pub struct TaskRow;

    unsafe impl NSObjectProtocol for TaskRow {}

    unsafe impl NSDraggingSource for TaskRow {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Move
        }
    }

    impl TaskRow {
        /// The checkbox keeps its clicks; the rest of the row (title included) is the handle.
        #[unsafe(method(hitTest:))]
        // Raw pointers because hitTest: returns +0; every candidate is kept alive by the view tree.
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
            match hit {
                Some(view) if view.isKindOfClass(NSButton::class()) => Retained::as_ptr(&view).cast_mut(),
                Some(_) => (self as *const Self).cast::<NSView>().cast_mut(),
                None => std::ptr::null_mut(),
            }
        }

        // The panel never activates the app, so the first click must already count.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        // Swallowed so the drag that follows is delivered here.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let payload = NSPasteboardItem::new();
            let index = NSString::from_str(&self.ivars().index.to_string());
            payload.setString_forType(&index, unsafe { NSPasteboardTypeString });
            let item = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                ProtocolObject::from_ref(&*payload),
            );
            unsafe { item.setDraggingFrame_contents(self.bounds(), Some(&crate::widgets::snapshot(self))) };
            self.beginDraggingSessionWithItems_event_source(
                &NSArray::from_retained_slice(&[item]),
                event,
                ProtocolObject::from_ref(self),
            );
        }
    }
);

impl TaskRow {
    pub fn new(index: usize, content: &NSView, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Ivars { index });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.addSubview(content);
        content.setTranslatesAutoresizingMaskIntoConstraints(false);
        content.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()).setActive(true);
        content.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()).setActive(true);
        content.topAnchor().constraintEqualToAnchor(&this.topAnchor()).setActive(true);
        content.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()).setActive(true);
        this
    }

    pub fn index(&self) -> usize {
        self.ivars().index
    }
}
