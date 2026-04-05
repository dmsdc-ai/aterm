use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::pty::WorkspaceStatus;

pub const IDLE_THRESHOLD: Duration = Duration::from_secs(2);
pub const OUTPUT_SETTLE: Duration = Duration::from_secs(1);
const PROMPT_PATTERNS: &[&str] = &[
    "\u{276f}",
    "> ",
    "$ ",
    "% ",
    ">>> ",
    "# ",
    "\u{2192} ",
    "\u{276f}\u{276f} ",
    "\u{2726} ",
];
const BARE_PROMPTS: &[&str] = &[
    "\u{276f}",
    ">",
    "$",
    "%",
    ">>>",
    "#",
    "\u{2192}",
    "\u{276f}\u{276f}",
    "\u{2726}",
];
const FORCE_INJECT_TIMEOUT: Duration = Duration::from_secs(30);
const INJECT_QUEUE_CAPACITY: usize = 256;

/// How the prompt was detected — determines inject timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PromptSource {
    /// Definitive signal from OSC 133;B — no settle wait needed.
    Osc133,
    /// Pattern-matched heuristic — requires OUTPUT_SETTLE wait.
    Heuristic,
}

/// OSC 133 semantic prompt markers (FinalTerm/shell integration protocol).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Osc133Mark {
    PromptStart,             // A — shell is about to display prompt
    PromptEnd,               // B — prompt displayed, ready for input
    CommandStart,            // C — user submitted command, execution begins
    CommandEnd(Option<i32>), // D;exit_status — command execution finished
}

/// Scan data for OSC 133 markers. Returns all markers found.
/// Handles both BEL-terminated (\x1b]133;X\x07) and ST-terminated (\x1b]133;X\x1b\\) forms.
/// Also handles D marks with exit status: \x1b]133;D;N\x07 where N is the exit code.
pub fn detect_osc133(data: &str) -> Vec<Osc133Mark> {
    let mut marks = Vec::new();
    let bytes = data.as_bytes();
    let len = bytes.len();

    // Debug: check if ANY ESC ] sequence exists in the data
    let esc_count = bytes.iter().filter(|&&b| b == 0x1b).count();
    if esc_count > 0 {
        // Look for \x1b] specifically (OSC introducer)
        let osc_count = bytes
            .windows(2)
            .filter(|w| w[0] == 0x1b && w[1] == b']')
            .count();
        if osc_count > 0 {
            // Check if "133" follows any \x1b]
            let osc133_count = bytes
                .windows(6)
                .filter(|w| w[0] == 0x1b && w[1] == b']' && w[2] == b'1' && w[3] == b'3' && w[4] == b'3' && w[5] == b';')
                .count();
            debug_log!(
                "[osc133-debug] detect_osc133: len={}, esc={}, osc_intros={}, osc133_matches={}",
                len, esc_count, osc_count, osc133_count
            );
        }
    }

    let mut i = 0;

    while i + 6 < len {
        if bytes[i] == 0x1b
            && bytes[i + 1] == b']'
            && bytes[i + 2] == b'1'
            && bytes[i + 3] == b'3'
            && bytes[i + 4] == b'3'
            && bytes[i + 5] == b';'
        {
            if let Some(&mark_byte) = bytes.get(i + 6) {
                match mark_byte {
                    b'A' | b'B' | b'C' => {
                        let terminated = match bytes.get(i + 7) {
                            Some(&0x07) => true,
                            Some(&0x1b) => bytes.get(i + 8) == Some(&b'\\'),
                            _ => false,
                        };
                        if terminated {
                            match mark_byte {
                                b'A' => marks.push(Osc133Mark::PromptStart),
                                b'B' => marks.push(Osc133Mark::PromptEnd),
                                b'C' => marks.push(Osc133Mark::CommandStart),
                                _ => {}
                            }
                        }
                        // outer i += 7 handles advancement
                    }
                    b'D' => {
                        // D can be followed by ;exit_status before the terminator.
                        // Scan forward (max 16 bytes) for BEL or ST.
                        let start = i + 7;
                        let scan_end = (start + 16).min(len);
                        let mut j = start;
                        let mut terminated = false;
                        while j < scan_end {
                            match bytes[j] {
                                0x07 => {
                                    terminated = true;
                                    break;
                                }
                                0x1b if j + 1 < len && bytes[j + 1] == b'\\' => {
                                    terminated = true;
                                    break;
                                }
                                _ => j += 1,
                            }
                        }
                        if terminated {
                            // Parse optional exit status from ;N between mark_byte and terminator
                            let param_slice = &bytes[start..j];
                            let exit_status = if param_slice.first() == Some(&b';') {
                                std::str::from_utf8(&param_slice[1..])
                                    .ok()
                                    .and_then(|s| s.parse::<i32>().ok())
                            } else {
                                None
                            };
                            marks.push(Osc133Mark::CommandEnd(exit_status));
                        }
                        i = j + 1;
                        continue;
                    }
                    _ => {}
                }
            }
            i += 7;
        } else {
            i += 1;
        }
    }

    marks
}
/// Leading-edge throttle: after processing an inject, coalesce subsequent
/// events within this window before processing the next one.
const THROTTLE_COALESCE: Duration = Duration::from_millis(25);

