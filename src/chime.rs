//! The two-note bell that marks the end of a focus session or a break, or a task reminder. It is synthesised
//! into an in-memory WAV, so the app ships no sound files.

use std::f64::consts::TAU;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::NSSound;
use objc2_foundation::NSData;

const RATE: u32 = 44_100;
const LENGTH_S: f64 = 1.9;
const ATTACK_S: f64 = 0.012;
/// (frequency ratio, peak level, decay seconds). A sine plus two inharmonic partials that
/// die away sooner is what makes the tone ring like a small bell rather than beep.
const PARTIALS: [(f64, f64, f64); 3] = [(1.0, 0.2, 1.6), (2.76, 0.05, 0.6), (5.4, 0.018, 0.6)];

#[derive(Clone, Copy)]
pub enum Chime {
    /// Rising D5 → A5: the session is done.
    FocusEnd,
    /// Falling G5 → D5: the break is over, time to settle back in.
    BreakEnd,
    /// A5 twice: a timed task is coming up or due.
    Reminder,
}

impl Chime {
    /// (frequency in Hz, start in seconds) of the two notes.
    fn notes(self) -> [(f64, f64); 2] {
        match self {
            Chime::FocusEnd => [(587.33, 0.0), (880.0, 0.2)],
            Chime::BreakEnd => [(783.99, 0.0), (587.33, 0.2)],
            Chime::Reminder => [(880.0, 0.0), (880.0, 0.18)],
        }
    }

    /// A ready-to-play sound. Keep it alive until it has finished: a released `NSSound` stops.
    pub fn sound(self) -> Option<Retained<NSSound>> {
        let data = NSData::with_bytes(&wav(&samples(self.notes())));
        NSSound::initWithData(NSSound::alloc(), &data)
    }
}

/// Mono 16-bit samples: each partial rises over `ATTACK_S`, then decays exponentially to
/// silence (1/10000 of its peak) at its decay time.
fn samples(notes: [(f64, f64); 2]) -> Vec<i16> {
    let rate = f64::from(RATE);
    let mut mix = vec![0.0; (LENGTH_S * rate) as usize];
    for (freq, start) in notes {
        let first = (start * rate) as usize;
        for (ratio, level, decay) in PARTIALS {
            for (i, out) in mix.iter_mut().enumerate().skip(first) {
                let t = (i - first) as f64 / rate;
                if t > decay {
                    break;
                }
                let envelope = if t < ATTACK_S { level * t / ATTACK_S } else { level * 1e-4_f64.powf((t - ATTACK_S) / (decay - ATTACK_S)) };
                *out += envelope * (TAU * freq * ratio * t).sin();
            }
        }
    }
    mix.iter().map(|v| (v.clamp(-1.0, 1.0) * f64::from(i16::MAX)) as i16).collect()
}

/// A canonical 44-byte RIFF header followed by the little-endian samples.
fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bell_starts_silent_rings_and_fades_out() {
        let s = samples(Chime::FocusEnd.notes());
        assert_eq!(s.len(), (LENGTH_S * f64::from(RATE)) as usize);
        assert_eq!(s[0], 0);
        let peak = s.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak > 5_000 && peak < i16::MAX as u16, "peak {peak}");
        assert!(s[s.len() - 2_000..].iter().all(|v| v.unsigned_abs() < 50));
    }

    #[test]
    fn wav_header_describes_the_samples() {
        let bytes = wav(&[0, 1, -1]);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), RATE);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(&bytes[46..48], &1i16.to_le_bytes());
    }
}
