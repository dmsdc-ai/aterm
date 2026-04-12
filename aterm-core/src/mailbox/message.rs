use serde::{Deserialize, Serialize};

/// Message state machine (protocol/mailbox.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageState {
    Pending,
    InFlight,
    Acked,
    Nacked,
    DeadLetter,
    Expired,
}

impl MessageState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Acked | Self::DeadLetter | Self::Expired)
    }
}

/// Core message type (protocol/mailbox.md §2).
/// Field names use snake_case; Node.js side accepts both via alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub msg_id: String,
    pub from: String,
    pub to: String,
    pub payload: String,
    pub created_at: u64,
    pub attempt: u32,
}

/// ACK returned by enqueue (protocol/mailbox.md §4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueueAck {
    pub msg_id: String,
    pub queued: bool,
    pub pending: usize,
}

/// Summary for peek (non-payload view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub msg_id: String,
    pub from: String,
    pub created_at: u64,
    pub attempt: u32,
    pub state: MessageState,
}

/// Dead letter entry (protocol/mailbox.md §6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterEntry {
    pub msg_id: String,
    pub from: String,
    pub to: String,
    pub payload: String,
    pub reason: String,
    pub failed_at: u64,
    pub attempts: u32,
    pub created_at: u64,
}

/// Internal: state transition log entry for state.jsonl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEntry {
    pub msg_id: String,
    pub state: MessageState,
    pub ts: u64,
}

/// Stats from compaction.
#[derive(Debug, Clone, Default)]
pub struct CompactStats {
    pub messages_removed: usize,
    pub states_removed: usize,
}
