//! The Terminal tab's command runner: current folder, history and one background child.
//! There is no PTY, so interactive programs are refused and sent to Terminal.app instead.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub const MAX_LINES: usize = 2000;
const MAX_HISTORY: usize = 50;
/// Programs that need a real terminal; running them here would hang on missing input.
const INTERACTIVE: [&str; 8] = ["vim", "nvim", "vi", "top", "htop", "less", "man", "ssh"];
const SIGKILL: i32 = 9;

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// A piece of a running command's output, or its end.
pub enum Chunk {
    Out(String),
    Err(String),
    Exit(i32),
}

/// What `Shell::submit` did with a line, so the caller knows what to print and whether to
/// start polling.
pub enum Submitted {
    /// Handled in-process; the message is an error, if any.
    Done(Option<String>),
    Refused(String),
    Started,
}

pub struct Shell {
    home: PathBuf,
    dir: PathBuf,
    history: History,
    running: Option<Running>,
}

struct Running {
    rx: Receiver<Chunk>,
    pid: i32,
}

impl Default for Shell {
    fn default() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()));
        Self { dir: home.clone(), home, history: History::default(), running: None }
    }
}

impl Shell {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn dir_label(&self) -> String {
        dir_label(&self.dir, &self.home)
    }

    pub fn history_up(&mut self, draft: &str) -> Option<String> {
        self.history.up(draft).map(str::to_owned)
    }

    pub fn history_down(&mut self) -> Option<String> {
        self.history.down().map(str::to_owned)
    }

    /// Runs `line`: `cd` in-process, interactive programs refused, anything else spawned.
    pub fn submit(&mut self, line: &str) -> Submitted {
        self.history.push(line);
        if let Some(arg) = cd_arg(line) {
            let target = resolve_cd(&self.dir, &self.home, arg);
            return Submitted::Done(match target.canonicalize() {
                Ok(dir) if dir.is_dir() => {
                    self.dir = dir;
                    None
                }
                _ => Some(format!("cd: no such directory: {arg}")),
            });
        }
        let first = line.split_whitespace().next().unwrap_or("");
        if INTERACTIVE.contains(&first) {
            return Submitted::Refused(format!("{first} needs a real terminal; use Open in Terminal."));
        }
        match spawn(line, &self.dir) {
            Ok(running) => {
                self.running = Some(running);
                Submitted::Started
            }
            Err(err) => Submitted::Done(Some(format!("could not run: {err}"))),
        }
    }

    /// Output that arrived since the last call; a trailing `Exit` ends the command.
    pub fn drain(&mut self) -> Vec<Chunk> {
        let Some(running) = &self.running else { return Vec::new() };
        let chunks: Vec<Chunk> = running.rx.try_iter().collect();
        if chunks.iter().any(|c| matches!(c, Chunk::Exit(_))) {
            self.running = None;
        }
        chunks
    }

    /// Kills the whole process group: `zsh -lc` may have started children of its own.
    pub fn kill(&self) {
        if let Some(running) = &self.running {
            unsafe { kill(-running.pid, SIGKILL) };
        }
    }

    pub fn open_in_terminal(&self) -> std::io::Result<()> {
        Command::new("open").args(["-a", "Terminal"]).arg(&self.dir).status().map(|_| ())
    }
}

fn spawn(line: &str, dir: &Path) -> std::io::Result<Running> {
    let mut child = Command::new("/bin/zsh")
        .args(["-lc", line])
        .current_dir(dir)
        // Own process group, so kill reaches grandchildren; no stdin, so `cat` can't hang.
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id() as i32;
    let (tx, rx) = mpsc::channel();
    let out = pump(child.stdout.take().expect("piped"), tx.clone(), Chunk::Out);
    let err = pump(child.stderr.take().expect("piped"), tx.clone(), Chunk::Err);
    thread::spawn(move || {
        // Exit goes last, after both pipes are drained.
        let _ = (out.join(), err.join());
        let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = tx.send(Chunk::Exit(code));
    });
    Ok(Running { rx, pid })
}

fn pump(mut pipe: impl Read + Send + 'static, tx: Sender<Chunk>, wrap: fn(String) -> Chunk) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        // A multi-byte character split across reads shows as U+FFFD; rare enough to accept.
        while let Ok(n @ 1..) = pipe.read(&mut buf) {
            if tx.send(wrap(String::from_utf8_lossy(&buf[..n]).into_owned())).is_err() {
                break;
            }
        }
    })
}

/// The argument of a plain `cd` / `cd <dir>`; `None` for anything else, which zsh runs.
fn cd_arg(line: &str) -> Option<&str> {
    let mut words = line.split_whitespace();
    if words.next()? != "cd" {
        return None;
    }
    let arg = words.next().unwrap_or("");
    words.next().is_none().then_some(arg)
}

fn resolve_cd(current: &Path, home: &Path, arg: &str) -> PathBuf {
    match arg {
        "" | "~" => home.to_owned(),
        _ => match arg.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => current.join(arg), // an absolute `arg` replaces `current`
        },
    }
}

