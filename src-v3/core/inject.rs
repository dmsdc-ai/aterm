use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const INJECTOR_POLL: Duration = Duration::from_millis(500);
pub const IDLE_THRESHOLD: Duration = Duration::from_secs(2);
pub const OUTPUT_SETTLE: Duration = Duration::from_secs(1);
const PROMPT_PATTERNS: &[&str] = &["❯", "> ", "$ ", "% "];

pub type SharedInjectQueue = Arc<Mutex<InjectQueue>>;

#[derive(Debug, Clone)]
pub struct InjectMessage {
    pub from: String,
    pub text: String,
    pub timestamp: u64,
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

    pub fn push(&mut self, message: InjectMessage) -> usize {
        self.messages.push_back(message);
        self.messages.len()
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
        }
    }

    pub fn record_user_input(&mut self) {
        self.last_user_input = Instant::now();
        self.last_output_has_prompt = false;
    }

    pub fn record_output(&mut self, has_prompt: bool) {
        let now = Instant::now();
        self.last_output_at = now;
        self.last_output_has_prompt = has_prompt;
    }

    pub fn should_inject(&self, queue_has_messages: bool) -> bool {
        self.last_output_has_prompt
            && self.last_output_at.elapsed() >= OUTPUT_SETTLE
            && self.last_user_input.elapsed() >= IDLE_THRESHOLD
            && queue_has_messages
    }
}

pub fn has_prompt_pattern(data: &str) -> bool {
    PROMPT_PATTERNS.iter().any(|pattern| data.contains(pattern))
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
    status: Arc<Mutex<String>>,
) {
    loop {
        thread::sleep(INJECTOR_POLL);

        if let Ok(current) = status.lock() {
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

        if !should_inject {
            continue;
        }

        let message = inject_queue.lock().ok().and_then(|mut queue| queue.pop());
        let Some(message) = message else {
            continue;
        };

        if let Ok(mut handle) = writer.lock() {
            let payload = with_terminal_enter(&message.text);
            let _ = handle.write_all(payload.as_bytes());
            let _ = handle.flush();
        }

        if let Ok(mut idle) = idle_state.lock() {
            idle.last_output_has_prompt = false;
        }
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
