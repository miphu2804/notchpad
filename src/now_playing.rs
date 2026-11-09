//! What the system is playing, learned from MediaRemote like the menu-bar Now Playing widget.
//!
//! macOS 15.4+ serves MediaRemote only to Apple-signed processes, so a small helper dylib
//! (`macos/now_playing_helper.m`) is loaded into `/usr/bin/perl`. It prints one JSON line per
//! change and takes control words on stdin; it exits when notchpad (its stdin) goes away.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;
use std::{fs, thread};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::msg_send;
use objc2_app_kit::{NSImage, NSWorkspace};
use objc2_foundation::{NSData, NSDataBase64DecodingOptions, NSString};
use serde_json::Value;

const HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/now_playing_helper.dylib"));
/// Loads the dylib, installs its entry point as a perl sub and runs it.
const LOADER: &str = r#"use DynaLoader; my $h = DynaLoader::dl_load_file($ARGV[0], 0) or die "load";
my $s = DynaLoader::dl_find_symbol($h, "np_run") or die "symbol";
DynaLoader::dl_install_xsub("main::np", $s); np();"#;

#[derive(Clone, Copy)]
pub enum Command {
    Previous,
    PlayPause,
    Next,
}

pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    /// Elapsed seconds as of `at`; read it through [`Track::position`].
    elapsed: f64,
    at: Instant,
    pub playing: bool,
    pub bundle: String,
    pub artwork: Option<Retained<NSImage>>,
}

impl Track {
    /// Extrapolated, so a long-collapsed panel still shows the right time without ticking.
    pub fn position(&self) -> f64 {
        let moved = if self.playing { self.at.elapsed().as_secs_f64() } else { 0.0 };
        (self.elapsed + moved).min(self.duration.max(self.elapsed))
    }

    fn same_as(&self, other: &Track) -> bool {
        self.bundle == other.bundle && self.title == other.title && self.artist == other.artist && self.album == other.album
    }
}

/// One helper line, still free of AppKit objects so the reader thread can own it.
#[derive(Debug, PartialEq)]
pub struct Update {
    title: String,
    artist: String,
    album: String,
    duration: f64,
    position: f64,
    playing: bool,
    bundle: String,
    artwork: String,
}

/// `None` for an empty object (nothing is playing) or malformed text.
pub fn parse_line(line: &str) -> Option<Update> {
    let json: Value = serde_json::from_str(line).ok()?;
    let text = |key: &str| json[key].as_str().unwrap_or_default().to_string();
    let number = |key: &str| json[key].as_f64().unwrap_or(0.0);
    Some(Update {
        title: json["title"].as_str()?.to_string(),
        artist: text("artist"),
        album: text("album"),
        duration: number("duration"),
        position: number("position"),
        playing: json["playing"].as_bool().unwrap_or(false),
        bundle: text("bundle"),
        artwork: text("artwork"),
    })
}

/// The current track, or `None` when nothing is playing or paused.
#[derive(Default)]
pub struct NowPlaying {
    pub track: Option<Track>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    updates: Option<Receiver<Option<Update>>>,
}

