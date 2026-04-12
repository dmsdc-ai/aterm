use super::error::MailboxError;

/// Wake mechanism: tells a receiver that a new message is available.
pub trait Notifier: Send + Sync {
    fn notify_new_message(&self, session_id: &str) -> Result<(), MailboxError>;
}

/// No-op notifier for standalone use (aterm without telepty daemon).
/// Wake is handled by InjectSignal (Condvar) separately.
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify_new_message(&self, _session_id: &str) -> Result<(), MailboxError> {
        Ok(())
    }
}
