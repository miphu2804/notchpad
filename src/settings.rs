//! User preferences in `settings.txt` beside `tasks.txt`: which notch sections are shown and
//! what the collapsed notch may display.
//!
//! Plain `key=value` lines: `hidden=clips,terminal`, `wing_music=1`, `wing_focus=1`.
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

pub const SECTIONS: [Section; 6] = [
    Section { key: "workspace", name: "Workspace", symbol: "checklist", blurb: "Today's tasks and the focus timer" },
    Section { key: "week", name: "Week", symbol: "calendar", blurb: "Events, reminders and the week's tasks" },
    Section { key: "clips", name: "Clips", symbol: "paperclip", blurb: "Clipboard history and dropped files" },
    Section { key: "music", name: "Music", symbol: "music.note", blurb: "Now playing and playback controls" },
    Section { key: "notes", name: "Notes", symbol: "note.text", blurb: "A scratch note" },
    Section { key: "terminal", name: "Terminal", symbol: "terminal", blurb: "A shell in the notch" },
];

/// Index of Music in [`SECTIONS`].
const MUSIC: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Indexed like [`SECTIONS`]. Never all true: one section always stays on.
    pub hidden: [bool; SECTIONS.len()],
    pub wing_music: bool,
    pub wing_focus: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { hidden: [false; SECTIONS.len()], wing_music: true, wing_focus: true }
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
            "wing_music" => settings.wing_music = flag(settings.wing_music),
            "wing_focus" => settings.wing_focus = flag(settings.wing_focus),
            _ => {}
        }
    }
    // A hand-edited file must not leave an empty header.
    if settings.hidden.iter().all(|h| *h) {
        settings.hidden = [false; SECTIONS.len()];
    }
    settings
}

fn format(settings: &Settings) -> String {
    let hidden: Vec<&str> = SECTIONS.iter().zip(settings.hidden).filter(|(_, h)| *h).map(|(s, _)| s.key).collect();
    format!(
        "hidden={}\nwing_music={}\nwing_focus={}\n",
        hidden.join(","),
        settings.wing_music as u8,
        settings.wing_focus as u8,
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
        settings.hidden[2] = true;
        settings.hidden[5] = true;
        settings.wing_music = false;
        assert_eq!(format(&settings), "hidden=clips,terminal\nwing_music=0\nwing_focus=1\n");
        assert_eq!(parse(&format(&settings)), settings);
        assert_eq!(parse(&format(&Settings::default())), Settings::default());
    }

    #[test]
    fn unknown_keys_and_names_are_ignored() {
        let settings = parse("theme=dark\nhidden=notes,bogus\nwing_focus=0\n");
        assert_eq!(settings.hidden, [false, false, false, false, true, false]);
        assert!(!settings.wing_focus);
    }

    #[test]
    fn garbage_lines_are_ignored() {
        let settings = parse("???\n\n=\nwing_music=maybe\nhidden\n  hidden = week \n");
        assert_eq!(settings.hidden, [false, true, false, false, false, false]);
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
        for i in 0..5 {
            assert!(settings.set_shown(i, false));
        }
        assert!(!settings.set_shown(5, false));
        assert!(!settings.hidden[5]);
        assert_eq!(settings.first_visible(), 5);
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
