//! User preferences in `settings.txt` beside `tasks.txt`: which notch sections are shown and
//! what the collapsed notch may display.
//!
//! Plain `key=value` lines: `hidden=clips,terminal`, `size=terminal:xtall,clips:tall`,
//! `wing_music=1`, `wing_focus=1`, `calendar=1` (show today's calendar events), `linear=1` (a Linear key is in the Keychain; the key itself is never written here), `focus=25`, `short_break=5`, `long_break=15`, `sound=1`,
//! `remind=10` (minutes ahead, `0` for only at the time, `off` for none).
//! A missing file, a missing key, an unknown key or a garbled line leaves the default,
//! which shows everything.

use std::{fs, io, path::PathBuf};

/// A notch section as the Settings window lists it, in header order.
pub struct Section {
    pub key: &'static str,
    pub name: &'static str,
    pub symbol: &'static str,
    pub blurb: &'static str,
}

pub const SECTIONS: [Section; 4] = [
    Section { key: "workspace", name: "Workspace", symbol: "checklist", blurb: "Today's tasks and the focus timer" },
    Section { key: "clips", name: "Clips", symbol: "doc.on.clipboard", blurb: "Clipboard history and dropped files" },
    Section { key: "music", name: "Playing", symbol: "play.circle", blurb: "Now playing and playback controls" },
    Section { key: "terminal", name: "Terminal", symbol: "terminal", blurb: "A shell in the notch" },
];

/// How tall the expanded notch is for a section, as a multiple of the compact body height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Size {
    #[default]
    Compact,
    Tall,
    ExtraTall,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Compact, Size::Tall, Size::ExtraTall];

    pub fn scale(self) -> f64 {
        match self {
            Size::Compact => 1.0,
            Size::Tall => 1.6,
            Size::ExtraTall => 2.5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Size::Compact => "Compact",
            Size::Tall => "Tall",
            Size::ExtraTall => "Extra tall",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Size::Compact => "compact",
            Size::Tall => "tall",
            Size::ExtraTall => "xtall",
        }
    }
}

/// Focus lengths the timer offers, in minutes; clicking the timer cycles through them.
pub const FOCUS_CHOICES: [u64; 4] = [15, 25, 45, 60];
pub const SHORT_BREAK_CHOICES: [u64; 3] = [3, 5, 10];
pub const LONG_BREAK_CHOICES: [u64; 4] = [10, 15, 20, 30];
/// How long before a timed task the notch first reminds of it; `None` turns reminders off.
pub const REMIND_CHOICES: [Option<u32>; 5] = [None, Some(0), Some(5), Some(10), Some(15)];

/// Index of Music in [`SECTIONS`].
const MUSIC: usize = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Indexed like [`SECTIONS`]. Never all true: one section always stays on.
    pub hidden: [bool; SECTIONS.len()],
    /// Indexed like [`SECTIONS`].
    pub sizes: [Size; SECTIONS.len()],
    pub wing_music: bool,
    pub wing_focus: bool,
    /// Whether today's calendar events show beside the tasks. Off until the user turns it on,
    /// which is also when macOS is asked for calendar access.
    pub calendar: bool,
    /// Whether Linear is connected. Only this flag lives in `settings.txt`; the key is in the Keychain,
    /// which the app reads only while this is set.
    pub linear: bool,
    /// One of [`FOCUS_CHOICES`]: the length a new session starts with.
    pub focus: u64,
    /// One of [`SHORT_BREAK_CHOICES`].
    pub short_break: u64,
    /// One of [`LONG_BREAK_CHOICES`].
    pub long_break: u64,
    /// Whether alerts ring the chime.
    pub sound: bool,
    /// One of [`REMIND_CHOICES`].
    pub remind: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hidden: [false; SECTIONS.len()],
            sizes: [Size::Compact; SECTIONS.len()],
            wing_music: true,
            wing_focus: true,
            calendar: false,
            linear: false,
            focus: 25,
            short_break: 5,
            long_break: 15,
            sound: true,
            remind: Some(10),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        fs::read_to_string(path()).map(|text| parse(&text)).unwrap_or_default()
    }

    /// Writes via a temp file and rename, like the task file.
    pub fn save(&self) -> io::Result<()> {
        let path = path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, format(self))?;
        fs::rename(tmp, path)
    }

    /// Shows or hides section `index`. Refuses (returns false) to hide the last visible one.
    pub fn set_shown(&mut self, index: usize, shown: bool) -> bool {
        let mut hidden = self.hidden;
        hidden[index] = !shown;
        if hidden.iter().all(|h| *h) {
            return false;
        }
        self.hidden = hidden;
        true
    }

    /// First section that is not hidden.
    pub fn first_visible(&self) -> usize {
        self.hidden.iter().position(|h| !h).unwrap_or(0)
    }

    /// The now-playing helper is needed by the Music section or by the music wing.
    pub fn needs_now_playing(&self) -> bool {
        !self.hidden[MUSIC] || self.wing_music
    }
}

