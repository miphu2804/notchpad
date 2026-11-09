//! Small AppKit view factories shared by the controller's builders.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AnyThread, ClassType, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSButton, NSColor, NSFocusRingType, NSFont,
    NSFontWeight, NSImage, NSImageSymbolConfiguration, NSLayoutConstraintOrientation,
    NSLineBreakMode, NSStackView, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
    NSWorkspace,
};
use objc2_foundation::NSString;

use crate::pill_button::PillButton;

/// The dark card surface shared by the notch and the dashboard.
pub const SURFACE: (u8, u8, u8) = (24, 25, 27);

pub fn rgba(r: u8, g: u8, b: u8, a: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, a)
}

pub fn label(text: &str, size: f64, weight: NSFontWeight, color: &NSColor, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
    label.setTextColor(Some(color));
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    label.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
    label
}

/// Plain view with a rounded, filled layer.
pub fn filled(color: &NSColor, radius: f64, mtm: MainThreadMarker) -> Retained<NSView> {
    let view = NSView::new(mtm);
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    paint(&view, color, radius);
    view
}

pub fn paint(view: &NSView, color: &NSColor, radius: f64) {
    view.setWantsLayer(true);
    let layer = view.layer().expect("layer-backed view");
    layer.setBackgroundColor(Some(&color.CGColor()));
    layer.setCornerRadius(radius);
}

pub fn symbol(name: &str) -> Retained<NSImage> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
        .unwrap_or_else(NSImage::new)
}

/// Borderless button showing an SF Symbol and optional title, tinted with `tint`.
pub fn icon_button(
    name: &str,
    title: &str,
    point_size: f64,
    tint: &NSColor,
    target: &AnyObject,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let image = symbol(name);
    let button = unsafe {
        if title.is_empty() {
            NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm)
        } else {
            NSButton::buttonWithTitle_image_target_action(&NSString::from_str(title), &image, Some(target), Some(action), mtm)
        }
    };
    button.setBordered(false);
    button.setContentTintColor(Some(tint));
    button.setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithPointSize_weight(
        point_size,
        unsafe { objc2_app_kit::NSFontWeightRegular },
    )));
    button.setFont(Some(&NSFont::systemFontOfSize_weight(point_size, unsafe { objc2_app_kit::NSFontWeightMedium })));
    button
}

/// Borderless text button on a rounded fill, padded around its title.
pub fn pill(title: &str, height: f64, target: &AnyObject, action: Sel, _mtm: MainThreadMarker) -> Retained<NSButton> {
    let title = NSString::from_str(title);
    let button: Retained<PillButton> =
        unsafe { msg_send![PillButton::class(), buttonWithTitle: &*title, target: Some(target), action: Some(action)] };
    let button = button.into_super();
    button.setBordered(false);
    button.setFont(Some(&NSFont::systemFontOfSize_weight(12.0, unsafe { objc2_app_kit::NSFontWeightMedium })));
    button.setTranslatesAutoresizingMaskIntoConstraints(false);
    button.heightAnchor().constraintEqualToConstant(height).setActive(true);
    button
}

/// Sizes a pill to its current title plus horizontal padding; call after changing the title.
pub fn fit_pill(button: &NSButton, padding: f64) {
    for constraint in button.constraints().iter() {
        if constraint.firstAttribute() == objc2_app_kit::NSLayoutAttribute::Width {
            button.removeConstraint(&constraint);
        }
    }
    let width = button.intrinsicContentSize().width + padding * 2.0;
    button.widthAnchor().constraintEqualToConstant(width).setActive(true);
}

/// Pill colours: `fill` behind the title, `text` for the title.
pub fn style_pill(button: &NSButton, fill: &NSColor, text: &NSColor, radius: f64) {
    paint(button, fill, radius);
    button.setContentTintColor(Some(text));
}

/// Single-line borderless input that sends `action` on Return.
pub fn input(placeholder: &str, color: &NSColor, tag: isize, target: &AnyObject, action: Sel, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::new(mtm);
    field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
    field.setBordered(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setFocusRingType(NSFocusRingType::None);
    field.setTextColor(Some(color));
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setTag(tag);
    unsafe {
        field.setTarget(Some(target));
        field.setAction(Some(action));
    }
    field
}

pub fn stack(views: &[&NSView], vertical: bool, spacing: f64, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    stack.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    if vertical {
        stack.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    } else {
        stack.setAlignment(objc2_app_kit::NSLayoutAttribute::CenterY);
    }
    stack.setSpacing(spacing);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    for view in views {
        stack.addArrangedSubview(view);
    }
    stack
}

/// Pins `inner` inside `outer`. `bottom: None` leaves the bottom edge free.
pub fn pin(inner: &NSView, outer: &NSView, inset: f64, bottom: Option<f64>) {
    inner.setTranslatesAutoresizingMaskIntoConstraints(false);
    inner.leadingAnchor().constraintEqualToAnchor_constant(&outer.leadingAnchor(), inset).setActive(true);
    inner.trailingAnchor().constraintEqualToAnchor_constant(&outer.trailingAnchor(), -inset).setActive(true);
    inner.topAnchor().constraintEqualToAnchor_constant(&outer.topAnchor(), inset).setActive(true);
    if let Some(b) = bottom {
        inner.bottomAnchor().constraintEqualToAnchor_constant(&outer.bottomAnchor(), -b).setActive(true);
    }
}

/// Flexible empty view that absorbs extra space in a stack.
pub fn spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    let view = NSView::new(mtm);
    view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Vertical);
    view
}

/// Image of `view` as drawn now, shown under the pointer while dragging it.
pub fn snapshot(view: &NSView) -> Retained<NSImage> {
    let bounds = view.bounds();
    let image = NSImage::initWithSize(NSImage::alloc(), bounds.size);
    if let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(bounds) {
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        image.addRepresentation(&rep);
    }
    image
}

/// System Settings › Accessibility › Display › Reduce motion.
pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Animates `view`'s opacity to `alpha`. Core Animation runs it, so no timer is involved.
pub fn fade(view: &NSView, alpha: f64, seconds: f64) {
    NSAnimationContext::beginGrouping();
    NSAnimationContext::currentContext().setDuration(seconds);
    view.animator().setAlphaValue(alpha);
    NSAnimationContext::endGrouping();
}
