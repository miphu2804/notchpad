//! The ASCII timer style as plain text: a big countdown, a progress bar and a looping scene.
//! Pure, so the controller only has to put the string in a monospaced label.

use std::f64::consts::TAU;

/// Characters per line; every line of [`frame`] is padded to this, so the label never reflows.
pub const WIDTH: usize = 26;
/// Frames after which the animation repeats (6 s at 10 fps). Every moving part below makes a
/// whole number of cycles per loop, so the wrap is invisible.
const LOOP: u64 = 60;
const BAR: usize = 21;

/// Heavy box-drawing digits, three rows of three columns.
const DIGITS: [[&str; 3]; 10] = [
    ["┏━┓", "┃ ┃", "┗━┛"],
    [" ╻ ", " ┃ ", " ╹ "],
    ["╺━┓", "┏━┛", "┗━╸"],
    ["╺━┓", "╺━┫", "╺━┛"],
    ["╻ ╻", "┗━┫", "  ╹"],
    ["┏━╸", "┗━┓", "╺━┛"],
    ["┏━╸", "┣━┓", "┗━┛"],
    ["╺━┓", "  ┃", "  ╹"],
    ["┏━┓", "┣━┫", "┗━┛"],
    ["┏━┓", "┗━┫", "╺━┛"],
];
const COLON: [&str; 3] = ["▪", " ", "▪"];
const WAVE: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Left-aligned partial blocks, one eighth to seven eighths of a cell.
const PARTIAL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const SPINNER_FOCUS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const SPINNER_BREAK: [char; 6] = ['◜', '◠', '◝', '◞', '◡', '◟'];
/// Break scene: (glyph, start column, laps per loop). Different speeds so they drift apart.
const BUBBLES: [(char, usize, u64); 6] = [('∘', 0, 1), ('·', 5, 2), ('○', 9, 1), ('∙', 14, 2), ('°', 18, 1), ('·', 22, 2)];

/// Eight lines for a session with `remaining_secs` of `total_secs` left. `total_secs` of 0 (the
/// stopwatch) leaves the bar empty. Minutes show at most 99.
pub fn frame(remaining_secs: u64, total_secs: u64, frame: u64, on_break: bool) -> String {
    let f = frame % LOOP;
    let elapsed = total_secs.saturating_sub(remaining_secs);
    let fraction = if total_secs == 0 { 0.0 } else { (elapsed as f64 / total_secs as f64).min(1.0) };
    let (scene, spinner, label) = if on_break {
        (bubbles(f), SPINNER_BREAK[f as usize / 2 % SPINNER_BREAK.len()], "break")
    } else {
        (wave(f), SPINNER_FOCUS[f as usize % SPINNER_FOCUS.len()], "focus")
    };
    let digits = big_clock(remaining_secs);
    let mut lines: Vec<String> = digits.iter().map(|row| center(row)).collect();
    lines.extend([center(""), bar(fraction), center(""), center(&scene), center(&format!("{spinner} {label}"))]);
    lines.join("\n")
}

/// `⠋24:13`: the collapsed notch has room for little more than the time.
pub fn wing(remaining_secs: u64, frame: u64) -> String {
    let spinner = SPINNER_FOCUS[(frame % LOOP) as usize % SPINNER_FOCUS.len()];
    format!("{spinner}{:02}:{:02}", (remaining_secs / 60).min(99), remaining_secs % 60)
}

fn big_clock(secs: u64) -> [String; 3] {
    let (m, s) = ((secs / 60).min(99) as usize, (secs % 60) as usize);
    let glyphs = [DIGITS[m / 10], DIGITS[m % 10], COLON, DIGITS[s / 10], DIGITS[s % 10]];
    std::array::from_fn(|row| glyphs.map(|g| g[row]).join(" "))
}