fn path() -> PathBuf {
    crate::store::path().with_file_name("settings.txt")
}

fn parse(text: &str) -> Settings {
    let mut settings = Settings::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        let flag = |current: bool| match value {
            "0" => false,
            "1" => true,
            _ => current,
        };
        match key.trim() {
            "hidden" => {
                for name in value.split(',') {
                    if let Some(i) = SECTIONS.iter().position(|s| s.key == name.trim()) {
                        settings.hidden[i] = true;
                    }
                }
            }
            "size" => {
                for entry in value.split(',') {
                    let Some((name, word)) = entry.split_once(':') else { continue };
                    let size = Size::ALL.into_iter().find(|s| s.key() == word.trim());
                    if let (Some(i), Some(size)) = (SECTIONS.iter().position(|s| s.key == name.trim()), size) {
                        settings.sizes[i] = size;
                    }
                }
            }
            "wing_music" => settings.wing_music = flag(settings.wing_music),
            "wing_focus" => settings.wing_focus = flag(settings.wing_focus),
            "calendar" => settings.calendar = flag(settings.calendar),
            "linear" => settings.linear = flag(settings.linear),
            "focus" => settings.focus = choice(value, &FOCUS_CHOICES, settings.focus),
            "short_break" => settings.short_break = choice(value, &SHORT_BREAK_CHOICES, settings.short_break),
            "long_break" => settings.long_break = choice(value, &LONG_BREAK_CHOICES, settings.long_break),
            "sound" => settings.sound = flag(settings.sound),
            "remind" if value == "off" => settings.remind = None,
            "remind" => settings.remind = value.parse().ok().filter(|m| REMIND_CHOICES.contains(&Some(*m))).map_or(settings.remind, Some),
            _ => {}
        }
    }
    // A hand-edited file must not leave an empty header.
    if settings.hidden.iter().all(|h| *h) {
        settings.hidden = [false; SECTIONS.len()];
    }
    settings
}

/// `value` if it parses to one of `choices`, else `current`.
fn choice(value: &str, choices: &[u64], current: u64) -> u64 {
    value.parse().ok().filter(|v| choices.contains(v)).unwrap_or(current)
}

