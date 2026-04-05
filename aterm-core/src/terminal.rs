use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{Config, Term},
    vte::ansi::{self, Color, NamedColor},
};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};
use unicode_normalization::UnicodeNormalization;

use crate::inject::Osc133Mark;

pub type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
pub type SharedPtyWriter = Arc<Mutex<Option<PtyWriter>>>;
pub type SharedTerminal = Arc<Mutex<Term<AtermEventListener>>>;

const SCROLLBACK_LINES: usize = 10000;
const MAX_PROMPT_MARKS: usize = 2000;

// --- OSC 133 prompt mark store ---

/// Per-line prompt kind, following Kitty's 2-bit pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PromptKind {
    /// OSC 133;A — shell prompt begins on this line.
    PromptStart = 1,
    /// OSC 133;C — command output begins on this line.
    OutputStart = 2,
}

/// A single prompt mark, positioned by distance from the bottom of the buffer.
#[derive(Debug, Clone)]
struct PromptMarkEntry {
    /// Distance from the very bottom line of the terminal buffer.
    /// 0 = bottom-most line, increasing upward.
    bottom_distance: usize,
    kind: PromptKind,
}

/// Stores OSC 133 prompt marks for scroll-to-prompt navigation.
/// Marks are sorted by `bottom_distance` ascending (newest/closest to bottom first).
pub struct PromptMarkStore {
    marks: VecDeque<PromptMarkEntry>,
    /// Last exit status from OSC 133;D.
    pub last_exit_status: Option<i32>,
}

impl PromptMarkStore {
    fn new() -> Self {
        Self {
            marks: VecDeque::new(),
            last_exit_status: None,
        }
    }

    /// Shift all marks upward when new lines are produced.
    fn shift_marks(&mut self, new_lines: usize, max_distance: usize) {
        if new_lines == 0 {
            return;
        }
        for mark in &mut self.marks {
            mark.bottom_distance += new_lines;
        }
        // Prune marks that have scrolled beyond the buffer
        while let Some(front) = self.marks.front() {
            if front.bottom_distance > max_distance {
                self.marks.pop_front();
            } else {
                break;
            }
        }
    }

    /// Record a new mark at the given distance from the buffer bottom.
    fn add_mark(&mut self, bottom_distance: usize, kind: PromptKind) {
        // Insert in sorted position (marks are sorted ascending by bottom_distance,
        // but new marks almost always go to the back since they're near the bottom)
        self.marks.push_back(PromptMarkEntry {
            bottom_distance,
            kind,
        });
        // Enforce capacity limit by removing oldest marks (front)
        while self.marks.len() > MAX_PROMPT_MARKS {
            self.marks.pop_front();
        }
    }

    /// Find the next PromptStart mark ABOVE the current view top.
    /// Returns the mark's bottom_distance.
    fn find_prompt_up(&self, current_top_bd: usize) -> Option<usize> {
        self.marks
            .iter()
            .filter(|m| m.kind == PromptKind::PromptStart && m.bottom_distance > current_top_bd)
            .min_by_key(|m| m.bottom_distance)
            .map(|m| m.bottom_distance)
    }

    /// Find the next PromptStart mark BELOW the current view top.
    /// Returns the mark's bottom_distance.
    fn find_prompt_down(&self, current_top_bd: usize) -> Option<usize> {
        self.marks
            .iter()
            .filter(|m| m.kind == PromptKind::PromptStart && m.bottom_distance < current_top_bd)
            .max_by_key(|m| m.bottom_distance)
            .map(|m| m.bottom_distance)
    }

    pub fn mark_count(&self) -> usize {
        self.marks.len()
    }
}

pub type PendingOsc133 = Arc<Mutex<Vec<Osc133Mark>>>;

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
    /// OSC 133 prompt marks for scroll-to-prompt navigation.
    prompt_marks: PromptMarkStore,
    /// Pending OSC 133 marks pushed by reader_loop, drained in advance().
    pending_osc133: PendingOsc133,
    /// Max scrollback capacity (from Config) — used for mark pruning boundary.
    max_scrollback: usize,
}