impl NowPlaying {
    /// Spawns the helper once. Each line it prints is queued and `target` is told to call
    /// `selector` on the main thread, so nothing polls while nothing plays.
    pub fn start(&mut self, target: &AnyObject, selector: Sel) {
        if self.child.is_some() {
            return;
        }
        let Some(mut child) = spawn_helper() else { return };
        let (Some(stdout), stdin) = (child.stdout.take(), child.stdin.take()) else { return };
        let (sender, receiver) = mpsc::channel();
        // Raw address: the controller lives for the process, and performSelector is thread-safe.
        let target = target as *const AnyObject as usize;
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(parse_line(&line)).is_err() {
                    break;
                }
                let target = target as *const AnyObject;
                let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
            }
        });
        self.child = Some(child);
        self.stdin = stdin;
        self.updates = Some(receiver);
    }

    /// Applies every queued helper line.
    pub fn drain(&mut self) {
        let updates: Vec<_> = self.updates.iter().flat_map(|r| r.try_iter()).collect();
        for update in updates {
            self.apply(update);
        }
    }

    fn apply(&mut self, update: Option<Update>) {
        let Some(update) = update else {
            self.track = None;
            return;
        };
        let mut track = Track {
            title: update.title,
            artist: update.artist,
            album: update.album,
            duration: update.duration,
            elapsed: update.position,
            at: Instant::now(),
            playing: update.playing,
            bundle: update.bundle,
            artwork: decode(&update.artwork),
        };
        // MediaRemote briefly drops the artwork around state changes; keep the last one.
        if track.artwork.is_none() {
            match self.track.take() {
                Some(old) if old.same_as(&track) && old.artwork.is_some() => track.artwork = old.artwork,
                _ => track.artwork = app_icon(&track.bundle),
            }
        }
        self.track = Some(track);
    }

    pub fn playing(&self) -> bool {
        self.track.as_ref().is_some_and(|t| t.playing)
    }

    /// The helper reports the resulting state change, which updates the track.
    pub fn send(&mut self, command: Command) {
        let word = match command {
            Command::Previous => "previous",
            Command::PlayPause => "toggle",
            Command::Next => "next",
        };
        if let Some(stdin) = &mut self.stdin {
            let _ = writeln!(stdin, "{word}").and_then(|()| stdin.flush());
        }
    }

    pub fn stop(&mut self) {
        self.stdin = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Writes the embedded dylib beside the app's data (perl can only load a file) and runs perl on it.
fn spawn_helper() -> Option<Child> {
    let path = crate::store::path().with_file_name("now_playing_helper.dylib");
    if fs::read(&path).ok().as_deref() != Some(HELPER) {
        fs::create_dir_all(path.parent()?).ok()?;
        fs::write(&path, HELPER).ok()?;
    }
    Process::new("/usr/bin/perl")
        .args(["-e", LOADER])
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| eprintln!("notchpad: now playing helper failed: {e}"))
        .ok()
}

fn decode(base64: &str) -> Option<Retained<NSImage>> {
    let data = NSData::initWithBase64EncodedString_options(NSData::alloc(), &NSString::from_str(base64), NSDataBase64DecodingOptions::empty())?;
    NSImage::initWithData(NSImage::alloc(), &data)
}

/// Fallback when the player supplies no artwork.
fn app_icon(bundle: &str) -> Option<Retained<NSImage>> {
    let workspace = NSWorkspace::sharedWorkspace();
    let url = workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle))?;
    Some(workspace.iconForFile(&*url.path()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_helper_line() {
        let update = parse_line(r#"{"title":"Song","artist":"Band","album":"","duration":215.5,"position":3,"playing":true,"bundle":"com.google.Chrome","artwork":"AA=="}"#).unwrap();
        assert_eq!((update.title.as_str(), update.artist.as_str(), update.bundle.as_str()), ("Song", "Band", "com.google.Chrome"));
        assert_eq!((update.duration, update.position, update.playing), (215.5, 3.0, true));
        assert_eq!(update.artwork, "AA==");
    }

    #[test]
    fn missing_optional_fields_default() {
        let update = parse_line(r#"{"title":"Only title"}"#).unwrap();
        assert_eq!((update.artist.as_str(), update.duration, update.playing), ("", 0.0, false));
    }

    #[test]
    fn empty_object_or_garbage_has_no_track() {
        assert_eq!(parse_line("{}"), None);
        assert_eq!(parse_line("not json"), None);
    }

    #[test]
    fn paused_position_does_not_move_and_playing_is_capped() {
        let track = |playing| Track {
            title: String::new(), artist: String::new(), album: String::new(), duration: 10.0,
            elapsed: 9.99, at: Instant::now() - std::time::Duration::from_secs(5), playing,
            bundle: String::new(), artwork: None,
        };
        assert_eq!(track(false).position(), 9.99);
        assert_eq!(track(true).position(), 10.0);
    }
}
