use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{Config, Term},
    vte::ansi::{self, Color, NamedColor},
};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use unicode_normalization::UnicodeNormalization;

use crate::inject::Osc133Mark;
use crate::sync::FairMutex;

pub type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
pub type SharedPtyWriter = Arc<Mutex<Option<PtyWriter>>>;
pub type SharedTerminal = Arc<FairMutex<Term<AtermEventListener>>>;

const SCROLLBACK_LINES: usize = 2000;
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
    pub fn shift_marks(&mut self, new_lines: usize, max_distance: usize) {
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
    pub fn add_mark(&mut self, bottom_distance: usize, kind: PromptKind) {
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
    fn find_prompt_up(&self, current_top_bd: usize) -> Option<usize> {
        self.marks
            .iter()
            .filter(|m| m.kind == PromptKind::PromptStart && m.bottom_distance > current_top_bd)
            .min_by_key(|m| m.bottom_distance)
            .map(|m| m.bottom_distance)
    }

    /// Find the next PromptStart mark BELOW the current view top.
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
    /// Terminal foreground color [R, G, B] for OSC 10 query response (#196).
    fg_color: Arc<Mutex<[u8; 3]>>,
    /// Terminal background color [R, G, B] for OSC 11 query response (#196).
    bg_color: Arc<Mutex<[u8; 3]>>,
    /// Buffered responses for when writer is None (Fix 2: writer race).
    /// Flushed when set_pty_writer connects the writer.
    pending_responses: Arc<Mutex<Vec<String>>>,
}

impl AtermEventListener {
    pub fn new() -> Self {
        Self {
            writer: Arc::new(Mutex::new(None)),
            // Dark theme defaults (Tokyo Night Storm)
            fg_color: Arc::new(Mutex::new([0xc0, 0xca, 0xf5])),
            bg_color: Arc::new(Mutex::new([0x1a, 0x1b, 0x26])),
            pending_responses: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Update terminal colors for OSC 10/11 responses (call on theme change).
    pub fn set_colors(&self, fg: [u8; 3], bg: [u8; 3]) {
        if let Ok(mut f) = self.fg_color.lock() {
            *f = fg;
        }
        if let Ok(mut b) = self.bg_color.lock() {
            *b = bg;
        }
    }

    pub fn fg_color_arc(&self) -> Arc<Mutex<[u8; 3]>> {
        self.fg_color.clone()
    }

    pub fn bg_color_arc(&self) -> Arc<Mutex<[u8; 3]>> {
        self.bg_color.clone()
    }

    pub fn pending_responses_arc(&self) -> Arc<Mutex<Vec<String>>> {
        self.pending_responses.clone()
    }

    /// Write text to PTY, or buffer if writer not yet connected.
    fn write_or_buffer(&self, text: &str) {
        if let Ok(guard) = self.writer.lock() {
            if let Some(ref writer) = *guard {
                if let Ok(mut w) = writer.lock() {
                    let _ = w.write_all(text.as_bytes());
                    let _ = w.flush();
                    debug_log!("[osc-color] wrote to PTY ({} bytes): {:?}", text.len(), &text[..text.len().min(60)]);
                    return;
                }
            }
        }
        // Writer not connected yet — buffer for flush on set_pty_writer
        debug_log!("[osc-color] writer not connected — buffering ({} bytes)", text.len());
        if let Ok(mut pending) = self.pending_responses.lock() {
            pending.push(text.to_string());
        }
    }
}

impl EventListener for AtermEventListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                // Override DA1: report VT220 + ANSI color instead of VT102.
                // Gemini CLI uses DA1 to detect terminal capabilities.
                if text == "\x1b[?6c" {
                    self.write_or_buffer("\x1b[?62;22c");
                } else {
                    self.write_or_buffer(&text);
                }
            }
            Event::ColorRequest(index, formatter) => {
                debug_log!("[osc-color] ColorRequest received: index={}", index);
                let rgb = match index {
                    256 => self
                        .fg_color
                        .lock()
                        .ok()
                        .map(|c| ansi::Rgb { r: c[0], g: c[1], b: c[2] }),
                    257 => self
                        .bg_color
                        .lock()
                        .ok()
                        .map(|c| ansi::Rgb { r: c[0], g: c[1], b: c[2] }),
                    258 => self
                        .fg_color
                        .lock()
                        .ok()
                        .map(|c| ansi::Rgb { r: c[0], g: c[1], b: c[2] }),
                    _ => None,
                };
                if let Some(rgb) = rgb {
                    debug_log!(
                        "[osc-color] responding: index={} rgb=({},{},{})",
                        index, rgb.r, rgb.g, rgb.b
                    );
                    let response = formatter(rgb);
                    self.write_or_buffer(&response);
                } else {
                    debug_log!("[osc-color] no color for index={} — no response", index);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// AdvanceHandle — owns the VTE parser, lives on the PTY reader thread.
// ---------------------------------------------------------------------------

/// Thread-safe handle for advancing terminal state from the PTY reader thread.
/// Owns the VTE parser.  Shares the terminal grid via FairMutex.
pub struct AdvanceHandle {
    terminal: SharedTerminal,
    parser: ansi::Processor,
    sync_active: Arc<AtomicBool>,
    /// Main thread sets this to request sync flush on next advance.
    force_stop_sync: Arc<AtomicBool>,
    pending_osc133: PendingOsc133,
    prompt_marks: Arc<Mutex<PromptMarkStore>>,
    scroll_offset: Arc<AtomicUsize>,
    max_scrollback: usize,
    /// PTY writer for XTVERSION responses (CSI > q) that VTE doesn't handle.
    pty_writer: SharedPtyWriter,
}

impl AdvanceHandle {
    /// Advance terminal with new PTY bytes.  Called from reader thread.
    ///
    /// Follows alacritty's pattern: lease → try_lock_unfair → advance.
    /// Terminal lock is dropped BEFORE prompt_marks to prevent deadlock.
    pub fn advance(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        // Handle force-stop-sync request from main thread (resize / timeout).
        if self.force_stop_sync.load(Ordering::Acquire) {
            self.force_stop_sync.store(false, Ordering::Release);
            let mut term = self.terminal.lock_unfair();
            self.parser.stop_sync(&mut *term);
            drop(term);
            self.sync_active.store(false, Ordering::Release);
        }

        // NFC normalization — Korean jamo → syllable composition.
        let has_non_ascii = bytes.iter().any(|&b| b > 0x7F);
        let normalized_buf: Vec<u8>;
        let feed: &[u8] = if has_non_ascii {
            match std::str::from_utf8(bytes) {
                Ok(s) => {
                    normalized_buf = s.nfc().collect::<String>().into_bytes();
                    &normalized_buf
                }
                Err(e) => {
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

        // XTVERSION: respond to CSI > q (\x1b[>q) — VTE parser doesn't handle this.
        // Gemini CLI sends this to detect terminal type; respond so it uses proper colors.
        const XTVERSION_SEQ: &[u8] = b"\x1b[>q";
        if feed.len() >= 4 && feed.windows(4).any(|w| w == XTVERSION_SEQ) {
            if let Ok(guard) = self.pty_writer.lock() {
                if let Some(ref writer) = *guard {
                    if let Ok(mut w) = writer.lock() {
                        // DCS >| aterm 3.0 ST
                        let _ = w.write_all(b"\x1bP>|aterm 3.0\x1b\\");
                        let _ = w.flush();
                    }
                }
            }
        }

        let was_at_bottom = self.scroll_offset.load(Ordering::Acquire) == 0;

        // Alacritty pattern: lease reserves turn, try_lock_unfair avoids blocking.
        let _lease = self.terminal.lease();
        let mut term = self.terminal.try_lock_unfair()
            .unwrap_or_else(|| self.terminal.lock_unfair());

        let old_history = term.grid().history_size();
        let old_cursor = term.grid().cursor.point.line.0.max(0) as usize;

        self.parser.advance(&mut *term, feed);
        let in_sync = self.parser.sync_bytes_count() > 0;
        self.sync_active.store(in_sync, Ordering::Release);

        if was_at_bottom {
            term.scroll_display(Scroll::Bottom);
        }
        self.scroll_offset
            .store(term.grid().display_offset(), Ordering::Release);

        // Capture grid metrics while lock is held.
        let new_history = term.grid().history_size();
        let new_cursor = term.grid().cursor.point.line.0.max(0) as usize;
        let screen_lines = term.grid().screen_lines();

        // DROP terminal lock BEFORE touching prompt_marks (lock ordering rule).
        drop(term);
        drop(_lease);

        let new_lines = (new_history + new_cursor).saturating_sub(old_history + old_cursor);

        // Drain pending OSC 133 marks (lock independently from prompt_marks).
        let drained_marks: Vec<Osc133Mark> = if let Ok(mut pending) = self.pending_osc133.lock() {
            if !pending.is_empty() {
                pending.drain(..).collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        if new_lines > 0 || !drained_marks.is_empty() {
            if let Ok(mut marks) = self.prompt_marks.lock() {
                if new_lines > 0 {
                    marks.shift_marks(new_lines, self.max_scrollback + screen_lines);
                }
                if !drained_marks.is_empty() {
                    let bottom_dist =
                        screen_lines.saturating_sub(1).saturating_sub(new_cursor);
                    debug_log!(
                        "[osc133-debug] processing {} pending marks, bottom_dist={}, total_marks={}",
                        drained_marks.len(),
                        bottom_dist,
                        marks.mark_count()
                    );
                    for mark in drained_marks {
                        match mark {
                            Osc133Mark::PromptStart => {
                                debug_log!("[osc133-debug] PromptStart mark stored at bd={}", bottom_dist);
                                marks.add_mark(bottom_dist, PromptKind::PromptStart);
                            }
                            Osc133Mark::CommandStart => {
                                marks.add_mark(bottom_dist, PromptKind::OutputStart);
                            }
                            Osc133Mark::CommandEnd(status) => {
                                marks.last_exit_status = status;
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

    /// Create a fresh handle for a restarted child process.
    /// Reuses shared state (terminal, marks, etc.) but resets the VTE parser.
    pub fn fresh_for_restart(&self) -> AdvanceHandle {
        AdvanceHandle {
            terminal: Arc::clone(&self.terminal),
            parser: ansi::Processor::new(),
            sync_active: Arc::clone(&self.sync_active),
            force_stop_sync: Arc::clone(&self.force_stop_sync),
            pending_osc133: Arc::clone(&self.pending_osc133),
            prompt_marks: Arc::clone(&self.prompt_marks),
            scroll_offset: Arc::clone(&self.scroll_offset),
            max_scrollback: self.max_scrollback,
            pty_writer: Arc::clone(&self.pty_writer),
        }
    }
}

// SAFETY: AdvanceHandle is moved to the reader thread exactly once.
// All fields are either Send (Arc<...>, ansi::Processor) or primitive.
// ansi::Processor contains vte::Parser which is Send.
unsafe impl Send for AdvanceHandle {}

// ---------------------------------------------------------------------------
// TerminalState — main thread interface to the terminal.
// ---------------------------------------------------------------------------

pub struct TerminalState {
    terminal: SharedTerminal,
    pty_writer: SharedPtyWriter,
    columns: usize,
    rows: usize,
    /// Shared with AdvanceHandle — atomic for cross-thread access.
    scroll_offset: Arc<AtomicUsize>,
    /// Shared with AdvanceHandle — mutex-protected for cross-thread access.
    prompt_marks: Arc<Mutex<PromptMarkStore>>,
    /// Pending OSC 133 marks pushed by reader_loop, drained in AdvanceHandle::advance().
    pending_osc133: PendingOsc133,
    /// Max scrollback capacity — used for mark pruning boundary.
    max_scrollback: usize,
    /// When synchronized output (?2026h) started. None = not in sync.
    /// Timeout after 1000ms forces render (ghostty pattern). #201.
    sync_started_at: Option<Instant>,
    /// Shared sync flag — updated by AdvanceHandle, read by main thread.
    sync_active: Arc<AtomicBool>,
    /// Signal to AdvanceHandle to flush sync bytes on next advance.
    force_stop_sync: Arc<AtomicBool>,
    /// Listener color arcs for theme sync (Fix 1: OSC 10/11 theme sync).
    listener_fg: Arc<Mutex<[u8; 3]>>,
    listener_bg: Arc<Mutex<[u8; 3]>>,
    /// Buffered responses waiting for writer connection (Fix 2: writer race).
    listener_pending: Arc<Mutex<Vec<String>>>,
}

impl TerminalState {
    /// Create a new terminal and its companion AdvanceHandle.
    ///
    /// The AdvanceHandle must be passed to the PTY reader thread so that
    /// VTE parsing happens off the main thread.
    pub fn new(
        columns: usize,
        rows: usize,
        sync_active: Option<Arc<AtomicBool>>,
    ) -> (Self, AdvanceHandle) {
        let columns = columns.max(2);
        let rows = rows.max(1);

        let listener = AtermEventListener::new();
        let pty_writer = listener.writer.clone();
        let listener_fg = listener.fg_color_arc();
        let listener_bg = listener.bg_color_arc();
        let listener_pending = listener.pending_responses_arc();
        let config = Config::default();
        let max_scrollback = config.scrolling_history;
        let sync_active =
            sync_active.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let force_stop_sync = Arc::new(AtomicBool::new(false));
        let scroll_offset = Arc::new(AtomicUsize::new(0));
        let prompt_marks = Arc::new(Mutex::new(PromptMarkStore::new()));
        let pending_osc133: PendingOsc133 = Arc::new(Mutex::new(Vec::new()));
        let parser = ansi::Processor::new();

        let terminal: SharedTerminal = Arc::new(FairMutex::new(Term::new(
            config,
            &TerminalDimensions { columns, rows },
            listener,
        )));

        let advance_handle = AdvanceHandle {
            terminal: Arc::clone(&terminal),
            parser,
            sync_active: Arc::clone(&sync_active),
            force_stop_sync: Arc::clone(&force_stop_sync),
            pending_osc133: Arc::clone(&pending_osc133),
            prompt_marks: Arc::clone(&prompt_marks),
            scroll_offset: Arc::clone(&scroll_offset),
            max_scrollback,
            pty_writer: pty_writer.clone(),
        };

        let state = Self {
            terminal,
            pty_writer,
            columns,
            rows,
            scroll_offset,
            prompt_marks,
            pending_osc133,
            max_scrollback,
            sync_started_at: None,
            sync_active,
            force_stop_sync,
            listener_fg,
            listener_bg,
            listener_pending,
        };

        (state, advance_handle)
    }

    /// Create a new AdvanceHandle for this terminal (e.g. after workspace restart).
    /// The returned handle has a fresh VTE parser but shares all state.
    pub fn create_advance_handle(&self) -> AdvanceHandle {
        AdvanceHandle {
            terminal: Arc::clone(&self.terminal),
            parser: ansi::Processor::new(),
            sync_active: Arc::clone(&self.sync_active),
            force_stop_sync: Arc::clone(&self.force_stop_sync),
            pending_osc133: Arc::clone(&self.pending_osc133),
            prompt_marks: Arc::clone(&self.prompt_marks),
            scroll_offset: Arc::clone(&self.scroll_offset),
            max_scrollback: self.max_scrollback,
            pty_writer: self.pty_writer.clone(),
        }
    }

    /// Connect the PTY writer so DA responses flow back to the child process.
    /// Also flushes any buffered responses from before the writer was connected (Fix 2).
    pub fn set_pty_writer(&self, writer: PtyWriter) {
        if let Ok(mut slot) = self.pty_writer.lock() {
            *slot = Some(writer.clone());
        }
        // Flush buffered responses (OSC 10/11 queries that arrived before writer was set)
        if let Ok(mut pending) = self.listener_pending.lock() {
            if !pending.is_empty() {
                if let Ok(mut w) = writer.lock() {
                    for response in pending.drain(..) {
                        let _ = w.write_all(response.as_bytes());
                    }
                    let _ = w.flush();
                }
            }
        }
    }

    /// Check if GPU render should be skipped (synchronized output active).
    /// Returns true = skip render, false = render now.
    /// Handles 1000ms timeout: auto-forces render if ?2026l never arrives (#201).
    pub fn should_skip_render(&mut self) -> bool {
        if !self.sync_active.load(Ordering::Acquire) {
            self.sync_started_at = None;
            return false;
        }
        // In sync mode — check timeout
        const SYNC_TIMEOUT_MS: u64 = 1000;
        match self.sync_started_at {
            None => {
                self.sync_started_at = Some(Instant::now());
                true
            }
            Some(started) => {
                if started.elapsed().as_millis() as u64 >= SYNC_TIMEOUT_MS {
                    debug_log!(
                        "[sync-output] timeout expired ({}ms), forcing render",
                        SYNC_TIMEOUT_MS
                    );
                    self.force_stop_sync();
                    false
                } else {
                    true
                }
            }
        }
    }

    /// Force exit synchronized output mode.
    /// Signals the reader thread to flush buffered sync bytes on next advance.
    /// Called on timeout and resize (ghostty/kitty pattern).
    fn force_stop_sync(&mut self) {
        self.force_stop_sync.store(true, Ordering::Release);
        self.sync_active.store(false, Ordering::Release);
        self.sync_started_at = None;
    }

    /// Shared flag for reader thread: true = parser in sync, suppress mark_dirty().
    pub fn sync_active_flag(&self) -> Arc<AtomicBool> {
        self.sync_active.clone()
    }

    /// Update listener colors for OSC 10/11 responses (call on theme/scheme change).
    pub fn set_listener_colors(&self, fg: [u8; 3], bg: [u8; 3]) {
        if let Ok(mut f) = self.listener_fg.lock() {
            *f = fg;
        }
        if let Ok(mut b) = self.listener_bg.lock() {
            *b = bg;
        }
    }

    pub fn terminal(&self) -> SharedTerminal {
        Arc::clone(&self.terminal)
    }

    /// One-time drain of startup bytes that arrived before the AdvanceHandle
    /// was connected to the reader thread.  Called on main thread.
    pub fn advance_startup(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut term = self.terminal.lock();
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut *term, bytes);
    }

    pub fn scroll(&mut self, delta: i32) {
        let mut term = self.terminal.lock();
        term.scroll_display(Scroll::Delta(delta));
        self.scroll_offset
            .store(term.grid().display_offset(), Ordering::Release);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset.store(0, Ordering::Release);
        let mut term = self.terminal.lock();
        term.scroll_display(Scroll::Bottom);
    }

    /// Check if the visible screen contains a text pattern.
    pub fn screen_contains(&self, pattern: &str) -> bool {
        let term = self.terminal.lock();
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
        self.prompt_marks
            .lock()
            .map(|m| m.mark_count())
            .unwrap_or(0)
    }

    /// Get a clone of the pending OSC 133 marks handle for sharing with reader_loop.
    pub fn pending_osc133(&self) -> PendingOsc133 {
        Arc::clone(&self.pending_osc133)
    }

    /// Scroll to the next/previous prompt mark.
    /// direction < 0 = up (previous prompt), direction > 0 = down (next prompt).
    /// Returns true if scrolled, false if no prompt found in that direction.
    pub fn scroll_to_prompt(&mut self, direction: i32) -> bool {
        // Step 1: Read current view position (terminal lock).
        let (current_top_bd, _screen_lines) = {
            let term = self.terminal.lock();
            let screen_lines = term.grid().screen_lines();
            let display_offset = term.grid().display_offset();
            (display_offset + screen_lines - 1, screen_lines)
        };
        // Terminal lock dropped here.

        // Step 2: Find target mark (prompt_marks lock, no terminal lock held).
        let target_bd = if let Ok(marks) = self.prompt_marks.lock() {
            if direction < 0 {
                marks.find_prompt_up(current_top_bd)
            } else {
                marks.find_prompt_down(current_top_bd)
            }
        } else {
            None
        };
        // prompt_marks lock dropped here.

        // Step 3: Scroll to target (terminal lock only).
        if let Some(bd) = target_bd {
            let mut term = self.terminal.lock();
            let screen_lines = term.grid().screen_lines();
            let history_size = term.grid().history_size();
            let target_offset = bd.saturating_sub(screen_lines - 1);
            let clamped = target_offset.min(history_size);

            term.scroll_display(Scroll::Bottom);
            if clamped > 0 {
                term.scroll_display(Scroll::Delta(clamped as i32));
            }
            self.scroll_offset
                .store(term.grid().display_offset(), Ordering::Release);
            true
        } else if direction > 0 {
            // No prompt below → scroll to bottom
            let mut term = self.terminal.lock();
            term.scroll_display(Scroll::Bottom);
            self.scroll_offset.store(0, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(2);
        let rows = rows.max(1);

        // Resize clears synchronized output (ghostty/kitty pattern) #201
        if self.sync_active.load(Ordering::Acquire) {
            debug_log!("[sync-output] resize clears sync state");
            self.force_stop_sync();
        }

        if self.columns == columns && self.rows == rows {
            return;
        }

        self.columns = columns;
        self.rows = rows;

        let mut term = self.terminal.lock();
        term.resize(TerminalDimensions { columns, rows });

        // Fix #157: Reset cursor template bg to default after resize.
        term.grid_mut().cursor.template.bg = Color::Named(NamedColor::Background);
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