/// `~/a/b/c` becomes `…/b/c`: the last two components, home shown as `~`.
fn dir_label(dir: &Path, home: &Path) -> String {
    let full = match dir.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => dir.display().to_string(),
    };
    let parts: Vec<&str> = full.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [.., a, b] if parts.len() > 2 => format!("…/{a}/{b}"),
        _ => full,
    }
}

/// Byte offset where the text must start to keep only its last `max` lines.
pub fn trim_start(text: &str, max: usize) -> usize {
    // A trailing newline ends the last line rather than starting another one.
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines = body.matches('\n').count() + 1;
    if lines <= max {
        return 0;
    }
    body.match_indices('\n').nth(lines - max - 1).map_or(0, |(i, _)| i + 1)
}

/// Previously run commands, newest last, with a cursor for ↑/↓.
#[derive(Default)]
struct History {
    entries: Vec<String>,
    /// Equals `entries.len()` when not browsing.
    cursor: usize,
    /// What was typed before browsing started, restored by ↓ past the newest entry.
    draft: String,
}

impl History {
    fn push(&mut self, line: &str) {
        if !line.is_empty() && self.entries.last().map(String::as_str) != Some(line) {
            self.entries.push(line.to_owned());
            if self.entries.len() > MAX_HISTORY {
                self.entries.remove(0);
            }
        }
        self.cursor = self.entries.len();
    }

    fn up(&mut self, draft: &str) -> Option<&str> {
        if self.cursor == self.entries.len() {
            self.draft = draft.to_owned();
        }
        self.cursor = self.cursor.checked_sub(1)?;
        Some(&self.entries[self.cursor])
    }

    fn down(&mut self) -> Option<&str> {
        if self.cursor >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        Some(self.entries.get(self.cursor).unwrap_or(&self.draft))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cd_arguments() {
        assert_eq!(cd_arg("cd"), Some(""));
        assert_eq!(cd_arg("cd  /tmp "), Some("/tmp"));
        assert_eq!(cd_arg("cd a b"), None);
        assert_eq!(cd_arg("cdx a"), None);
        assert_eq!(cd_arg("ls"), None);
    }

    #[test]
    fn cd_resolution() {
        let (cur, home) = (Path::new("/work/proj"), Path::new("/Users/me"));
        assert_eq!(resolve_cd(cur, home, ""), home);
        assert_eq!(resolve_cd(cur, home, "~"), home);
        assert_eq!(resolve_cd(cur, home, "~/docs"), Path::new("/Users/me/docs"));
        assert_eq!(resolve_cd(cur, home, "/etc"), Path::new("/etc"));
        assert_eq!(resolve_cd(cur, home, "src"), Path::new("/work/proj/src"));
        assert_eq!(resolve_cd(cur, home, ".."), Path::new("/work/proj/.."));
    }

    #[test]
    fn labels() {
        let home = Path::new("/Users/me");
        assert_eq!(dir_label(Path::new("/Users/me"), home), "~");
        assert_eq!(dir_label(Path::new("/Users/me/Projects"), home), "~/Projects");
        assert_eq!(dir_label(Path::new("/Users/me/a/b/c"), home), "…/b/c");
        assert_eq!(dir_label(Path::new("/usr/local/bin"), home), "…/local/bin");
        assert_eq!(dir_label(Path::new("/usr"), home), "/usr");
    }

    #[test]
    fn history_walks_up_and_down() {
        let mut h = History::default();
        for c in ["one", "two", "three"] {
            h.push(c);
        }
        assert_eq!(h.up("draft"), Some("three"));
        assert_eq!(h.up("ignored"), Some("two"));
        assert_eq!(h.up(""), Some("one"));
        assert_eq!(h.up(""), None);
        assert_eq!(h.down(), Some("two"));
        assert_eq!(h.down(), Some("three"));
        assert_eq!(h.down(), Some("draft"));
        assert_eq!(h.down(), None);
    }

    #[test]
    fn history_skips_blanks_and_repeats_and_caps() {
        let mut h = History::default();
        h.push("");
        h.push("a");
        h.push("a");
        assert_eq!(h.entries, ["a"]);
        for i in 0..60 {
            h.push(&format!("c{i}"));
        }
        assert_eq!(h.entries.len(), MAX_HISTORY);
        assert_eq!(h.entries[0], "c10");
    }

    #[test]
    fn trim_keeps_last_lines() {
        assert_eq!(trim_start("a\nb\nc\n", 3), 0);
        assert_eq!(trim_start("a\nb\nc\nd\n", 3), 2);
        assert_eq!(trim_start("a\nb\nc\nd", 2), 4);
        assert_eq!(trim_start("", 2), 0);
        let long: String = (0..2500).map(|i| format!("{i}\n")).collect();
        let kept = &long[trim_start(&long, MAX_LINES)..];
        assert_eq!(kept.lines().count(), MAX_LINES);
        assert!(kept.ends_with("2499\n"));
    }
}
