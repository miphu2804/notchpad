//! The Notes tab's text view: a plain `NSTextView` that reports ⌘↩ instead of typing it.

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSTextView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

pub struct NoteIvars {
    on_command_return: Box<dyn Fn()>,
}

define_class!(
    /// Key equivalents reach the first responder before `keyDown:`, which is where ⌘↩ has to
    /// be caught: `insertNewline:` is not reliably sent for a Command-modified Return.
    #[unsafe(super(NSTextView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = NoteIvars]
    #[name = "NoteView"]
    pub struct NoteView;

    impl NoteView {
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            const RETURN: u16 = 36;
            const KEYPAD_ENTER: u16 = 76;
            // Other views see key equivalents too; only the focused note may act on this one.
            let focused = self.window().and_then(|w| w.firstResponder()).is_some_and(|r| std::ptr::eq(&*r as *const _ as *const u8, self as *const _ as *const u8));
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            if focused && command && matches!(event.keyCode(), RETURN | KEYPAD_ENTER) {
                (self.ivars().on_command_return)();
                true
            } else {
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
        }
    }
);

impl NoteView {
    pub fn new(on_command_return: Box<dyn Fn()>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(NoteIvars { on_command_return });
        unsafe { msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(100.0, 100.0))] }
    }
}

/// Splits `text` around the line holding the cursor (`cursor` counts UTF-16 units, as
/// `NSRange` does). Returns the line, the text without it and its newline, and the
/// cursor position that leaves the caret where the line used to be.
pub fn take_line(text: &str, cursor: usize) -> (String, String, usize) {
    let mut utf16 = 0;
    let mut at = text.len();
    for (i, c) in text.char_indices() {
        if utf16 >= cursor {
            at = i;
            break;
        }
        utf16 += c.len_utf16();
    }
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let line = text[start..end].to_owned();
    // Drop the line's own newline, or the previous one when it is the last line.
    let (cut_start, cut_end) = if end < text.len() { (start, end + 1) } else { (start.saturating_sub(1), end) };
    let rest = format!("{}{}", &text[..cut_start], &text[cut_end..]);
    let caret = text[..cut_start].encode_utf16().count();
    (line, rest, caret)
}

#[cfg(test)]
mod tests {
    use super::take_line;

    #[test]
    fn first_line() {
        assert_eq!(take_line("one\ntwo\nthree", 2), ("one".into(), "two\nthree".into(), 0));
    }

    #[test]
    fn middle_line() {
        assert_eq!(take_line("one\ntwo\nthree", 5), ("two".into(), "one\nthree".into(), 4));
    }

    #[test]
    fn last_line_takes_the_previous_newline() {
        assert_eq!(take_line("one\ntwo\nthree", 13), ("three".into(), "one\ntwo".into(), 7));
    }

    #[test]
    fn blank_line_is_returned_empty() {
        assert_eq!(take_line("one\n\nthree", 4), ("".into(), "one\nthree".into(), 4));
    }

    #[test]
    fn cursor_at_line_end_and_start_of_next() {
        assert_eq!(take_line("one\ntwo", 3).0, "one");
        assert_eq!(take_line("one\ntwo", 4).0, "two");
    }

    #[test]
    fn single_line_and_empty_text() {
        assert_eq!(take_line("only", 2), ("only".into(), "".into(), 0));
        assert_eq!(take_line("", 0), ("".into(), "".into(), 0));
    }

    #[test]
    fn counts_utf16_units() {
        // "😀" is two UTF-16 units, so offset 3 is the start of "b".
        assert_eq!(take_line("😀\nb", 3).0, "b");
    }
}
