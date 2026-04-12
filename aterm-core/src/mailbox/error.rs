use std::fmt;

use super::message::MessageState;

#[derive(Debug)]
pub enum MailboxError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    /// msg_id or session_id not found.
    NotFound(String),
    /// Lock timeout: another process holds the session lock.
    Locked(String),
    /// Message is in a terminal state — cannot transition.
    TerminalState { msg_id: String, state: MessageState },
}

impl fmt::Display for MailboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "mailbox I/O: {}", e),
            Self::Serde(e) => write!(f, "mailbox serde: {}", e),
            Self::NotFound(id) => write!(f, "mailbox not found: {}", id),
            Self::Locked(id) => write!(f, "mailbox locked: {}", id),
            Self::TerminalState { msg_id, state } => {
                write!(f, "mailbox: {} is in terminal state {:?}", msg_id, state)
            }
        }
    }
}

impl std::error::Error for MailboxError {}

impl From<std::io::Error> for MailboxError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for MailboxError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}
