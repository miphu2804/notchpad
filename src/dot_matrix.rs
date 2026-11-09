//! The dot-matrix countdown beside the collapsed notch: `mm:ss` in 5×7 LED dots with a ripple
//! running through them like light on water. Pure, so the wing view only sets each dot's opacity.

use std::f64::consts::TAU;

pub const ROWS: usize = 7;
/// Five glyphs of `mm:ss`: four 5-dot digits, a 1-dot colon and a gap between each.
pub const COLS: usize = 4 * 5 + 1 + 4;
/// Frames after which the ripple repeats (6 s at 10 fps); every wave makes whole cycles per loop.
const LOOP: u64 = 60;

/// Each digit's rows, top first, as the low five bits with the leftmost dot highest.
const DIGITS: [[u8; ROWS]; 10] = [
    [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

/// Opacity of every dot, rows top first, for `text` (`mm:ss`) at animation `frame`. Lit dots
/// shimmer between 0.55 and 1; the unlit ones stay a faint grid that the ripple also crosses.
pub fn frame(text: &str, frame: u64) -> [[f64; COLS]; ROWS] {
    let lit = lit(text);
    let t = (frame % LOOP) as f64 / LOOP as f64;
    let mut out = [[0.0; COLS]; ROWS];
    for (y, row) in out.iter_mut().enumerate() {
        for (x, dot) in row.iter_mut().enumerate() {
            let (xf, yf) = (x as f64, y as f64);
            // A long swell rolling right and a short ripple crossing it diagonally.
            let swell = (TAU * (xf / 10.0 - 2.0 * t) + yf * 0.5).sin();
            let ripple = (TAU * (xf / 6.0 + yf / 7.0 + t)).sin();
            let wave = 0.5 + 0.5 * (0.7 * swell + 0.3 * ripple);
            *dot = if lit[y][x] { 0.55 + 0.45 * wave } else { 0.05 + 0.08 * wave };
        }
    }
    out
}

/// Which dots `text` lights, left-aligned with one dark column between glyphs.
fn lit(text: &str) -> [[bool; COLS]; ROWS] {
    let mut lit = [[false; COLS]; ROWS];
    let mut x = 0;
    for c in text.chars() {
        let (rows, width) = match c {
            ':' => ([0, 0, 1, 0, 1, 0, 0], 1),
            _ => (c.to_digit(10).map_or([0; ROWS], |d| DIGITS[d as usize]), 5),
        };
        for (y, bits) in rows.iter().enumerate() {
            for dx in 0..width {
                if x + dx < COLS && bits >> (width - 1 - dx) & 1 == 1 {
                    lit[y][x + dx] = true;
                }
            }
        }
        x += width + 1;
    }
    lit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(text: &str) -> Vec<String> {
        lit(text).iter().map(|row| row.iter().map(|&on| if on { '#' } else { '.' }).collect()).collect()
    }

    #[test]
    fn mm_ss_fills_the_grid_exactly() {
        let rows = picture("44:57");
        assert_eq!(rows[0], "...#.....#....#####.#####");
        assert!(rows.iter().all(|r| r.chars().count() == COLS));
        // The colon is the 13th column, dots on the 3rd and 5th rows only.
        let colon: String = rows.iter().map(|r| r.chars().nth(12).unwrap()).collect();
        assert_eq!(colon, "..#.#..");
    }

    #[test]
    fn lit_dots_stay_brighter_than_unlit_ones_in_every_frame() {
        let on = lit("08:30");
        for f in 0..LOOP {
            let dots = frame("08:30", f);
            let dim = (0..ROWS).flat_map(|y| (0..COLS).map(move |x| (x, y))).filter(|&(x, y)| !on[y][x]).map(|(x, y)| dots[y][x]).fold(0.0, f64::max);
            let bright = (0..ROWS).flat_map(|y| (0..COLS).map(move |x| (x, y))).filter(|&(x, y)| on[y][x]).map(|(x, y)| dots[y][x]).fold(1.0, f64::min);
            assert!(dim < bright, "frame {f}: {dim} vs {bright}");
        }
    }

    #[test]
    fn the_ripple_loops_and_keeps_moving() {
        assert_eq!(frame("12:00", 5), frame("12:00", 5 + LOOP));
        assert!((0..LOOP).all(|f| frame("12:00", f) != frame("12:00", f + 1)));
    }
}
