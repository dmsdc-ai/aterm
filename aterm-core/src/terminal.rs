use alacritty_terminal::{
    event::VoidListener,
    grid::{Dimensions, Scroll},
    term::{Config, Term},
    vte::ansi,
};
use std::sync::{Arc, Mutex};

pub type SharedTerminal = Arc<Mutex<Term<VoidListener>>>;

const SCROLLBACK_LINES: usize = 10000;

pub struct TerminalState {
    terminal: SharedTerminal,
    parser: ansi::Processor,
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
            parser: ansi::Processor::new(),
            columns,
            rows,
            scroll_offset: 0,
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
                self.scroll_offset = 0;
                term.scroll_display(Scroll::Bottom);
            }
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
