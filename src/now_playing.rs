//! What Music or Spotify is playing, learned from their distributed notifications, plus
//! playback control through AppleScript. Only the apps' public interfaces are used.

use std::collections::HashMap;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSImage, NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSAppleScript, NSDictionary, NSString};

pub const MUSIC_NOTIFICATION: &str = "com.apple.Music.playerInfo";
pub const SPOTIFY_NOTIFICATION: &str = "com.spotify.client.PlaybackStateChanged";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    Music,
    Spotify,
}

impl Source {
    fn app_name(self) -> &'static str {
        match self {
            Source::Music => "Music",
            Source::Spotify => "Spotify",
        }
    }

    fn bundle_id(self) -> &'static str {
        match self {
            Source::Music => "com.apple.Music",
            Source::Spotify => "com.spotify.client",
        }
    }
}

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
    pub position: f64,
    pub playing: bool,
    pub source: Source,
    pub artwork: Option<Retained<NSImage>>,
}

impl Track {
    fn same_as(&self, other: &Track) -> bool {
        self.source == other.source && self.title == other.title && self.artist == other.artist && self.album == other.album
    }
}

/// The current track, or `None` when nothing is playing or paused.
#[derive(Default)]
pub struct NowPlaying {
    pub track: Option<Track>,
}

impl NowPlaying {
    /// Applies a player notification's userInfo.
    pub fn apply(&mut self, source: Source, info: &HashMap<String, String>) {
        let Some(mut track) = parse(source, info) else {
            self.track = None;
            return;
        };
        match self.track.take() {
            Some(old) if old.same_as(&track) => track.artwork = old.artwork,
            _ => track.artwork = artwork(source),
        }
        // Music's notification carries no position, so ask it.
        if source == Source::Music {
            track.position = run(source, "get player position as integer").map_or(0.0, |d| f64::from(d.int32Value()));
        }
        self.track = Some(track);
    }

    /// Advances the position by one second while playing.
    pub fn tick(&mut self) {
        if let Some(track) = self.track.as_mut().filter(|t| t.playing) {
            track.position = (track.position + 1.0).min(track.duration);
        }
    }

    pub fn playing(&self) -> bool {
        self.track.as_ref().is_some_and(|t| t.playing)
    }

    /// Music and Spotify post a notification after the command, which updates the track.
    pub fn send(&self, command: Command) {
        let Some(track) = &self.track else { return };
        let verb = match command {
            Command::Previous => "previous track",
            Command::PlayPause => "playpause",
            Command::Next => "next track",
        };
        run(track.source, verb);
    }
}

/// userInfo keys and values as strings; numbers become their decimal text.
pub fn info_map(info: &NSDictionary<AnyObject, AnyObject>) -> HashMap<String, String> {
    let describe = |object: &AnyObject| -> String {
        let text: Retained<NSString> = unsafe { objc2::msg_send![object, description] };
        text.to_string()
    };
    info.keys()
        .filter_map(|key| {
            let value = info.objectForKey(&key)?;
            Some((describe(&key), describe(&value)))
        })
        .collect()
}

/// A track from the notification's userInfo. Music reports `Total Time` in ms, Spotify
/// `Duration` in ms and `Playback Position` in seconds. "Stopped" means no track.
pub fn parse(source: Source, info: &HashMap<String, String>) -> Option<Track> {
    let text = |key: &str| info.get(key).cloned().unwrap_or_default();
    let number = |key: &str| info.get(key).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let state = info.get("Player State")?;
    if state == "Stopped" {
        return None;
    }
    let duration_key = if source == Source::Music { "Total Time" } else { "Duration" };
    Some(Track {
        title: text("Name"),
        artist: text("Artist"),
        album: text("Album"),
        duration: number(duration_key) / 1000.0,
        position: number("Playback Position"),
        playing: state == "Playing",
        source,
        artwork: None,
    })
}

/// Music's cover art via AppleScript; otherwise (and for Spotify) the app's icon.
fn artwork(source: Source) -> Option<Retained<NSImage>> {
    if source == Source::Music
        && let Some(descriptor) = run(source, "get data of artwork 1 of current track")
    {
        let data = descriptor.data();
        if data.length() > 0
            && let Some(image) = NSImage::initWithData(NSImage::alloc(), &data)
        {
            return Some(image);
        }
    }
    let workspace = NSWorkspace::sharedWorkspace();
    let url = workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(source.bundle_id()))?;
    let path = url.path()?;
    Some(workspace.iconForFile(&path))
}

/// Runs `tell application "<app>" to <command>`, but never launches a closed app.
fn run(source: Source, command: &str) -> Option<Retained<objc2_foundation::NSAppleEventDescriptor>> {
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(source.bundle_id()));
    if running.is_empty() {
        return None;
    }
    let script = format!("tell application \"{}\" to {command}", source.app_name());
    let script = NSAppleScript::initWithSource(NSAppleScript::alloc(), &NSString::from_str(&script))?;
    let mut error = None;
    let result = unsafe { script.executeAndReturnError(Some(&mut error)) };
    if let Some(error) = error {
        eprintln!("notchpad: {} script failed: {error:?}", source.app_name());
        return None;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn parses_music_notification() {
        let track = parse(
            Source::Music,
            &info(&[("Name", "Song"), ("Artist", "Band"), ("Album", "LP"), ("Total Time", "215000"), ("Player State", "Playing")]),
        )
        .unwrap();
        assert_eq!((track.title.as_str(), track.artist.as_str(), track.album.as_str()), ("Song", "Band", "LP"));
        assert_eq!(track.duration, 215.0);
        assert!(track.playing);
    }

    #[test]
    fn parses_spotify_notification_with_position() {
        let track = parse(
            Source::Spotify,
            &info(&[("Name", "Tune"), ("Duration", "180000"), ("Playback Position", "12.5"), ("Player State", "Paused")]),
        )
        .unwrap();
        assert_eq!(track.duration, 180.0);
        assert_eq!(track.position, 12.5);
        assert!(!track.playing);
    }

    #[test]
    fn stopped_or_stateless_notification_has_no_track() {
        assert!(parse(Source::Music, &info(&[("Player State", "Stopped")])).is_none());
        assert!(parse(Source::Spotify, &info(&[("Name", "x")])).is_none());
    }

    #[test]
    fn tick_stops_at_duration_and_only_while_playing() {
        let mut now = NowPlaying::default();
        now.track = parse(Source::Spotify, &info(&[("Duration", "2000"), ("Player State", "Playing")]));
        for _ in 0..5 {
            now.tick();
        }
        assert_eq!(now.track.as_ref().unwrap().position, 2.0);
        now.track.as_mut().unwrap().playing = false;
        now.track.as_mut().unwrap().position = 1.0;
        now.tick();
        assert_eq!(now.track.as_ref().unwrap().position, 1.0);
    }
}
