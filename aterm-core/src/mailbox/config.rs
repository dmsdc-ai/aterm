use std::path::PathBuf;

/// Resolve mailbox root directory.
/// Priority: AIGENTRY_MAILBOX_DIR env > ~/.aigentry/mailbox/
pub fn platform_mailbox_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("AIGENTRY_MAILBOX_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".aigentry/mailbox")
}

#[derive(Debug, Clone)]
pub struct MailboxConfig {
    pub root: PathBuf,
    pub max_retries: u32,
    pub base_backoff_secs: u64,
    pub ttl_secs: u64,
    pub inflight_timeout_secs: u64,
    pub compaction_threshold: usize,
    pub lock_timeout_ms: u64,
}

impl Default for MailboxConfig {
    fn default() -> Self {
        Self {
            root: platform_mailbox_dir(),
            max_retries: 3,
            base_backoff_secs: 5,
            ttl_secs: 86_400,
            inflight_timeout_secs: 30,
            compaction_threshold: 100,
            lock_timeout_ms: 500,
        }
    }
}
