//! File-backed mailbox for persistent, ACK-able message delivery.
//! See protocol/mailbox.md for the protocol spec.

pub mod config;
pub mod delivery;
pub mod error;
pub mod message;
pub mod notifier;
pub mod storage;

use config::MailboxConfig;
use error::MailboxError;
use message::{
    DeadLetterEntry, EnqueueAck, Message, MessageState, MessageSummary,
};
use storage::{unix_now, FileStorage, Locker, MailboxStorage, PidLocker};

/// File-backed mailbox for aterm (desktop standalone).
/// Composes FileStorage + PidLocker. Notification is handled externally
/// by InjectSignal (Condvar) — the mailbox does not own the wake mechanism.
pub struct FileMailbox {
    storage: FileStorage,
    locker: PidLocker,
    config: MailboxConfig,
}

impl FileMailbox {
    pub fn new(config: MailboxConfig) -> Self {
        let storage = FileStorage::new(config.root.clone());
        let locker = PidLocker::new(config.root.clone());
        Self {
            storage,
            locker,
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(MailboxConfig::default())
    }

    /// Enqueue a message. Idempotent: duplicate msg_id returns queued=false.
    pub fn enqueue(&self, msg: Message) -> Result<EnqueueAck, MailboxError> {
        let session_id = &msg.to;
        let _lock = self.locker.acquire(session_id, self.config.lock_timeout_ms)?;

        // Idempotency: skip if msg_id already has any state
        if self
            .storage
            .read_state(session_id, &msg.msg_id)?
            .is_some()
        {
            let pending = self.pending_count_locked(session_id)?;
            return Ok(EnqueueAck {
                msg_id: msg.msg_id,
                queued: false,
                pending,
            });
        }

        self.storage.write_message(session_id, &msg)?;
        self.storage
            .write_state(session_id, &msg.msg_id, MessageState::Pending)?;
        let pending = self.pending_count_locked(session_id)?;

        Ok(EnqueueAck {
            msg_id: msg.msg_id,
            queued: true,
            pending,
        })
    }

    /// Dequeue oldest pending message. Transitions to in_flight.
    pub fn dequeue(&self, session_id: &str) -> Result<Option<Message>, MailboxError> {
        let _lock = self.locker.acquire(session_id, self.config.lock_timeout_ms)?;

        let messages = self.storage.read_messages(session_id)?;
        for msg in messages {
            let state = self.storage.read_state(session_id, &msg.msg_id)?;
            if state == Some(MessageState::Pending) {
                self.storage.write_state(
                    session_id,
                    &msg.msg_id,
                    MessageState::InFlight,
                )?;
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }

    /// ACK: mark message as successfully delivered.
    pub fn ack(&self, session_id: &str, msg_id: &str) -> Result<(), MailboxError> {
        let _lock = self.locker.acquire(session_id, self.config.lock_timeout_ms)?;

        match self.storage.read_state(session_id, msg_id)? {
            Some(MessageState::Acked) => Ok(()), // idempotent
            Some(MessageState::InFlight) => {
                self.storage
                    .write_state(session_id, msg_id, MessageState::Acked)?;
                self.storage.remove_message(session_id, msg_id)?;
                Ok(())
            }
            Some(state) => Err(MailboxError::TerminalState {
                msg_id: msg_id.to_string(),
                state,
            }),
            None => Err(MailboxError::NotFound(msg_id.to_string())),
        }
    }

    /// NACK: delivery failed. Retry (re-enqueue with attempt+1) or dead-letter.
    pub fn nack(
        &self,
        session_id: &str,
        msg_id: &str,
        reason: &str,
    ) -> Result<(), MailboxError> {
        let _lock = self.locker.acquire(session_id, self.config.lock_timeout_ms)?;

        let messages = self.storage.read_messages(session_id)?;
        let msg = messages
            .iter()
            .find(|m| m.msg_id == msg_id)
            .ok_or_else(|| MailboxError::NotFound(msg_id.to_string()))?;

        self.storage
            .write_state(session_id, msg_id, MessageState::Nacked)?;

        if msg.attempt + 1 < self.config.max_retries {
            // Re-enqueue with incremented attempt
            let mut retry_msg = msg.clone();
            retry_msg.attempt += 1;
            self.storage.remove_message(session_id, msg_id)?;
            self.storage.write_message(session_id, &retry_msg)?;
            self.storage
                .write_state(session_id, msg_id, MessageState::Pending)?;
        } else {
            // Dead letter
            let entry = DeadLetterEntry {
                msg_id: msg.msg_id.clone(),
                from: msg.from.clone(),
                to: msg.to.clone(),
                payload: msg.payload.clone(),
                reason: reason.to_string(),
                failed_at: unix_now(),
                attempts: msg.attempt + 1,
                created_at: msg.created_at,
            };
            self.storage.write_dead_letter(session_id, &entry)?;
            self.storage
                .write_state(session_id, msg_id, MessageState::DeadLetter)?;
            self.storage.remove_message(session_id, msg_id)?;
        }

        Ok(())
    }

    /// Peek: list non-terminal messages with state.
    pub fn peek(
        &self,
        session_id: &str,
    ) -> Result<Vec<MessageSummary>, MailboxError> {
        let messages = self.storage.read_messages(session_id)?;
        let mut summaries = Vec::new();
        for msg in messages {
            let state = self
                .storage
                .read_state(session_id, &msg.msg_id)?
                .unwrap_or(MessageState::Pending);
            if !state.is_terminal() {
                summaries.push(MessageSummary {
                    msg_id: msg.msg_id,
                    from: msg.from,
                    created_at: msg.created_at,
                    attempt: msg.attempt,
                    state,
                });
            }
        }
        Ok(summaries)
    }

    pub fn purge(&self, session_id: &str) -> Result<(), MailboxError> {
        self.storage.purge_session(session_id)
    }

    pub fn peek_dead_letter(
        &self,
        session_id: &str,
    ) -> Result<Vec<DeadLetterEntry>, MailboxError> {
        self.storage.read_dead_letters(session_id)
    }

    pub fn purge_dead_letter(&self, session_id: &str) -> Result<(), MailboxError> {
        let path = self
            .storage
            .session_dir(session_id)
            .join("dead-letter.jsonl");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Count pending messages (caller already holds lock).
    fn pending_count_locked(&self, session_id: &str) -> Result<usize, MailboxError> {
        let messages = self.storage.read_messages(session_id)?;
        let mut count = 0;
        for msg in &messages {
            if self.storage.read_state(session_id, &msg.msg_id)?
                == Some(MessageState::Pending)
            {
                count += 1;
            }
        }
        Ok(count)
    }
}