pub type SharedInjectQueue = Arc<Mutex<InjectQueue>>;

/// Event-driven signal for the injector loop. Replaces 500ms polling with
/// Condvar-based wake: zero CPU when idle, instant response on events.
#[derive(Clone)]
pub struct InjectSignal {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl InjectSignal {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Wake the injector loop. Called on: message enqueued, prompt detected,
    /// status change to dead/closing.
    pub fn notify(&self) {
        let (lock, cvar) = &*self.inner;
        if let Ok(mut ready) = lock.lock() {
            *ready = true;
            cvar.notify_one();
        }
    }

    /// Block until notified or timeout expires. Returns true if notified.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let (lock, cvar) = &*self.inner;
        let Ok(ready) = lock.lock() else {
            return false;
        };
        let mut ready = if *ready {
            ready
        } else {
            match cvar.wait_timeout(ready, timeout) {
                Ok((guard, _)) => guard,
                Err(_) => return false,
            }
        };
        let was = *ready;
        *ready = false;
        was
    }

    /// Block indefinitely until notified.
    pub fn wait(&self) {
        let (lock, cvar) = &*self.inner;
        if let Ok(mut ready) = lock.lock() {
            while !*ready {
                match cvar.wait(ready) {
                    Ok(guard) => ready = guard,
                    Err(_) => return,
                }
            }
            *ready = false;
        }
    }
}

impl Default for InjectSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct InjectMessage {
    pub from: String,
    pub text: String,
    pub timestamp: u64,
    pub enqueued_at: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct InjectMessageInfo {
    pub from: String,
    pub text: String,
    pub timestamp: u64,
}

#[derive(Debug, Default)]
pub struct InjectQueue {
    messages: VecDeque<InjectMessage>,
}

impl InjectQueue {
    pub fn new() -> Self {
        Self {
            messages: VecDeque::new(),
        }
    }

    pub fn push(&mut self, mut message: InjectMessage) -> Result<usize, String> {
        if self.messages.len() >= INJECT_QUEUE_CAPACITY {
            return Err("inject queue full".to_string());
        }
        message.enqueued_at = Instant::now();
        self.messages.push_back(message);
        Ok(self.messages.len())
    }

    pub fn oldest_enqueued_at(&self) -> Option<Instant> {
        self.messages.front().map(|m| m.enqueued_at)
    }