impl TerminalState {
    pub fn new(columns: usize, rows: usize) -> Self {
        let columns = columns.max(2);
        let rows = rows.max(1);

        let listener = AtermEventListener::new();
        let pty_writer = listener.writer.clone();
        let config = Config::default();
        let max_scrollback = config.scrolling_history;

        Self {
            terminal: Arc::new(Mutex::new(Term::new(
                config,
                &TerminalDimensions { columns, rows },
                listener,
            ))),
            pty_writer,
            parser: ansi::Processor::new(),
            columns,
            rows,
            scroll_offset: 0,
            prompt_marks: PromptMarkStore::new(),
            pending_osc133: Arc::new(Mutex::new(Vec::new())),
            max_scrollback,
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
    ///
    /// Applies NFC normalization so macOS NFD Korean jamo are composed into
    /// syllables before the VTE parser stores them. Escape sequences are
    /// ASCII-only, so NFC is identity for them — safe to normalize the whole buffer.
    pub fn advance(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        // Fast path: pure ASCII needs no normalization (zero allocation).
        let has_non_ascii = bytes.iter().any(|&b| b > 0x7F);
        let normalized_buf: Vec<u8>;
        let feed: &[u8] = if has_non_ascii {
            match std::str::from_utf8(bytes) {
                Ok(s) => {
                    normalized_buf = s.nfc().collect::<String>().into_bytes();
                    &normalized_buf
                }
                Err(e) => {
                    // Partial UTF-8 at chunk boundary: normalize the valid prefix,
                    // pass trailing incomplete bytes through as-is.
                    let valid_up_to = e.valid_up_to();
                    if valid_up_to == 0 {
                        bytes
                    } else {
                        let valid =
                            unsafe { std::str::from_utf8_unchecked(&bytes[..valid_up_to]) };
                        normalized_buf = {
                            let mut buf = valid.nfc().collect::<String>().into_bytes();
                            buf.extend_from_slice(&bytes[valid_up_to..]);
                            buf
                        };
                        &normalized_buf
                    }
                }
            }
        } else {
            bytes
        };

        let was_at_bottom = self.scroll_offset == 0;
        if let Ok(mut term) = self.terminal.lock() {
            // Capture cursor position before advance for mark shifting
            let old_history = term.grid().history_size();
            let old_cursor = term.grid().cursor.point.line.0.max(0) as usize;

            self.parser.advance(&mut *term, feed);
            if was_at_bottom {
                term.scroll_display(Scroll::Bottom);
            }
            // Always re-sync from grid to prevent drift after VTE state changes
            self.scroll_offset = term.grid().display_offset();

            // Process OSC 133 marks: shift existing marks and record new ones
            let new_history = term.grid().history_size();
            let new_cursor = term.grid().cursor.point.line.0.max(0) as usize;
            let screen_lines = term.grid().screen_lines();

            let new_total = new_history + new_cursor;
            let old_total = old_history + old_cursor;
            let new_lines = new_total.saturating_sub(old_total);

            if new_lines > 0 {
                self.prompt_marks
                    .shift_marks(new_lines, self.max_scrollback + screen_lines);
            }

            if let Ok(mut pending) = self.pending_osc133.lock() {
                if !pending.is_empty() {
                    let bottom_dist =
                        screen_lines.saturating_sub(1).saturating_sub(new_cursor);
                    debug_log!(
                        "[osc133-debug] processing {} pending marks, bottom_dist={}, total_marks={}",
                        pending.len(),
                        bottom_dist,
                        self.prompt_marks.mark_count()
                    );
                    for mark in pending.drain(..) {
                        match mark {
                            Osc133Mark::PromptStart => {
                                debug_log!("[osc133-debug] PromptStart mark stored at bd={}", bottom_dist);
                                self.prompt_marks
                                    .add_mark(bottom_dist, PromptKind::PromptStart);
                            }
                            Osc133Mark::CommandStart => {
                                self.prompt_marks
                                    .add_mark(bottom_dist, PromptKind::OutputStart);
                            }
                            Osc133Mark::CommandEnd(status) => {
                                self.prompt_marks.last_exit_status = status;
                            }
                            Osc133Mark::PromptEnd => {
                                // B marks don't need line storage — used for inject timing only
                            }
                        }
                    }
                }
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

    /// Number of OSC 133 prompt marks currently stored.
    pub fn prompt_mark_count(&self) -> usize {
        self.prompt_marks.mark_count()
    }

    /// Get a clone of the pending OSC 133 marks handle for sharing with reader_loop.
    pub fn pending_osc133(&self) -> PendingOsc133 {
        Arc::clone(&self.pending_osc133)
    }

    /// Scroll to the next/previous prompt mark.
    /// direction < 0 = up (previous prompt), direction > 0 = down (next prompt).
    /// Returns true if scrolled, false if no prompt found in that direction.
    pub fn scroll_to_prompt(&mut self, direction: i32) -> bool {
        if let Ok(mut term) = self.terminal.lock() {
            let screen_lines = term.grid().screen_lines();
            let display_offset = term.grid().display_offset();
            let history_size = term.grid().history_size();

            // Top of current view expressed as bottom_distance
            let current_top_bd = display_offset + screen_lines - 1;

            let target_bd = if direction < 0 {
                self.prompt_marks.find_prompt_up(current_top_bd)
            } else {
                self.prompt_marks.find_prompt_down(current_top_bd)
            };

            if let Some(bd) = target_bd {
                // Place the prompt at the top of the visible area
                let target_offset = bd.saturating_sub(screen_lines - 1);
                let clamped = target_offset.min(history_size);

                term.scroll_display(Scroll::Bottom);
                if clamped > 0 {
                    term.scroll_display(Scroll::Delta(clamped as i32));
                }
                self.scroll_offset = term.grid().display_offset();
                true
            } else if direction > 0 {
                // No prompt below → scroll to bottom
                term.scroll_display(Scroll::Bottom);
                self.scroll_offset = 0;
                true
            } else {
                false
            }
        } else {
            false
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

            // Fix #157: Reset cursor template bg to default after resize.
            // Industry standard (ghostty/alacritty/wezterm/kitty/contour):
            // resize new cells = default bg, NOT cursor's current SGR bg.
            // Without this, child process post-SIGWINCH erase ops inherit
            // the cursor's SGR bg (e.g. codex's magenta #FF00FF), painting
            // the entire screen with that color instead of the terminal
            // background. The child will re-set SGR attributes when it
            // redraws after SIGWINCH.
            term.grid_mut().cursor.template.bg = Color::Named(NamedColor::Background);
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
