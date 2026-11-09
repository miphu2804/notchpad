//! Terminal colors tuned to the notch's dark card, indexed like alacritty's color table.

use alacritty_terminal::vte::ansi::Rgb;

pub const FOREGROUND: Rgb = Rgb { r: 224, g: 224, b: 226 };
/// Equals the Terminal card's fill, so default-background cells need no painting.
pub const BACKGROUND: Rgb = Rgb { r: 42, g: 42, b: 45 };
pub const CURSOR: Rgb = Rgb { r: 230, g: 230, b: 232 };

const fn c(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

const ANSI: [Rgb; 16] = [
    c(72, 72, 78), c(255, 105, 97), c(120, 200, 120), c(240, 200, 100),
    c(100, 160, 255), c(200, 130, 230), c(90, 200, 210), c(200, 200, 205),
    c(110, 110, 118), c(255, 135, 125), c(150, 225, 140), c(255, 220, 130),
    c(130, 185, 255), c(225, 160, 250), c(120, 225, 230), c(245, 245, 247),
];

/// Color for a table index: 0-15 ANSI, 16-255 the xterm cube and gray ramp, then the
/// named foreground, background, cursor and dim variants.
pub fn rgb(index: usize) -> Rgb {
    match index {
        0..16 => ANSI[index],
        16..232 => {
            let n = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
            c(level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        232..=255 => {
            let gray = (8 + 10 * (index - 232)) as u8;
            c(gray, gray, gray)
        }
        257 => BACKGROUND,
        258 => CURSOR,
        259..267 => {
            let Rgb { r, g, b } = ANSI[index - 259];
            c(r / 3 * 2, g / 3 * 2, b / 3 * 2)
        }
        _ => FOREGROUND,
    }
}