fn format(settings: &Settings) -> String {
    let hidden: Vec<&str> = SECTIONS.iter().zip(settings.hidden).filter(|(_, h)| *h).map(|(s, _)| s.key).collect();
    let sizes: Vec<String> = SECTIONS.iter().zip(settings.sizes).filter(|(_, z)| *z != Size::Compact).map(|(s, z)| format!("{}:{}", s.key, z.key())).collect();
    format!(
        "hidden={}\nsize={}\nwing_music={}\nwing_focus={}\ncalendar={}\nlinear={}\nfocus={}\nshort_break={}\nlong_break={}\nsound={}\nremind={}\n",
        hidden.join(","),
        sizes.join(","),
        settings.wing_music as u8,
        settings.wing_focus as u8,
        settings.calendar as u8,
        settings.linear as u8,
        settings.focus,
        settings.short_break,
        settings.long_break,
        settings.sound as u8,
        settings.remind.map_or("off".to_owned(), |m| m.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_index_matches_the_table() {
        assert_eq!(SECTIONS[MUSIC].key, "music");
    }

    #[test]
    fn missing_file_content_shows_everything() {
        assert_eq!(parse(""), Settings::default());
    }

    #[test]
    fn round_trip() {
        let mut settings = Settings::default();
        settings.hidden[1] = true;
        settings.hidden[3] = true;
        settings.wing_music = false;
        assert!(format(&settings).starts_with("hidden=clips,terminal\nsize=\nwing_music=0\nwing_focus=1\n"));
        assert_eq!(parse(&format(&settings)), settings);
        assert_eq!(parse(&format(&Settings::default())), Settings::default());
    }

    #[test]
    fn calendar_is_off_unless_the_file_says_so() {
        assert!(!parse("").calendar);
        assert!(!parse("calendar=maybe\n").calendar);
        let mut settings = parse("calendar=1\n");
        assert!(settings.calendar);
        assert!(format(&settings).contains("calendar=1\n"));
        settings.calendar = false;
        assert_eq!(parse(&format(&settings)), settings);
    }

    #[test]
    fn linear_is_off_unless_the_file_says_so() {
        assert!(!parse("").linear);
        let mut settings = parse("linear=1\n");
        assert!(settings.linear);
        assert!(format(&settings).contains("linear=1\n"));
        settings.linear = false;
        assert_eq!(parse(&format(&settings)), settings);
    }

    #[test]
    fn sizes_round_trip_listing_only_non_compact() {
        let mut settings = Settings::default();
        settings.sizes[1] = Size::Tall;
        settings.sizes[3] = Size::ExtraTall;
        assert!(format(&settings).contains("size=clips:tall,terminal:xtall\n"));
        assert_eq!(parse(&format(&settings)), settings);
        assert_eq!(parse("hidden=clips\n").sizes, [Size::Compact; SECTIONS.len()]);
    }

    #[test]
    fn unknown_size_words_and_sections_are_ignored() {
        // `week` and `notes` were sections once; files that still name them load as before.
        let settings = parse("size=terminal:huge,bogus:tall,week:tall,clips:tall,notes\n");
        assert_eq!(settings.sizes, [Size::Compact, Size::Tall, Size::Compact, Size::Compact]);
    }

    #[test]
    fn focus_options_round_trip() {
        let mut settings = Settings::default();
        settings.focus = 45;
        settings.short_break = 10;
        settings.long_break = 30;
        settings.sound = false;
        settings.remind = None;
        assert!(format(&settings).ends_with("focus=45\nshort_break=10\nlong_break=30\nsound=0\nremind=off\n"));
        assert_eq!(parse(&format(&settings)), settings);
        settings.remind = Some(0);
        assert_eq!(parse(&format(&settings)), settings);
    }

    #[test]
    fn focus_options_outside_the_choices_keep_the_default() {
        let settings = parse("focus=7\nshort_break=x\nlong_break=999\nremind=12\nsound=maybe\n");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn unknown_keys_and_names_are_ignored() {
        let settings = parse("theme=dark\nhidden=notes,bogus,music\nwing_focus=0\n");
        assert_eq!(settings.hidden, [false, false, true, false]);
        assert!(!settings.wing_focus);
    }

    #[test]
    fn garbage_lines_are_ignored() {
        let settings = parse("???\n\n=\nwing_music=maybe\nhidden\n  hidden = clips \n");
        assert_eq!(settings.hidden, [false, true, false, false]);
        assert!(settings.wing_music);
    }

    #[test]
    fn hiding_every_section_in_the_file_shows_them_all() {
        let all = SECTIONS.map(|s| s.key).join(",");
        assert_eq!(parse(&format!("hidden={all}")).hidden, [false; SECTIONS.len()]);
    }

    #[test]
    fn the_last_visible_section_cannot_be_hidden() {
        let mut settings = Settings::default();
        for i in 0..3 {
            assert!(settings.set_shown(i, false));
        }
        assert!(!settings.set_shown(3, false));
        assert!(!settings.hidden[3]);
        assert_eq!(settings.first_visible(), 3);
        assert!(settings.set_shown(0, true));
    }

    #[test]
    fn helper_runs_for_the_music_section_or_its_wing() {
        let mut settings = Settings::default();
        settings.hidden[MUSIC] = true;
        assert!(settings.needs_now_playing());
        settings.wing_music = false;
        assert!(!settings.needs_now_playing());
    }
}