/// A progress bar with eighth-cell resolution, so it creeps instead of stepping, then ` 42%`.
fn bar(fraction: f64) -> String {
    let eighths = (fraction * (BAR * 8) as f64).round() as usize;
    let (full, part) = (eighths / 8, eighths % 8);
    let mut cells = "█".repeat(full);
    if full < BAR {
        cells.push(if part == 0 { '░' } else { PARTIAL[part] });
        cells.push_str(&"░".repeat(BAR - full - 1));
    }
    format!("{cells} {:>3}%", (fraction * 100.0) as u32)
}

/// A swell rolling right: two crests across the width, two cycles per loop.
fn wave(f: u64) -> String {
    (0..WIDTH)
        .map(|x| {
            let phase = TAU * (x as f64 / 13.0 - 2.0 * f as f64 / LOOP as f64);
            // A second, slower ripple keeps the crests from looking mechanical.
            let height = 0.5 + 0.35 * phase.sin() + 0.15 * (phase / 2.0 + 1.0).sin();
            WAVE[(height * 7.0).round().clamp(0.0, 7.0) as usize]
        })
        .collect()
}

/// Bubbles drifting right and wrapping round; later ones overwrite earlier ones where they meet.
fn bubbles(f: u64) -> String {
    let mut row = vec![' '; WIDTH];
    for (glyph, start, laps) in BUBBLES {
        row[(start + (f * laps * WIDTH as u64 / LOOP) as usize) % WIDTH] = glyph;
    }
    row.into_iter().collect()
}

fn center(text: &str) -> String {
    let pad = WIDTH.saturating_sub(text.chars().count());
    format!("{}{text}{}", " ".repeat(pad / 2), " ".repeat(pad - pad / 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_is_the_same_width_and_there_are_eight() {
        for (remaining, total, f, on_break) in [(1500, 1500, 0, false), (0, 1500, 7, true), (5999, 0, 59, false), (7300, 100, 33, true), (61, 1500, 12, false)] {
            let text = frame(remaining, total, f, on_break);
            assert_eq!(text.lines().count(), 8);
            for line in text.lines() {
                assert_eq!(line.chars().count(), WIDTH, "{line:?}");
            }
        }
    }

    #[test]
    fn the_clock_draws_each_digit() {
        let rows = big_clock(24 * 60 + 13);
        assert_eq!(rows[0], "╺━┓ ╻ ╻ ▪  ╻  ╺━┓");
        assert_eq!(rows[2], "┗━╸   ╹ ▪  ╹  ╺━┛");
    }

    #[test]
    fn progress_never_goes_backwards_as_time_passes() {
        let mut last = (0, 0);
        for remaining in (0..=1500).rev() {
            let line = bar(1.0 - remaining as f64 / 1500.0);
            let percent: usize = line[line.rfind(' ').unwrap() + 1..].trim_end_matches('%').parse().unwrap();
            let now = (line.matches('█').count(), percent);
            assert!(now >= last, "{line:?}");
            last = now;
        }
        assert!(bar(0.0).starts_with('░') && bar(0.0).ends_with("  0%"));
        assert!(bar(1.0).starts_with(&"█".repeat(BAR)) && bar(1.0).ends_with("100%"));
    }

    #[test]
    fn frames_loop_and_move() {
        for on_break in [false, true] {
            assert_eq!(frame(900, 1500, 0, on_break), frame(900, 1500, LOOP, on_break));
            assert_eq!(frame(900, 1500, 17, on_break), frame(900, 1500, 17 + 5 * LOOP, on_break));
            let moved = (0..LOOP).filter(|f| frame(900, 1500, *f, on_break) != frame(900, 1500, f + 1, on_break)).count();
            assert!(moved as u64 > LOOP * 3 / 4, "only {moved} of {LOOP} steps change the picture");
        }
        assert_eq!(wing(754, 3), wing(754, 3 + LOOP));
        assert_ne!(wing(754, 3), wing(754, 4));
    }

    #[test]
    fn a_break_looks_different_from_focus() {
        assert_ne!(frame(300, 300, 10, true), frame(300, 300, 10, false));
    }

    #[test]
    fn the_wing_is_compact() {
        assert_eq!(wing(24 * 60 + 13, 0), "⠋24:13");
        assert_eq!(wing(99999, 0).chars().count(), 6);
    }
}
