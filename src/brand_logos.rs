//! Service icons: vendor app icons embedded from assets/integrations/ (see its README), the
//! installed Claude app's own icon, and a glyph tile for Claude Code, which has no app icon.

use block2::RcBlock;
use objc2::{AnyThread, rc::Retained, runtime::Bool};
use objc2_app_kit::{NSBezierPath, NSColor, NSGraphicsContext, NSImage, NSImageSymbolConfiguration, NSWorkspace};
use objc2_foundation::{NSArray, NSData, NSPoint, NSRect, NSSize, NSString};

const LINEAR: &[u8] = include_bytes!("../assets/integrations/linear.png");
const NOTION: &[u8] = include_bytes!("../assets/integrations/notion.png");
const GOOGLE_CALENDAR: &[u8] = include_bytes!("../assets/integrations/google-calendar.png");
const CLAUDE_BUNDLE: &str = "com.anthropic.claudefordesktop";
/// Height of the row's icon slot; every logo is drawn into a canvas this tall.
const SLOT: f64 = 22.0;

/// The logo for a settings row, or `None` to fall back to the row's SF Symbol.
pub fn logo(name: &str) -> Option<Retained<NSImage>> {
    match name {
        "linear" => Some(tiles(vec![png(LINEAR)], 20.0)),
        // Two smaller tiles, so the pair stays clear of the row's text column.
        "calendars" => Some(tiles(vec![png(GOOGLE_CALENDAR), png(NOTION)], 18.0)),
        // The Claude Desktop row keeps its generic bubble only when Claude isn't installed.
        "bubble.left" => claude_icon(),
        "terminal" => Some(glyph_tile("terminal")),
        _ => None,
    }
}

/// Square vendor icons side by side, each clipped to the same rounded tile so full-bleed
/// icons (Linear's black, Notion's white) read as app icons rather than hard squares.
fn tiles(icons: Vec<Retained<NSImage>>, size: f64) -> Retained<NSImage> {
    const GAP: f64 = 3.0;
    let width = icons.len() as f64 * (size + GAP) - GAP;
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        for (i, icon) in icons.iter().enumerate() {
            let frame = NSRect::new(NSPoint::new(i as f64 * (size + GAP), (SLOT - size) / 2.0), NSSize::new(size, size));
            NSGraphicsContext::saveGraphicsState_class();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, size * 0.225, size * 0.225).addClip();
            icon.drawInRect(frame);
            NSGraphicsContext::restoreGraphicsState_class();
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(width, SLOT), false, &draw)
}

/// The installed Claude app's icon, enlarged past the slot because macOS icons carry a
/// transparent margin; `None` when Claude isn't installed.
fn claude_icon() -> Option<Retained<NSImage>> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(CLAUDE_BUNDLE))?;
    let path = app.path()?;
    let icon = workspace.iconForFile(&path);
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        icon.drawInRect(NSRect::new(NSPoint::new(-2.0, -2.0), NSSize::new(SLOT + 4.0, SLOT + 4.0)));
        Bool::YES
    });
    Some(NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(SLOT, SLOT), false, &draw))
}

/// An SF Symbol in white on a graphite tile, so a service with no app icon still matches the
/// vendor tiles beside it.
fn glyph_tile(symbol: &str) -> Retained<NSImage> {
    const SIZE: f64 = 20.0;
    let white = NSImageSymbolConfiguration::configurationWithPaletteColors(&NSArray::from_retained_slice(&[NSColor::whiteColor()]));
    let glyph = crate::widgets::symbol(symbol).imageWithSymbolConfiguration(&white).unwrap_or_else(|| crate::widgets::symbol(symbol));
    let draw = RcBlock::new(move |_: NSRect| -> Bool {
        let tile = NSRect::new(NSPoint::new(1.0, 1.0), NSSize::new(SIZE, SIZE));
        NSColor::colorWithWhite_alpha(0.28, 1.0).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(tile, SIZE * 0.225, SIZE * 0.225).fill();
        // Fit the glyph's own aspect inside the tile's inner 70%.
        let size = glyph.size();
        let scale = (SIZE * 0.7) / size.width.max(size.height);
        let (w, h) = (size.width * scale, size.height * scale);
        glyph.drawInRect(NSRect::new(NSPoint::new((SLOT - w) / 2.0, (SLOT - h) / 2.0), NSSize::new(w, h)));
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(SLOT, SLOT), false, &draw)
}

fn png(bytes: &[u8]) -> Retained<NSImage> {
    NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes)).expect("bundled service icon is a PNG")
}
