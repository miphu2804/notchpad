//! Three lines of synced lyrics, the way music apps show them: the line before dimmed, the line
//! being sung bright, the next one dimmed. When the sung line changes the block slides up one
//! line, so the words appear to scroll with the song.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSFontWeightRegular, NSFontWeightSemibold, NSLineBreakMode, NSStackView, NSTextField};
use objc2_foundation::NSString;
use objc2_quartz_core::{CAMediaTiming, CAMediaTimingFunction, CATransition, kCAMediaTimingFunctionEaseInEaseOut, kCATransitionFromTop, kCATransitionPush};

use crate::widgets as w;

/// A break between lines (an empty LRC line) shows this instead of nothing.
const BREAK: &str = "♪";
const SLIDE_SECS: f64 = 0.35;

pub struct LyricsView {
    /// Put this in the layout; hidden while there are no lyrics.
    pub view: Retained<NSStackView>,
    lines: [Retained<NSTextField>; 3],
    /// The line shown as being sung, to slide only when it changes.
    sung: RefCell<String>,
}

impl LyricsView {
    pub fn new(width: f64, mtm: MainThreadMarker) -> Self {
        let line = |size: f64, weight, alpha: f64| {
            let label = w::label("", size, weight, &w::rgba(255, 255, 255, alpha), mtm);
            label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            label.widthAnchor().constraintEqualToConstant(width).setActive(true);
            label
        };
        let (regular, semibold) = unsafe { (NSFontWeightRegular, NSFontWeightSemibold) };
        let lines = [line(13.0, regular, 0.35), line(17.0, semibold, 0.95), line(13.0, regular, 0.35)];
        let view = w::stack(&[&lines[0], &lines[1], &lines[2]], true, 4.0, mtm);
        view.setWantsLayer(true);
        view.setHidden(true);
        Self { view, lines, sung: RefCell::new(String::new()) }
    }

    /// Shows `[before, sung, after]`, or hides the block for `None`.
    pub fn show(&self, lines: Option<[&str; 3]>) {
        let Some(lines) = lines else {
            self.view.setHidden(true);
            self.sung.borrow_mut().clear();
            return;
        };
        self.view.setHidden(false);
        if *self.sung.borrow() == lines[1] {
            return;
        }
        *self.sung.borrow_mut() = lines[1].to_owned();
        if !w::reduce_motion() {
            self.slide_up();
        }
        for (k, (label, text)) in self.lines.iter().zip(lines).enumerate() {
            label.setStringValue(&NSString::from_str(if k == 1 && text.is_empty() { BREAK } else { text }));
        }
    }

    /// The next drawn state pushes in from below, moving the old lines up.
    fn slide_up(&self) {
        let Some(layer) = self.view.layer() else { return };
        let push = CATransition::new();
        push.setType(unsafe { kCATransitionPush });
        // Layers on macOS are not flipped, so "from top" in layer terms enters from the bottom on screen.
        push.setSubtype(Some(unsafe { kCATransitionFromTop }));
        push.setDuration(SLIDE_SECS);
        push.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseInEaseOut })));
        layer.addAnimation_forKey(&push, None);
    }
}
