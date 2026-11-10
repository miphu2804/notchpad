//! Synced lyrics for the Playing tab, looked up on lrclib.net (free, no key) only while the
//! "Synced lyrics" setting is on. One lookup per song, on a background thread through `curl`;
//! the answer is kept until another song plays.

use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use objc2::msg_send;
use objc2::runtime::{AnyObject, Sel};
use serde_json::Value;

const LOOKUP: &str = "https://lrclib.net/api/get";
/// Lines are shown this much early, so a line is up by the time it is sung.
const LEAD_SECS: f64 = 0.3;

/// A line of synced lyrics: when it starts, in seconds, and its text (empty for a break).
pub type Line = (f64, String);

#[derive(Default)]
pub struct Lyrics {
    /// `artist\ttitle` of the song the lines belong to, or being looked up.
    song: String,
    lines: Vec<Line>,
    pending: Option<Receiver<Vec<Line>>>,
}

impl Lyrics {
    /// Starts a lookup when `artist` and `title` name another song than the last one; `target`
    /// gets `selector` on the main thread when it finishes.
    pub fn want(&mut self, artist: &str, title: &str, duration: f64, target: &AnyObject, selector: Sel) {
        let song = format!("{artist}\t{title}");
        if song == self.song {
            return;
        }
        self.song = song;
        self.lines.clear();
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let (artist, title) = (artist.to_owned(), title.to_owned());
        let target = target as *const AnyObject as usize;
        thread::spawn(move || {
            let lines = lookup(&artist, &title, duration).unwrap_or_default();
            if sender.send(lines).is_ok() {
                let target = target as *const AnyObject;
                let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
            }
        });
    }

    /// Takes a finished lookup; false while it is still running. A lookup for a song that has
    /// since changed is dropped, because `want` replaced its receiver.
    pub fn take(&mut self) -> bool {
        let Some(lines) = self.pending.as_ref().and_then(|r| r.try_recv().ok()) else { return false };
        self.pending = None;
        self.lines = lines;
        true
    }

    /// The line before, at and after `position` seconds; `None` when the song has no synced lyrics.
    pub fn around(&self, position: f64) -> Option<[&str; 3]> {
        let now = current(&self.lines, position + LEAD_SECS)?;
        let text = |i: Option<usize>| i.and_then(|i| self.lines.get(i)).map_or("", |(_, t)| t.as_str());
        Some([text(now.checked_sub(1)), text(Some(now)), text(Some(now + 1))])
    }

    pub fn has_lines(&self) -> bool {
        !self.lines.is_empty()
    }
}

/// Index of the line playing at `position`: the last one started, or the first before it starts.
fn current(lines: &[Line], position: f64) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    Some(lines.iter().rposition(|(at, _)| *at <= position).unwrap_or(0))
}

/// Synced lines for the song, or `None` when lrclib has none (not found, plain lyrics only,
/// instrumental) or the network fails.
fn lookup(artist: &str, title: &str, duration: f64) -> Option<Vec<Line>> {
    let output = Command::new("curl")
        .args(["--silent", "--max-time", "10", "--get", LOOKUP, "--user-agent", "NotchGo (https://github.com/miphu2804/notchpad)"])
        .args(["--data-urlencode", &format!("artist_name={artist}"), "--data-urlencode", &format!("track_name={title}")])
        .args(["--data-urlencode", &format!("duration={}", duration.round() as u64)])
        .output()
        .ok()?;
    let body: Value = serde_json::from_slice(&output.stdout).ok()?;
    let lines = parse_lrc(body.get("syncedLyrics")?.as_str()?);
    (!lines.is_empty()).then_some(lines)
}

/// Lines of LRC text such as `[01:02.35] words`, in time order; lines without a time are skipped.
fn parse_lrc(text: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = text
        .lines()
        .filter_map(|line| {
            let (stamp, words) = line.strip_prefix('[')?.split_once(']')?;
            let (minutes, seconds) = stamp.split_once(':')?;
            let at = minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?;
            Some((at, words.trim().to_owned()))
        })
        .collect();
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const LRC: &str = "[ar: Someone]\n[00:12.00] First line\n[00:15.50]Second line\n[00:20.00]\n[01:02.25] Last line\n";

    #[test]
    fn parses_timed_lines_and_skips_tags() {
        let lines = parse_lrc(LRC);
        let times: Vec<f64> = lines.iter().map(|l| l.0).collect();
        assert_eq!(times, [12.0, 15.5, 20.0, 62.25]);
        assert_eq!((lines[0].1.as_str(), lines[2].1.as_str()), ("First line", ""));
    }

    #[test]
    fn picks_the_line_being_sung_with_its_neighbours() {
        let lyrics = Lyrics { lines: parse_lrc(LRC), ..Lyrics::default() };
        assert_eq!(lyrics.around(0.0), Some(["", "First line", "Second line"]));
        assert_eq!(lyrics.around(15.3), Some(["First line", "Second line", ""]));
        assert_eq!(lyrics.around(100.0), Some(["", "Last line", ""]));
        assert_eq!(Lyrics::default().around(10.0), None);
    }
}
