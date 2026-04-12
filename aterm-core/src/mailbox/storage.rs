use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::error::MailboxError;
use super::message::{
    CompactStats, DeadLetterEntry, Message, MessageState, StateEntry,
};

/// Persistence backend trait.
pub trait MailboxStorage: Send + Sync {
    fn write_message(&self, session_id: &str, msg: &Message) -> Result<(), MailboxError>;
    fn read_messages(&self, session_id: &str) -> Result<Vec<Message>, MailboxError>;
    fn remove_message(&self, session_id: &str, msg_id: &str) -> Result<(), MailboxError>;
    fn write_state(
        &self,
        session_id: &str,
        msg_id: &str,
        state: MessageState,
    ) -> Result<(), MailboxError>;
    fn read_state(
        &self,
        session_id: &str,
        msg_id: &str,
    ) -> Result<Option<MessageState>, MailboxError>;
    fn write_dead_letter(
        &self,
        session_id: &str,
        entry: &DeadLetterEntry,
    ) -> Result<(), MailboxError>;
    fn read_dead_letters(
        &self,
        session_id: &str,
    ) -> Result<Vec<DeadLetterEntry>, MailboxError>;
    fn compact(&self, session_id: &str) -> Result<CompactStats, MailboxError>;
    fn purge_session(&self, session_id: &str) -> Result<(), MailboxError>;
    fn list_sessions(&self) -> Result<Vec<String>, MailboxError>;
}

/// RAII lock guard — releases on Drop.
pub trait LockGuard: Send {}

/// Concurrency lock trait.
pub trait Locker: Send + Sync {
    fn acquire(
        &self,
        session_id: &str,
        timeout_ms: u64,
    ) -> Result<Box<dyn LockGuard>, MailboxError>;
    fn is_stale(&self, lock_path: &Path) -> bool;
}

// ─── FileStorage ───

pub struct FileStorage {
    root: PathBuf,
}

impl FileStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    fn ensure_dir(&self, session_id: &str) -> Result<PathBuf, MailboxError> {
        let dir = self.session_dir(session_id);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn append_jsonl<T: serde::Serialize>(path: &Path, item: &T) -> Result<(), MailboxError> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        serde_json::to_writer(&mut f, item)?;
        f.write_all(b"\n")?;
        Ok(())
    }

    pub fn read_jsonl<T: serde::de::DeserializeOwned>(
        path: &Path,
    ) -> Result<Vec<T>, MailboxError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(path)?;
        Ok(content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }
}

impl MailboxStorage for FileStorage {
    fn write_message(&self, session_id: &str, msg: &Message) -> Result<(), MailboxError> {
        let dir = self.ensure_dir(session_id)?;
        Self::append_jsonl(&dir.join("inbox.jsonl"), msg)
    }

    fn read_messages(&self, session_id: &str) -> Result<Vec<Message>, MailboxError> {
        let path = self.session_dir(session_id).join("inbox.jsonl");
        Self::read_jsonl(&path)
    }

