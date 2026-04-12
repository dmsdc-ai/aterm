use super::error::MailboxError;
use super::message::{DeadLetterEntry, MessageState};
use super::storage::{unix_now, FileStorage, MailboxStorage};

/// Sweep stats for observability.
#[derive(Debug, Default)]
pub struct SweepStats {
    pub nacked: usize,
    pub expired: usize,
    pub dead_lettered: usize,
}

/// Background delivery engine: in-flight timeout + TTL sweep.
/// Called periodically (e.g., every 10s) or on-demand.
pub struct DeliveryEngine {
    storage: FileStorage,
    max_retries: u32,
    ttl_secs: u64,
    inflight_timeout_secs: u64,
}

impl DeliveryEngine {
    pub fn new(
        storage: FileStorage,
        max_retries: u32,
        ttl_secs: u64,
        inflight_timeout_secs: u64,
    ) -> Self {
        Self {
            storage,
            max_retries,
            ttl_secs,
            inflight_timeout_secs,
        }
    }

    /// Sweep all sessions for expired and timed-out messages.
    pub fn sweep(&self) -> Result<SweepStats, MailboxError> {
        let sessions = self.storage.list_sessions()?;
        let mut stats = SweepStats::default();
        for session_id in &sessions {
            let session_stats = self.sweep_session(session_id)?;
            stats.nacked += session_stats.nacked;
            stats.expired += session_stats.expired;
            stats.dead_lettered += session_stats.dead_lettered;
        }
        Ok(stats)
    }

    fn sweep_session(&self, session_id: &str) -> Result<SweepStats, MailboxError> {
        let now = unix_now();
        let messages = self.storage.read_messages(session_id)?;
        let mut stats = SweepStats::default();

        // Read all state entries for timestamp lookups
        let state_path = self.storage.session_dir(session_id).join("state.jsonl");
        let all_states: Vec<super::message::StateEntry> =
            FileStorage::read_jsonl(&state_path)?;

        for msg in &messages {
            let state = self.storage.read_state(session_id, &msg.msg_id)?;

            match state {
                Some(MessageState::InFlight) => {
                    // Find when it went in_flight
                    let inflight_ts = all_states
                        .iter()
                        .rev()
                        .find(|e| {
                            e.msg_id == msg.msg_id && e.state == MessageState::InFlight
                        })
                        .map(|e| e.ts)
                        .unwrap_or(msg.created_at);

                    if inflight_ts + self.inflight_timeout_secs < now {
                        self.storage.write_state(
                            session_id,
                            &msg.msg_id,
                            MessageState::Nacked,
                        )?;
                        if msg.attempt + 1 < self.max_retries {
                            let mut retry_msg = msg.clone();
                            retry_msg.attempt += 1;
                            self.storage.remove_message(session_id, &msg.msg_id)?;
                            self.storage.write_message(session_id, &retry_msg)?;
                            self.storage.write_state(
                                session_id,
                                &retry_msg.msg_id,
                                MessageState::Pending,
                            )?;
                            stats.nacked += 1;
                        } else {
                            self.dead_letter(session_id, msg, "inflight_timeout")?;
                            stats.dead_lettered += 1;
                        }
                    }
                }
                Some(MessageState::Pending) | None => {
                    // TTL check
                    if msg.created_at + self.ttl_secs < now {
                        self.storage.write_state(
                            session_id,
                            &msg.msg_id,
                            MessageState::Expired,
                        )?;
                        self.storage.remove_message(session_id, &msg.msg_id)?;
                        stats.expired += 1;
                    }
                }
                _ => {}
            }
        }

        Ok(stats)
    }

    fn dead_letter(
        &self,
        session_id: &str,
        msg: &super::message::Message,
        reason: &str,
    ) -> Result<(), MailboxError> {
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
        self.storage.write_state(
            session_id,
            &msg.msg_id,
            MessageState::DeadLetter,
        )?;
        self.storage.remove_message(session_id, &msg.msg_id)?;
        Ok(())
    }
}
