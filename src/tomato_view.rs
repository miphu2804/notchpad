//! One pixel-art tomato, drawn cell by cell so it stays crisp at any size.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSColor, NSRectFill, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::widgets::rgba;

/// `.` empty, `s` stem and leaf, `b` body, `h` highlight, `d` shade.
const SPRITE: [&str; 11] = [
    "....sss....",
    "..ss.s.ss..",
    ".bbbbbbbbb.",
    "bhhbbbbbbbd",
    "bhbbbbbbbbd",
    "bbbbbbbbbbd",
    "bbbbbbbbbdd",
    "bbbbbbbbbdd",
    ".bbbbbbbdd.",
    "..bbbbbdd..",
    "....ddd....",
];

#[derive(Clone, Copy, PartialEq)]
pub enum Tomato {
    Ripe,
    /// The current session, ripening from green (0.0) to red (1.0) as it runs.
    Growing(f64),
    Empty,
}

/// Body, highlight and shade at three points of ripening; in between they blend, passing
/// through orange so the colour doesn't go muddy brown on the way from green to red.
const GREEN: [[u8; 3]; 3] = [[98, 176, 74], [150, 214, 118], [56, 112, 48]];
const ORANGE: [[u8; 3]; 3] = [[232, 142, 58], [250, 188, 112], [170, 92, 38]];
const RED: [[u8; 3]; 3] = [[214, 62, 52], [244, 122, 104], [150, 36, 34]];

impl Tomato {
    /// (stem, body, highlight, shade)
    fn palette(self) -> [Retained<NSColor>; 4] {
        let stem = rgba(86, 150, 64, 1.0);
        let [body, light, shade] = match self {
            Tomato::Ripe => RED,
            Tomato::Growing(ripeness) => ripen(ripeness),
            Tomato::Empty => return [rgba(62, 96, 54, 1.0), rgba(74, 72, 78, 1.0), rgba(90, 88, 94, 1.0), rgba(58, 56, 62, 1.0)],
        }
        .map(|[r, g, b]| rgba(r, g, b, 1.0));
        [stem, body, light, shade]
    }
}

fn ripen(ripeness: f64) -> [[u8; 3]; 3] {
    let t = ripeness.clamp(0.0, 1.0);
    let (from, to, k) = if t < 0.5 { (GREEN, ORANGE, t * 2.0) } else { (ORANGE, RED, t * 2.0 - 1.0) };
    std::array::from_fn(|i| std::array::from_fn(|c| (f64::from(from[i][c]) + (f64::from(to[i][c]) - f64::from(from[i][c])) * k).round() as u8))
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<Tomato>]
    #[name = "TomatoView"]
    pub struct TomatoView;

    impl TomatoView {
        // Row 0 of the sprite is the top row.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let size = self.bounds().size;
            // Whole-pixel cells keep the edges hard; leftover space is split around the sprite.
            let cell = (size.width.min(size.height) / SPRITE.len() as f64).floor();
            let side = cell * SPRITE.len() as f64;
            let (x0, y0) = (((size.width - side) / 2.0).floor(), ((size.height - side) / 2.0).floor());
            let [stem, body, light, shade] = self.ivars().get().palette();
            for (row, line) in SPRITE.iter().enumerate() {
                for (col, px) in line.bytes().enumerate() {
                    let color = match px {
                        b's' => &stem,
                        b'b' => &body,
                        b'h' => &light,
                        b'd' => &shade,
                        _ => continue,
                    };
                    color.setFill();
                    NSRectFill(NSRect::new(
                        NSPoint::new(x0 + col as f64 * cell, y0 + row as f64 * cell),
                        NSSize::new(cell, cell),
                    ));
                }
            }
        }
    }
);

impl TomatoView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Cell::new(Tomato::Empty));
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    pub fn set(&self, state: Tomato) {
        if self.ivars().replace(state) != state {
            self.setNeedsDisplay(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ripening_runs_green_through_orange_to_red() {
        assert_eq!(ripen(0.0), GREEN);
        assert_eq!(ripen(0.5), ORANGE);
        assert_eq!(ripen(1.0), RED);
        assert_eq!(ripen(-1.0), GREEN);
        assert_eq!(ripen(2.0), RED);
        assert_eq!(ripen(0.25)[0], [165, 159, 66]);
    }
}