    fn remove_message(&self, session_id: &str, msg_id: &str) -> Result<(), MailboxError> {
        let dir = self.session_dir(session_id);
        let path = dir.join("inbox.jsonl");
        if !path.exists() {
            return Ok(());
        }
        let messages: Vec<Message> = Self::read_jsonl(&path)?;
        let tmp = dir.join("inbox.jsonl.tmp");
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            for msg in &messages {
                if msg.msg_id != msg_id {
                    serde_json::to_writer(&mut f, msg)?;
                    f.write_all(b"\n")?;
                }
            }
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn write_state(
        &self,
        session_id: &str,
        msg_id: &str,
        state: MessageState,
    ) -> Result<(), MailboxError> {
        let dir = self.ensure_dir(session_id)?;
        let entry = StateEntry {
            msg_id: msg_id.to_string(),
            state,
            ts: unix_now(),
        };
        Self::append_jsonl(&dir.join("state.jsonl"), &entry)
    }

    fn read_state(
        &self,
        session_id: &str,
        msg_id: &str,
    ) -> Result<Option<MessageState>, MailboxError> {
        let path = self.session_dir(session_id).join("state.jsonl");
        let entries: Vec<StateEntry> = Self::read_jsonl(&path)?;
        Ok(entries
            .iter()
            .rev()
            .find(|e| e.msg_id == msg_id)
            .map(|e| e.state))
    }

    fn write_dead_letter(
        &self,
        session_id: &str,
        entry: &DeadLetterEntry,
    ) -> Result<(), MailboxError> {
        let dir = self.ensure_dir(session_id)?;
        Self::append_jsonl(&dir.join("dead-letter.jsonl"), entry)
    }

    fn read_dead_letters(
        &self,
        session_id: &str,
    ) -> Result<Vec<DeadLetterEntry>, MailboxError> {
        let path = self.session_dir(session_id).join("dead-letter.jsonl");
        Self::read_jsonl(&path)
    }

    fn compact(&self, session_id: &str) -> Result<CompactStats, MailboxError> {
        let dir = self.session_dir(session_id);
        let inbox_path = dir.join("inbox.jsonl");
        let state_path = dir.join("state.jsonl");

        let messages: Vec<Message> = Self::read_jsonl(&inbox_path)?;
        let states: Vec<StateEntry> = Self::read_jsonl(&state_path)?;

        // Build latest state per msg_id
        let mut latest: HashMap<String, MessageState> = HashMap::new();
        for entry in &states {
            latest.insert(entry.msg_id.clone(), entry.state);
        }

        // Keep only non-terminal messages
        let active: Vec<&Message> = messages
            .iter()
            .filter(|m| {
                latest
                    .get(&m.msg_id)
                    .map(|s| !s.is_terminal())
                    .unwrap_or(true)
            })
            .collect();

        let active_ids: HashSet<&str> =
            active.iter().map(|m| m.msg_id.as_str()).collect();
        let active_states: Vec<&StateEntry> = states
            .iter()
            .filter(|e| active_ids.contains(e.msg_id.as_str()))
            .collect();

        let msgs_removed = messages.len() - active.len();
        let states_removed = states.len() - active_states.len();

        if msgs_removed > 0 || states_removed > 0 {
            // Atomic rewrite via tmp + rename
            let tmp_inbox = dir.join("inbox.jsonl.tmp");
            let tmp_state = dir.join("state.jsonl.tmp");
            {
                let mut f = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&tmp_inbox)?;
                for msg in &active {
                    serde_json::to_writer(&mut f, msg)?;
                    f.write_all(b"\n")?;
                }
            }
            {
                let mut f = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&tmp_state)?;
                for entry in &active_states {
                    serde_json::to_writer(&mut f, entry)?;
                    f.write_all(b"\n")?;
                }
            }
            fs::rename(&tmp_inbox, &inbox_path)?;
            fs::rename(&tmp_state, &state_path)?;
        }

        Ok(CompactStats {
            messages_removed: msgs_removed,
            states_removed,
        })
    }

    fn purge_session(&self, session_id: &str) -> Result<(), MailboxError> {
        let dir = self.session_dir(session_id);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    fn list_sessions(&self) -> Result<Vec<String>, MailboxError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    if !name.starts_with('.') {
                        sessions.push(name.to_string());
                    }
                }
            }
        }
        Ok(sessions)
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── PidLocker ───

/// Advisory PID-file lock. No libc — uses `kill -0` via Command.
pub struct PidLocker {
    root: PathBuf,
}

impl PidLocker {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

struct PidLockGuard {
    path: PathBuf,
}

impl LockGuard for PidLockGuard {}

impl Drop for PidLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Locker for PidLocker {
    fn acquire(
        &self,
        session_id: &str,
        timeout_ms: u64,
    ) -> Result<Box<dyn LockGuard>, MailboxError> {
        let lock_dir = self.root.join(session_id);
        fs::create_dir_all(&lock_dir)?;
        let lock_path = lock_dir.join(".lock");
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(Box::new(PidLockGuard {
                        path: lock_path,
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if self.is_stale(&lock_path) {
                        let _ = fs::remove_file(&lock_path);
                        continue;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(MailboxError::Locked(session_id.to_string()));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(MailboxError::Io(e)),
            }
        }
    }

    fn is_stale(&self, lock_path: &Path) -> bool {
        let Ok(content) = fs::read_to_string(lock_path) else {
            return true;
        };
        let Ok(pid) = content.trim().parse::<u32>() else {
            return true;
        };
        // kill -0: check process exists without sending a signal
        match Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
        {
            Ok(o) => !o.status.success(),
            Err(_) => true,
        }
    }
}
