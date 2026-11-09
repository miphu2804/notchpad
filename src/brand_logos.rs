//! Service icons embedded from the vendors' published app icons; see assets/integrations/README.md.

use block2::RcBlock;
use objc2::{AnyThread, rc::Retained, runtime::Bool};
use objc2_app_kit::NSImage;
use objc2_foundation::{NSData, NSPoint, NSRect, NSSize};

const LINEAR: &[u8] = include_bytes!("../assets/integrations/linear.png");
const NOTION: &[u8] = include_bytes!("../assets/integrations/notion.png");
const GOOGLE_CALENDAR: &[u8] = include_bytes!("../assets/integrations/google-calendar.png");

/// Calendar services share a row, but keep separate, unmodified icons side by side.
pub fn logo(name: &str) -> Option<Retained<NSImage>> {
    match name {
        "linear" => Some(image(LINEAR)),
        "calendars" => {
            let google = image(GOOGLE_CALENDAR);
            let notion = image(NOTION);
            let draw = RcBlock::new(move |_: NSRect| -> Bool {
                // Two smaller icons, so the pair stays clear of the row's text column.
                google.drawInRect(NSRect::new(NSPoint::new(0.0, 3.0), NSSize::new(16.0, 16.0)));
                notion.drawInRect(NSRect::new(NSPoint::new(18.0, 3.0), NSSize::new(16.0, 16.0)));
                Bool::YES
            });
            Some(NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(34.0, 22.0), false, &draw))
        }
        _ => None,
    }
}

fn image(bytes: &[u8]) -> Retained<NSImage> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes)).expect("bundled service icon is a PNG");
    image.setSize(NSSize::new(22.0, 22.0));
    image
}