    pub fn pop(&mut self) -> Option<InjectMessage> {
        self.messages.pop_front()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn snapshot(&self) -> Vec<InjectMessageInfo> {
        self.messages
            .iter()
            .map(|message| InjectMessageInfo {
                from: message.from.clone(),
                text: message.text.clone(),
                timestamp: message.timestamp,
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct IdleState {
    last_user_input: Instant,
    last_output_at: Instant,
    last_output_has_prompt: bool,
    prompt_source: Option<PromptSource>,
}

impl Default for IdleState {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            last_user_input: now,
            last_output_at: now,
            last_output_has_prompt: false,
            prompt_source: None,
        }
    }

    pub fn record_user_input(&mut self) {
        self.last_user_input = Instant::now();
        self.last_output_has_prompt = false;
        self.prompt_source = None;
    }

    pub fn record_output(&mut self, has_prompt: bool) {
        let now = Instant::now();
        self.last_output_at = now;
        if has_prompt && self.prompt_source != Some(PromptSource::Osc133) {
            self.last_output_has_prompt = true;
            self.prompt_source = Some(PromptSource::Heuristic);
        }
    }

    /// Record prompt detected via OSC 133;B — definitive signal.
    pub fn record_osc133_prompt(&mut self) {
        self.last_output_has_prompt = true;
        self.prompt_source = Some(PromptSource::Osc133);
    }

    pub fn should_inject(&self, queue_has_messages: bool) -> bool {
        if !self.last_output_has_prompt || !queue_has_messages {
            return false;
        }
        if self.last_user_input.elapsed() < IDLE_THRESHOLD {
            return false;
        }
        match self.prompt_source {
            // OSC 133 is definitive — no settle wait needed
            Some(PromptSource::Osc133) => true,
            // Heuristic needs output to settle
            _ => self.last_output_at.elapsed() >= OUTPUT_SETTLE,
        }
    }

    pub fn reset_prompt(&mut self) {
        self.last_output_has_prompt = false;
        self.prompt_source = None;
    }
}

pub fn has_prompt_pattern(data: &str) -> bool {
    let stripped = strip_ansi(data);
    let trimmed = stripped.trim();

    BARE_PROMPTS.iter().any(|prompt| trimmed == *prompt)
        || PROMPT_PATTERNS
            .iter()
            .any(|pattern| stripped.contains(pattern))
}

pub fn normalize_terminal_text(input: &str) -> String {
    let stripped = strip_ansi(input);
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn with_terminal_enter(text: &str) -> String {
    let normalized = text.trim_end_matches(|ch| matches!(ch, '\r' | '\n'));
    format!("{}\r", normalized)
}

pub fn split_at_utf8_boundary(bytes: &[u8]) -> (&[u8], &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(_) => (bytes, &[]),
        Err(error) => {
            let valid_up_to = error.valid_up_to();
            (&bytes[..valid_up_to], &bytes[valid_up_to..])
        }
    }
}

pub fn run_injector_loop(
    inject_queue: SharedInjectQueue,
    idle_state: Arc<Mutex<IdleState>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    status: WorkspaceStatus,
    signal: InjectSignal,
) {
    loop {
        // Determine wait strategy based on queue state
        let wait_duration = {
            let queue = inject_queue.lock().ok();
            match queue {
                Some(q) if q.is_empty() => None, // wait indefinitely
                Some(q) => {
                    let deadline = q
                        .oldest_enqueued_at()
                        .map(|t| FORCE_INJECT_TIMEOUT.saturating_sub(t.elapsed()))
                        .unwrap_or(OUTPUT_SETTLE);
                    // Cap wait to OUTPUT_SETTLE so we recheck should_inject() as
                    // time-based conditions (output settle, user idle) become true.
                    // Without this cap, the loop blocks until force-inject (30s)
                    // even when idle-gate conditions are met within 1-2s.
                    Some(deadline.min(OUTPUT_SETTLE))
                }
                None => Some(Duration::from_secs(1)),
            }
        };

        // Condvar wait — zero CPU when queue is empty
        match wait_duration {
            None => signal.wait(),
            Some(dur) if dur > Duration::ZERO => {
                signal.wait_timeout(dur);
            }
            Some(_) => {} // deadline passed, proceed immediately
        }

        // Check lifecycle
        if let Ok(current) = status.0.lock() {
            if matches!(current.as_str(), "dead" | "closing") {
                break;
            }
        }

        let should_inject = {
            let idle = idle_state.lock().ok();
            let queue = inject_queue.lock().ok();
            match (idle, queue) {
                (Some(idle), Some(queue)) => idle.should_inject(!queue.is_empty()),
                _ => false,
            }
        };

        if should_inject {
            let source = idle_state.lock().ok().and_then(|s| s.prompt_source);
            let src_label = match source {
                Some(PromptSource::Osc133) => "osc133",
                Some(PromptSource::Heuristic) => "heuristic",
                None => "unknown",
            };
            log_stderr!("[inject] idle-gate passed (source={src_label})");
        }

        if !should_inject {
            // Force-inject fallback: oldest message waiting > 30s
            let force_inject = {
                let queue = inject_queue.lock().ok();
                match queue {
                    Some(queue) => queue
                        .oldest_enqueued_at()
                        .map(|t| t.elapsed() >= FORCE_INJECT_TIMEOUT)
                        .unwrap_or(false),
                    _ => false,
                }
            };

            if !force_inject {
                continue;
            }

            log_stderr!(
                "[inject] force-inject after {}s timeout (prompt not detected)",
                FORCE_INJECT_TIMEOUT.as_secs()
            );
        }

        let message = inject_queue.lock().ok().and_then(|mut queue| queue.pop());
        let Some(message) = message else {
            continue;
        };

        let write_result = if let Ok(mut handle) = writer.lock() {
            let text = message
                .text
                .trim_end_matches(|ch: char| ch == '\r' || ch == '\n');
            let result = handle
                .write_all(text.as_bytes())
                .and_then(|_| handle.flush());
            if result.is_ok() {
                // Brief pause so TUI frameworks (Codex/ink, Gemini/bubbletea)
                // process the text characters before receiving Enter.
                drop(handle);
                thread::sleep(Duration::from_millis(50));
                if let Ok(mut handle) = writer.lock() {
                    handle.write_all(b"\r").and_then(|_| handle.flush())
                } else {
                    Err(io::Error::other("inject writer lock failed"))
                }
            } else {
                result
            }
        } else {
            Err(io::Error::other("inject writer lock failed"))
        };

        if write_result.is_err() {
            if let Ok(mut queue) = inject_queue.lock() {
                queue.clear();
            }
            close_writer_handle(&writer);
            if let Ok(mut current) = status.0.lock() {
                *current = "dead".to_string();
            }
            status.1.notify_all();
            break;
        }

        if let Ok(mut idle) = idle_state.lock() {
            idle.reset_prompt();
        }

        // Leading-edge throttle: first inject fires immediately (above),
        // coalesce rapid subsequent events before next iteration.
        thread::sleep(THROTTLE_COALESCE);
    }
}

pub fn close_writer_handle(writer: &Arc<Mutex<Box<dyn Write + Send>>>) {
    if let Ok(mut handle) = writer.lock() {
        *handle = Box::new(ClosedWriter);
    }
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut escaping = false;

    for ch in input.chars() {
        if escaping {
            if ch.is_ascii_alphabetic() || matches!(ch, '~' | '\\') {
                escaping = false;
            }
            continue;
        }

        if ch == '\u{1b}' {
            escaping = true;
            continue;
        }

        output.push(ch);
    }

    output
}

struct ClosedWriter;

impl Write for ClosedWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "PTY writer closed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "PTY writer closed",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_prompt_pattern_detects_ansi_wrapped_prompt() {
        assert!(has_prompt_pattern("\u{1b}[32m>\u{1b}[0m"));
        assert!(has_prompt_pattern("\u{1b}[35m❯\u{1b}[0m"));
    }

    #[test]
    fn prompt_detection_stays_latched_until_user_input() {
        let mut idle = IdleState::new();

        idle.record_output(true);
        idle.record_output(false);
        idle.last_output_at = Instant::now()
            .checked_sub(OUTPUT_SETTLE + Duration::from_millis(50))
            .unwrap();
        idle.last_user_input = Instant::now()
            .checked_sub(IDLE_THRESHOLD + Duration::from_millis(50))
            .unwrap();

        assert!(idle.should_inject(true));

        idle.record_user_input();
        assert!(!idle.should_inject(true));
    }

    #[test]
    fn inject_signal_wakes_immediately_on_notify() {
        let signal = InjectSignal::new();
        let signal2 = signal.clone();

        let start = Instant::now();
        let handle = std::thread::spawn(move || {
            signal2.wait();
            start.elapsed()
        });

        std::thread::sleep(Duration::from_millis(10));
        signal.notify();

        let elapsed = handle.join().unwrap();
        assert!(
            elapsed < Duration::from_millis(200),
            "Signal took {:?} to wake (expected <200ms, not 500ms polling)",
            elapsed
        );
    }

    #[test]
    fn inject_signal_wait_timeout_returns_false_without_notify() {
        let signal = InjectSignal::new();
        let notified = signal.wait_timeout(Duration::from_millis(50));
        assert!(
            !notified,
            "wait_timeout should return false when not notified"
        );
    }

    #[test]
    fn inject_signal_wait_timeout_returns_true_on_notify() {
        let signal = InjectSignal::new();
        let signal2 = signal.clone();

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            signal2.notify();
        });

