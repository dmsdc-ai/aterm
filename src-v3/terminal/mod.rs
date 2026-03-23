pub mod renderer;
pub mod widget;

use alacritty_terminal::{
    event::VoidListener,
    grid::{Dimensions, Scroll},
    term::{Config, Term},
    vte::ansi,
};
use std::sync::{Arc, Mutex};

pub use renderer::{
    RenderedCell, RenderedLine, RenderedTerminal, TerminalRenderer,
};
pub use widget::{TerminalEvent, TerminalWidget};

pub type SharedTerminal = Arc<Mutex<Term<VoidListener>>>;

const SCROLLBACK_LINES: usize = 10000;

pub struct TerminalState {
    terminal: SharedTerminal,
    snapshot: String,
    columns: usize,
    rows: usize,
    scroll_offset: usize,
}

impl TerminalState {
    pub fn new(columns: usize, rows: usize) -> Self {
        let columns = columns.max(2);
        let rows = rows.max(1);

        Self {
            terminal: Arc::new(Mutex::new(Term::new(
                Config::default(),
                &TerminalDimensions { columns, rows },
                VoidListener,
            ))),
            snapshot: String::new(),
            columns,
            rows,
            scroll_offset: 0,
        }
    }

    pub fn terminal(&self) -> SharedTerminal {
        Arc::clone(&self.terminal)
    }

    pub fn snapshot(&self) -> &str {
        &self.snapshot
    }

    pub fn sync_snapshot(&mut self, snapshot: &str) {
        if self.snapshot == snapshot {
            return;
        }

        let was_at_bottom = self.scroll_offset == 0;
        self.snapshot.clear();
        self.snapshot.push_str(snapshot);
        self.rebuild_terminal();

        // Auto-scroll to bottom when new output arrives (if user wasn't scrolled up)
        if was_at_bottom {
            self.scroll_offset = 0;
            if let Ok(mut term) = self.terminal.lock() {
                term.scroll_display(Scroll::Bottom);
            }
        }
    }

    /// Scroll the viewport by `delta` lines (positive = up into history).
    pub fn scroll(&mut self, delta: i32) {
        if let Ok(mut term) = self.terminal.lock() {
            term.scroll_display(Scroll::Delta(delta));
            self.scroll_offset = term.grid().display_offset();
        }
    }

    /// Scroll to the bottom of the terminal output.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        if let Ok(mut term) = self.terminal.lock() {
            term.scroll_display(Scroll::Bottom);
        }
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(2);
        let rows = rows.max(1);

        if self.columns == columns && self.rows == rows {
            return;
        }

        self.columns = columns;
        self.rows = rows;
        self.rebuild_terminal();
    }

    fn rebuild_terminal(&mut self) {
        let mut terminal = Term::new(
            Config::default(),
            &TerminalDimensions {
                columns: self.columns,
                rows: self.rows,
            },
            VoidListener,
        );
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut terminal, self.snapshot.as_bytes());

        // Restore scroll position after rebuild
        if self.scroll_offset > 0 {
            terminal.scroll_display(Scroll::Delta(self.scroll_offset as i32));
        }

        if let Ok(mut shared) = self.terminal.lock() {
            *shared = terminal;
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
