//! One clip hanging on the Clips line: click to copy it, drag it into any app.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent,
    NSFontWeightMedium, NSFontWeightRegular, NSImageScaling, NSImageView, NSLineBreakMode, NSView,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint};

use crate::clips::{Clip, Kind};
use crate::widgets::{self as w, rgba};

pub const WIDTH: f64 = 128.0;
pub const HEIGHT: f64 = 172.0;
/// Peg height; the line runs through its middle, which is also the face's top edge.
pub const PEG_HEIGHT: f64 = 8.0;
const PAD: f64 = 10.0;

pub struct Ivars {
    clip: Clip,
    /// Set once a drag starts, so the mouse-up that may follow isn't taken as a click.
    dragged: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "ClipCard"]
    pub struct ClipCard;

    unsafe impl NSObjectProtocol for ClipCard {}

    unsafe impl NSDraggingSource for ClipCard {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Copy
        }
    }

    impl ClipCard {
        // The whole card is one target; its labels and image must not take the clicks.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
            match hit {
                Some(_) => (self as *const Self).cast::<NSView>().cast_mut(),
                None => std::ptr::null_mut(),
            }
        }

        // The panel never activates the app, so the first click must already count.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().dragged.set(false);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if !self.ivars().dragged.get() {
                self.ivars().clip.copy();
                self.setAlphaValue(0.4);
                w::fade(self, 1.0, 0.35);
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if self.ivars().dragged.replace(true) {
                return;
            }
            let payload = self.ivars().clip.pasteboard_item();
            let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*payload));
            unsafe { item.setDraggingFrame_contents(self.bounds(), Some(&w::snapshot(self))) };
            self.beginDraggingSessionWithItems_event_source(
                &NSArray::from_retained_slice(&[item]),
                event,
                ProtocolObject::from_ref(self),
            );
        }
    }
);

impl ClipCard {
    pub fn new(clip: &Clip, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Ivars { clip: clip.clone(), dragged: Cell::new(false) });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor().constraintEqualToConstant(WIDTH).setActive(true);
        this.heightAnchor().constraintEqualToConstant(HEIGHT).setActive(true);
        this.build(clip, mtm);
        this
    }

    /// A rounded face with the preview on top (thumbnail, or the text itself) and a one-line
    /// caption at the bottom, hanging from a peg that straddles the face's top edge.
    fn build(&self, clip: &Clip, mtm: MainThreadMarker) {
        let white = |a| rgba(255, 255, 255, a);
        // The face starts below the card's top so the peg stays inside the card's bounds,
        // where layer clipping can't cut it.
        let face = w::filled(&white(0.08), 14.0, mtm);
        self.addSubview(&face);
        face.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()).setActive(true);
        face.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()).setActive(true);
        face.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()).setActive(true);
        face.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), PEG_HEIGHT / 2.0).setActive(true);
        let caption = match &clip.kind {
            Kind::Text(text) => format!("Text · {} chars", text.chars().count()),
            Kind::Image(_) => "Image".to_owned(),
            Kind::File(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        let caption = w::label(&caption, 11.0, unsafe { NSFontWeightMedium }, &white(0.5), mtm);
        caption.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);

        let preview: Retained<NSView> = match (&clip.kind, &clip.thumb) {
            (Kind::Text(text), _) => {
                let label = w::label(text.trim(), 12.0, unsafe { NSFontWeightRegular }, &white(0.9), mtm);
                label.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
                label.setMaximumNumberOfLines(7);
                label.setPreferredMaxLayoutWidth(WIDTH - 2.0 * PAD);
                label.into_super().into_super()
            }
            (_, thumb) => {
                let image = NSImageView::new(mtm);
                image.setImage(thumb.as_deref());
                image.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
                image.into_super().into_super()
            }
        };

        for view in [&*preview, &**caption] {
            self.addSubview(view);
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            view.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PAD).setActive(true);
            view.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PAD).setActive(true);
        }
        preview.topAnchor().constraintEqualToAnchor_constant(&face.topAnchor(), PAD + 4.0).setActive(true);
        caption.bottomAnchor().constraintEqualToAnchor_constant(&self.bottomAnchor(), -PAD).setActive(true);
        if matches!(clip.kind, Kind::Text(_)) {
            preview.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&caption.topAnchor(), -6.0).setActive(true);
        } else {
            preview.bottomAnchor().constraintEqualToAnchor_constant(&caption.topAnchor(), -6.0).setActive(true);
        }

        let peg = w::filled(&white(0.55), 2.0, mtm);
        self.addSubview(&peg);
        peg.widthAnchor().constraintEqualToConstant(16.0).setActive(true);
        peg.heightAnchor().constraintEqualToConstant(PEG_HEIGHT).setActive(true);
        peg.centerXAnchor().constraintEqualToAnchor(&self.centerXAnchor()).setActive(true);
        peg.topAnchor().constraintEqualToAnchor(&self.topAnchor()).setActive(true);
    }
}
