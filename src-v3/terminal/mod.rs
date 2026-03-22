pub mod renderer;
pub mod widget;

use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{Config, Term},
    vte::ansi,
};
use std::sync::{Arc, Mutex};

pub use renderer::{
    RenderedCell, RenderedLine, RenderedTerminal, TerminalRenderer,
};
pub use widget::{TerminalEvent, TerminalWidget};

pub type SharedTerminal = Arc<Mutex<Term<VoidListener>>>;

pub struct TerminalState {
    terminal: SharedTerminal,
    snapshot: String,
    columns: usize,
    rows: usize,
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

        self.snapshot.clear();
        self.snapshot.push_str(snapshot);
        self.rebuild_terminal();
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
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}
