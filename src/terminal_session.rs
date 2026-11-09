//! A shell on a pseudo-terminal, with alacritty's emulator keeping the screen state.
//! Reading the PTY and parsing it happen on alacritty's own thread; the owner is told
//! through the wake callback and then reads the grid under `term()`.

use std::borrow::Cow;
use std::io;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config;
use alacritty_terminal::tty::{self, Options, Pty, Shell};
use alacritty_terminal::Term;

use crate::terminal_palette;

/// Called on the PTY thread whenever the screen changed or the shell exited.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

const SCROLLBACK_LINES: usize = 5000;

/// Grid size in cells, plus the cell size in pixels that programs can query.
#[derive(Clone, Copy, PartialEq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows.into()
    }

    fn screen_lines(&self) -> usize {
        self.rows.into()
    }

    fn columns(&self) -> usize {
        self.cols.into()
    }
}

impl Size {
    fn window(self) -> WindowSize {
        WindowSize { num_lines: self.rows, num_cols: self.cols, cell_width: self.cell_width, cell_height: self.cell_height }
    }
}

/// Runs on the PTY thread: forwards what the emulator wants to say back to the shell.
#[derive(Clone)]
pub struct Listener {
    pty_input: Arc<OnceLock<EventLoopSender>>,
    exited: Arc<AtomicBool>,
    wake: Wake,
}

impl Listener {
    fn reply(&self, text: String) {
        if let Some(sender) = self.pty_input.get() {
            let _ = sender.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => (self.wake)(),
            // Cursor-position and device-attribute replies; vim and claude wait for them.
            Event::PtyWrite(text) => self.reply(text),
            // Programs ask for the colors to pick a matching theme.
            Event::ColorRequest(index, format) => self.reply(format(terminal_palette::rgb(index))),
            Event::Exit => {
                self.exited.store(true, Ordering::SeqCst);
                (self.wake)();
            }
            _ => {}
        }
    }
}

pub struct TerminalSession {
    term: Arc<FairMutex<Term<Listener>>>,
    pty_input: EventLoopSender,
    exited: Arc<AtomicBool>,
    /// The thread owns the PTY; dropping the handle lets it finish and hang up the shell.
    _thread: JoinHandle<(EventLoop<Pty, Listener>, State)>,
}

impl TerminalSession {
    /// Starts the user's login shell in their home directory.
    pub fn spawn(size: Size, wake: Wake) -> io::Result<Self> {
        let pty = tty::new(&Self::options(), size.window(), 0)?;
        let listener = Listener { pty_input: Arc::new(OnceLock::new()), exited: Arc::new(AtomicBool::new(false)), wake };
        let term = Arc::new(FairMutex::new(Term::new(
            Config { scrolling_history: SCROLLBACK_LINES, ..Config::default() },
            &size,
            listener.clone(),
        )));
        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, false, false)?;
        let pty_input = event_loop.channel();
        let _ = listener.pty_input.set(pty_input.clone());
        Ok(Self { term, pty_input, exited: listener.exited, _thread: event_loop.spawn() })
    }

    fn options() -> Options {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".to_owned());
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let mut env = std::collections::HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ]);
        // An app launched from Finder has no locale, and multi-byte output would be mangled.
        if std::env::var_os("LANG").is_none_or(|l| l.is_empty()) {
            env.insert("LANG".to_owned(), "en_US.UTF-8".to_owned());
        }
        if let Some(home) = &home {
            env.insert("HOME".to_owned(), home.to_string_lossy().into_owned());
        }
        Options {
            shell: Some(Shell::new(shell, vec!["-l".to_owned()])),
            working_directory: home,
            env,
            ..Options::default()
        }
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.pty_input.send(Msg::Input(bytes.into()));
    }

    pub fn resize(&self, size: Size) {
        self.term.lock().resize(size);
        let _ = self.pty_input.send(Msg::Resize(size.window()));
    }

    pub fn scroll(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// The emulator state; hold the guard briefly, the PTY thread waits on it.
    pub fn term(&self) -> impl Deref<Target = Term<Listener>> + '_ {
        self.term.lock()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.pty_input.send(Msg::Shutdown);
    }
}
