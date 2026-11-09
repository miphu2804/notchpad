//! Maps Mac key presses to the bytes a terminal program expects on its stdin.

use std::borrow::Cow;

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Keys whose bytes do not come from the characters they type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Special {
    Return,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    F(u8),
}

/// Virtual key codes are layout independent, unlike the characters they produce.
pub fn special(key_code: u16) -> Option<Special> {
    Some(match key_code {
        36 | 76 => Special::Return,
        48 => Special::Tab,
        51 => Special::Backspace,
        53 => Special::Escape,
        126 => Special::Up,
        125 => Special::Down,
        124 => Special::Right,
        123 => Special::Left,
        115 => Special::Home,
        119 => Special::End,
        116 => Special::PageUp,
        121 => Special::PageDown,
        117 => Special::Delete,
        122 => Special::F(1),
        120 => Special::F(2),
        99 => Special::F(3),
        118 => Special::F(4),
        96 => Special::F(5),
        97 => Special::F(6),
        98 => Special::F(7),
        100 => Special::F(8),
        101 => Special::F(9),
        109 => Special::F(10),
        103 => Special::F(11),
        111 => Special::F(12),
        _ => return None,
    })
}

pub fn special_bytes(key: Special, mods: Mods, app_cursor: bool) -> Vec<u8> {
    // xterm's modifier parameter: 1 plus shift, alt and ctrl as bits 1, 2 and 4.
    let param = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let modified = param > 1;
    let cursor_key = |letter: char| match (modified, app_cursor) {
        (true, _) => format!("\x1b[1;{param}{letter}"),
        (false, true) => format!("\x1bO{letter}"),
        (false, false) => format!("\x1b[{letter}"),
    };
    let tilde = |n: u8| if modified { format!("\x1b[{n};{param}~") } else { format!("\x1b[{n}~") };
    // Option on a plain key means meta, which is an ESC prefix.
    let meta = |bytes: &str| if mods.alt { format!("\x1b{bytes}") } else { bytes.to_owned() };
    let text = match key {
        Special::Return => meta("\r"),
        Special::Tab if mods.shift => "\x1b[Z".to_owned(),
        Special::Tab => meta("\t"),
        Special::Backspace => meta("\x7f"),
        Special::Escape => "\x1b".to_owned(),
        Special::Up => cursor_key('A'),
        Special::Down => cursor_key('B'),
        Special::Right => cursor_key('C'),
        Special::Left => cursor_key('D'),
        Special::Home => cursor_key('H'),
        Special::End => cursor_key('F'),
        Special::PageUp => tilde(5),
        Special::PageDown => tilde(6),
        Special::Delete => tilde(3),
        Special::F(n @ 1..=4) => {
            let letter = ['P', 'Q', 'R', 'S'][usize::from(n) - 1];
            if modified { format!("\x1b[1;{param}{letter}") } else { format!("\x1bO{letter}") }
        }
        Special::F(n) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n.clamp(5, 12)) - 5]),
    };
    text.into_bytes()
}

/// The control byte for Ctrl+`c`, as typed on a US layout.
pub fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' => Some(27),
        '\\' => Some(28),
        ']' => Some(29),
        '^' | '6' => Some(30),
        '_' => Some(31),
        '?' => Some(127),
        _ => None,
    }
}

/// Option+key: the key's own character behind an ESC.
pub fn meta(text: &str) -> Vec<u8> {
    [b"\x1b".as_slice(), text.as_bytes()].concat()
}

/// Pasted text. Bracketed mode lets the program tell a paste from typing, so ESC is
/// stripped to stop the text from ending the bracket early; otherwise newlines become
/// the Return a keyboard would send.
pub fn paste(text: &str, bracketed: bool) -> Cow<'static, [u8]> {
    let bytes = if bracketed {
        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', ""))
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r")
    };
    Cow::Owned(bytes.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(key: Special, mods: Mods, app_cursor: bool) -> Vec<u8> {
        special_bytes(key, mods, app_cursor)
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(bytes(Special::Up, Mods::default(), false), b"\x1b[A");
        assert_eq!(bytes(Special::Up, Mods::default(), true), b"\x1bOA");
        assert_eq!(bytes(Special::Left, Mods::default(), true), b"\x1bOD");
        assert_eq!(bytes(Special::Home, Mods::default(), false), b"\x1b[H");
    }

    #[test]
    fn modified_arrows_use_csi_even_in_app_mode() {
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        assert_eq!(bytes(Special::Right, ctrl, true), b"\x1b[1;5C");
        let alt = Mods { alt: true, ..Mods::default() };
        assert_eq!(bytes(Special::Left, alt, false), b"\x1b[1;3D");
    }

    #[test]
    fn editing_keys() {
        let none = Mods::default();
        assert_eq!(bytes(Special::Return, none, false), b"\r");
        assert_eq!(bytes(Special::Backspace, none, false), b"\x7f");
        assert_eq!(bytes(Special::Escape, none, false), b"\x1b");
        assert_eq!(bytes(Special::Tab, Mods { shift: true, ..none }, false), b"\x1b[Z");
        assert_eq!(bytes(Special::PageUp, none, false), b"\x1b[5~");
        assert_eq!(bytes(Special::Delete, none, false), b"\x1b[3~");
        assert_eq!(bytes(Special::Backspace, Mods { alt: true, ..none }, false), b"\x1b\x7f");
    }

    #[test]
    fn function_keys() {
        let none = Mods::default();
        assert_eq!(bytes(Special::F(1), none, false), b"\x1bOP");
        assert_eq!(bytes(Special::F(5), none, false), b"\x1b[15~");
        assert_eq!(bytes(Special::F(12), none, false), b"\x1b[24~");
    }

    #[test]
    fn control_letters() {
        assert_eq!(ctrl_byte('c'), Some(3));
        assert_eq!(ctrl_byte('D'), Some(4));
        assert_eq!(ctrl_byte('['), Some(27));
        assert_eq!(ctrl_byte('1'), None);
    }

    #[test]
    fn option_is_an_escape_prefix() {
        assert_eq!(meta("b"), b"\x1bb");
    }

    #[test]
    fn paste_wrapping() {
        assert_eq!(&*paste("a\nb", false), b"a\rb");
        assert_eq!(&*paste("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(&*paste("x\x1b[201~y", true), b"\x1b[200~x[201~y\x1b[201~");
    }

    #[test]
    fn key_codes() {
        assert_eq!(special(36), Some(Special::Return));
        assert_eq!(special(126), Some(Special::Up));
        assert_eq!(special(0), None);
    }
}
