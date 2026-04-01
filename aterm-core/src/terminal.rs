use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{Config, Term},
    vte::ansi,
};
use std::io::Write;
use std::sync::{Arc, Mutex};

pub type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
pub type SharedPtyWriter = Arc<Mutex<Option<PtyWriter>>>;
pub type SharedTerminal = Arc<Mutex<Term<AtermEventListener>>>;

const SCROLLBACK_LINES: usize = 10000;

/// Routes terminal write-back events (DA responses, etc.) to the PTY master.
/// Without this, apps like Codex CLI never receive Device Attributes responses
/// and fall back to dumb terminal mode, leaking escape sequence fragments.
#[derive(Clone)]
pub struct AtermEventListener {
    writer: SharedPtyWriter,
}

impl AtermEventListener {
    pub fn new() -> Self {
        Self {
            writer: Arc::new(Mutex::new(None)),
        }
    }
}

impl EventListener for AtermEventListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                if let Ok(guard) = self.writer.lock() {
                    if let Some(ref writer) = *guard {
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(text.as_bytes());
                            let _ = w.flush();
                        }
                    }
                }
            }
            // Other events (Title, Bell, Clipboard, etc.) are not critical for
            // fixing the rendering residue. Can be handled later as needed.
            _ => {}
        }
    }
}

pub struct TerminalState {
    terminal: SharedTerminal,
    pty_writer: SharedPtyWriter,
    parser: ansi::Processor,
    columns: usize,
    rows: usize,
    scroll_offset: usize,
}

impl TerminalState {
    pub fn new(columns: usize, rows: usize) -> Self {
        let columns = columns.max(2);
        let rows = rows.max(1);

        let listener = AtermEventListener::new();
        let pty_writer = listener.writer.clone();

        Self {
            terminal: Arc::new(Mutex::new(Term::new(
                Config::default(),
                &TerminalDimensions { columns, rows },
                listener,
            ))),
            pty_writer,
            parser: ansi::Processor::new(),
            columns,
            rows,
            scroll_offset: 0,
        }
    }

    /// Connect the PTY writer so DA responses flow back to the child process.
    pub fn set_pty_writer(&self, writer: PtyWriter) {
        if let Ok(mut slot) = self.pty_writer.lock() {
            *slot = Some(writer);
        }
    }

    pub fn terminal(&self) -> SharedTerminal {
        Arc::clone(&self.terminal)
    }

    /// Feed new PTY bytes incrementally into the terminal -- O(new_bytes) not O(total).
    pub fn advance(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let was_at_bottom = self.scroll_offset == 0;
        if let Ok(mut term) = self.terminal.lock() {
            self.parser.advance(&mut *term, bytes);
            if was_at_bottom {
                term.scroll_display(Scroll::Bottom);
            }
            // Always re-sync from grid to prevent drift after VTE state changes
            self.scroll_offset = term.grid().display_offset();
        }
    }

    pub fn scroll(&mut self, delta: i32) {
        if let Ok(mut term) = self.terminal.lock() {
            term.scroll_display(Scroll::Delta(delta));
            self.scroll_offset = term.grid().display_offset();
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        if let Ok(mut term) = self.terminal.lock() {
            term.scroll_display(Scroll::Bottom);
        }
    }

    /// Check if the visible screen contains a text pattern.
    pub fn screen_contains(&self, pattern: &str) -> bool {
        let term = match self.terminal.lock() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        for line_idx in 0..rows {
            let row = &grid[alacritty_terminal::index::Line(line_idx as i32)];
            let line_text: String = (0..cols)
                .map(|col| row[alacritty_terminal::index::Column(col)].c)
                .collect();
            if line_text.contains(pattern) {
                return true;
            }
        }
        false
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(2);
        let rows = rows.max(1);

        if self.columns == columns && self.rows == rows {
            return;
        }

        self.columns = columns;
        self.rows = rows;

        if let Ok(mut term) = self.terminal.lock() {
            term.resize(TerminalDimensions { columns, rows });
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TerminalDimensions {
    columns: usize,
    rows: usize,
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.rows + SCROLLBACK_LINES
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}