        let notified = signal.wait_timeout(Duration::from_secs(1));
        assert!(notified, "wait_timeout should return true when notified");
    }

    #[test]
    fn inject_signal_empty_queue_waits_for_timeout() {
        let signal = InjectSignal::new();
        let start = Instant::now();
        let notified = signal.wait_timeout(Duration::from_millis(50));
        let elapsed = start.elapsed();

        assert!(!notified);
        assert!(
            elapsed >= Duration::from_millis(40),
            "Should wait close to timeout duration, got {:?}",
            elapsed
        );
    }

    #[test]
    fn force_inject_timeout_constant_is_30s() {
        assert_eq!(FORCE_INJECT_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn force_inject_not_triggered_for_fresh_message() {
        let mut queue = InjectQueue::new();
        queue
            .push(InjectMessage {
                from: "test".to_string(),
                text: "hello".to_string(),
                timestamp: 0,
                enqueued_at: Instant::now(),
            })
            .unwrap();
        assert!(
            queue.oldest_enqueued_at().unwrap().elapsed() < FORCE_INJECT_TIMEOUT,
            "Fresh message should not trigger force inject"
        );
    }

    #[test]
    fn throttle_coalesce_is_25ms() {
        assert_eq!(THROTTLE_COALESCE, Duration::from_millis(25));
    }

    #[test]
    fn idle_state_requires_all_conditions_for_inject() {
        let mut idle = IdleState::new();

        // Fresh state: no prompt, too recent
        assert!(!idle.should_inject(true));

        // Record prompt but output still too recent
        idle.record_output(true);
        assert!(!idle.should_inject(true));

        // Simulate time passing for output settle and idle threshold
        idle.last_output_at = Instant::now()
            .checked_sub(OUTPUT_SETTLE + Duration::from_millis(50))
            .unwrap();
        idle.last_user_input = Instant::now()
            .checked_sub(IDLE_THRESHOLD + Duration::from_millis(50))
            .unwrap();

        // Now should inject with messages in queue
        assert!(idle.should_inject(true));
        // But not without messages
        assert!(!idle.should_inject(false));
    }

    #[test]
    fn inject_queue_push_returns_length() {
        let mut queue = InjectQueue::new();
        assert!(queue.is_empty());

        let len = queue
            .push(InjectMessage {
                from: "a".to_string(),
                text: "msg1".to_string(),
                timestamp: 1,
                enqueued_at: Instant::now(),
            })
            .unwrap();
        assert_eq!(len, 1);

        let len = queue
            .push(InjectMessage {
                from: "b".to_string(),
                text: "msg2".to_string(),
                timestamp: 2,
                enqueued_at: Instant::now(),
            })
            .unwrap();
        assert_eq!(len, 2);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn inject_queue_fifo_order() {
        let mut queue = InjectQueue::new();
        queue
            .push(InjectMessage {
                from: "a".to_string(),
                text: "first".to_string(),
                timestamp: 1,
                enqueued_at: Instant::now(),
            })
            .unwrap();
        queue
            .push(InjectMessage {
                from: "b".to_string(),
                text: "second".to_string(),
                timestamp: 2,
                enqueued_at: Instant::now(),
            })
            .unwrap();

        assert_eq!(queue.pop().unwrap().text, "first");
        assert_eq!(queue.pop().unwrap().text, "second");
        assert!(queue.pop().is_none());
    }

    #[test]
    fn inject_queue_oldest_enqueued_at_tracks_front() {
        let mut queue = InjectQueue::new();
        assert!(queue.oldest_enqueued_at().is_none());

        let before = Instant::now();
        queue
            .push(InjectMessage {
                from: "a".to_string(),
                text: "first".to_string(),
                timestamp: 1,
                enqueued_at: Instant::now(),
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        queue
            .push(InjectMessage {
                from: "b".to_string(),
                text: "second".to_string(),
                timestamp: 2,
                enqueued_at: Instant::now(),
            })
            .unwrap();

        let oldest = queue.oldest_enqueued_at().unwrap();
        assert!(oldest >= before);

        queue.pop(); // remove first
        let new_oldest = queue.oldest_enqueued_at().unwrap();
        assert!(new_oldest > oldest);
    }

    #[test]
    fn inject_signal_notify_before_wait_still_wakes() {
        let signal = InjectSignal::new();
        signal.notify();
        // Already notified, wait_timeout should return true immediately
        let notified = signal.wait_timeout(Duration::from_millis(50));
        assert!(
            notified,
            "Pre-notified signal should return true immediately"
        );
    }

    // --- OSC 133 tests ---

    #[test]
    fn detect_osc133_bel_terminated() {
        let data = "some text\x1b]133;B\x07more text";
        let marks = detect_osc133(data);
        assert_eq!(marks, vec![Osc133Mark::PromptEnd]);
    }

    #[test]
    fn detect_osc133_st_terminated() {
        let data = "\x1b]133;A\x1b\\prompt text\x1b]133;B\x1b\\";
        let marks = detect_osc133(data);
        assert_eq!(marks, vec![Osc133Mark::PromptStart, Osc133Mark::PromptEnd]);
    }

    #[test]
    fn detect_osc133_all_marks() {
        let data = "\x1b]133;D\x07\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07";
        let marks = detect_osc133(data);
        assert_eq!(
            marks,
            vec![
                Osc133Mark::CommandEnd(None),
                Osc133Mark::PromptStart,
                Osc133Mark::PromptEnd,
                Osc133Mark::CommandStart,
            ]
        );
    }

    #[test]
    fn detect_osc133_no_marks_in_plain_text() {
        assert!(detect_osc133("hello world").is_empty());
        assert!(detect_osc133("133;B").is_empty());
        assert!(detect_osc133("\x1b[32mgreen\x1b[0m").is_empty());
    }

    #[test]
    fn detect_osc133_d_with_exit_status() {
        // zsh emits D;N with exit code
        let data = "\x1b]133;D;0\x07";
        let marks = detect_osc133(data);
        assert_eq!(marks, vec![Osc133Mark::CommandEnd(Some(0))]);

        let data = "\x1b]133;D;127\x07";
        let marks = detect_osc133(data);
        assert_eq!(marks, vec![Osc133Mark::CommandEnd(Some(127))]);

        // ST-terminated with exit status
        let data = "\x1b]133;D;1\x1b\\";
        let marks = detect_osc133(data);
        assert_eq!(marks, vec![Osc133Mark::CommandEnd(Some(1))]);
    }

    #[test]
    fn detect_osc133_d_without_exit_status() {
        // bash/fish emit plain D without exit code
        let data = "\x1b]133;D\x07";
        let marks = detect_osc133(data);
        assert_eq!(marks, vec![Osc133Mark::CommandEnd(None)]);
    }

    #[test]
    fn detect_osc133_unterminated_ignored() {
        // Missing terminator — should not match
        assert!(detect_osc133("\x1b]133;B").is_empty());
        assert!(detect_osc133("\x1b]133;B\x08").is_empty());
    }

    #[test]
    fn osc133_prompt_skips_output_settle() {
        let mut idle = IdleState::new();
        idle.last_user_input = Instant::now()
            .checked_sub(IDLE_THRESHOLD + Duration::from_millis(50))
            .unwrap();

        // OSC 133 prompt — should_inject immediately (no settle wait)
        idle.record_osc133_prompt();
        assert!(
            idle.should_inject(true),
            "OSC 133 should skip OUTPUT_SETTLE"
        );
        assert_eq!(idle.prompt_source, Some(PromptSource::Osc133));
    }

    #[test]
    fn heuristic_prompt_requires_output_settle() {
        let mut idle = IdleState::new();
        idle.last_user_input = Instant::now()
            .checked_sub(IDLE_THRESHOLD + Duration::from_millis(50))
            .unwrap();

        // Heuristic prompt — should NOT inject yet (output too recent)
        idle.record_output(true);
        assert!(
            !idle.should_inject(true),
            "Heuristic should require OUTPUT_SETTLE"
        );
        assert_eq!(idle.prompt_source, Some(PromptSource::Heuristic));

        // After settle time passes
        idle.last_output_at = Instant::now()
            .checked_sub(OUTPUT_SETTLE + Duration::from_millis(50))
            .unwrap();
        assert!(idle.should_inject(true));
    }

    #[test]
    fn osc133_not_downgraded_by_heuristic() {
        let mut idle = IdleState::new();
        idle.last_user_input = Instant::now()
            .checked_sub(IDLE_THRESHOLD + Duration::from_millis(50))
            .unwrap();

        // OSC 133 detected first
        idle.record_osc133_prompt();
        assert_eq!(idle.prompt_source, Some(PromptSource::Osc133));

        // Subsequent heuristic output should NOT downgrade to Heuristic
        idle.record_output(true);
        assert_eq!(idle.prompt_source, Some(PromptSource::Osc133));
    }

    #[test]
    fn reset_prompt_clears_source() {
        let mut idle = IdleState::new();
        idle.record_osc133_prompt();
        assert!(idle.last_output_has_prompt);
        assert_eq!(idle.prompt_source, Some(PromptSource::Osc133));

        idle.reset_prompt();
        assert!(!idle.last_output_has_prompt);
        assert_eq!(idle.prompt_source, None);
    }

    #[test]
    fn record_user_input_clears_osc133() {
        let mut idle = IdleState::new();
        idle.record_osc133_prompt();
        assert_eq!(idle.prompt_source, Some(PromptSource::Osc133));

        idle.record_user_input();
        assert!(!idle.last_output_has_prompt);
        assert_eq!(idle.prompt_source, None);
    }

    #[test]
    fn inject_queue_rejects_when_full() {
        let mut queue = InjectQueue::new();
        for i in 0..INJECT_QUEUE_CAPACITY {
            queue
                .push(InjectMessage {
                    from: "test".to_string(),
                    text: format!("msg{}", i),
                    timestamp: i as u64,
                    enqueued_at: Instant::now(),
                })
                .unwrap();
        }
        assert_eq!(queue.len(), INJECT_QUEUE_CAPACITY);

        let result = queue.push(InjectMessage {
            from: "test".to_string(),
            text: "overflow".to_string(),
            timestamp: 999,
            enqueued_at: Instant::now(),
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "inject queue full");
        assert_eq!(queue.len(), INJECT_QUEUE_CAPACITY);
    }
}
