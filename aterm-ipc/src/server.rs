use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aterm_session::action::{ActionResponse, SessionAction};

use crate::auth;

struct Subscriber {
    stream: UnixStream,
    events: Vec<String>,
    failed_writes: u32,
    last_write: Instant,
    last_seq: u64,
}

pub struct IpcServer {
    socket_path: String,
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
}

impl IpcServer {
    /// Start the embedded IPC server on a background thread.
    /// `dispatcher` is called for each incoming SessionAction.
    pub fn start(
        socket_path: &str,
        dispatcher: Arc<dyn Fn(SessionAction) -> ActionResponse + Send + Sync>,
    ) -> std::io::Result<Self> {
        // Remove stale socket
        let _ = std::fs::remove_file(socket_path);

        let listener = UnixListener::bind(socket_path)?;

        // Set socket permissions: owner-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));
        }

        eprintln!("[aterm-ipc] listening on {}", socket_path);

        let subscribers: Arc<Mutex<Vec<Subscriber>>> = Arc::new(Mutex::new(Vec::new()));
        let subs_clone = subscribers.clone();
        let path = socket_path.to_string();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };

                // Kernel-level UID auth
                if !auth::verify_peer(&stream) {
                    eprintln!("[aterm-ipc] rejected connection: peer UID mismatch");
                    continue;
                }

                let dispatcher = dispatcher.clone();
                let subs = subs_clone.clone();
                std::thread::spawn(move || {
                    handle_connection(stream, dispatcher.as_ref(), subs);
                });
            }
            eprintln!("[aterm-ipc] listener closed: {}", path);
        });

        Ok(Self {
            socket_path: socket_path.to_string(),
            subscribers,
        })
    }

    /// Broadcast a workspace event JSON to all matching subscribers.
    /// Non-blocking writes; drops subscribers after 3 consecutive failures.
    /// `snapshot_fn` is called lazily only when a subscriber has a sequence gap.
    pub fn broadcast(&self, event_json: &str, snapshot_fn: &dyn Fn() -> String) {
        let parsed = serde_json::from_str::<serde_json::Value>(event_json).ok();
        let event_type = parsed
            .as_ref()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from));
        let event_seq = parsed
            .as_ref()
            .and_then(|v| v.get("seq").and_then(|s| s.as_u64()))
            .unwrap_or(0);

        let mut subs = match self.subscribers.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let now = Instant::now();
        // Lazily generated snapshot — only computed if any subscriber has a gap
        let mut cached_snapshot: Option<String> = None;

        subs.retain_mut(|sub| {
            // Filter: skip events the subscriber didn't ask for
            if !sub.events.is_empty() {
                if let Some(ref etype) = event_type {
                    if !sub.events.iter().any(|e| e.eq_ignore_ascii_case(etype)) {
                        return true;
                    }
                }
            }

            // Gap detection: if subscriber missed events, send re-snapshot first
            if event_seq > 0 && sub.last_seq > 0 && event_seq > sub.last_seq + 1 {
                eprintln!(
                    "[aterm-ipc] gap detected: subscriber last_seq={}, event_seq={} — sending re-snapshot",
                    sub.last_seq, event_seq
                );
                let snapshot = cached_snapshot
                    .get_or_insert_with(|| snapshot_fn());
                let snap_line = format!("{}\n", snapshot.trim());
                let _ = sub.stream.set_nonblocking(true);
                let _ = sub
                    .stream
                    .write_all(snap_line.as_bytes())
                    .and_then(|_| sub.stream.flush());
                let _ = sub.stream.set_nonblocking(false);
            }

            let line = format!("{}\n", event_json.trim());

            // Set non-blocking for write attempt
            let _ = sub.stream.set_nonblocking(true);
            let result = sub
                .stream
                .write_all(line.as_bytes())
                .and_then(|_| sub.stream.flush());
            let _ = sub.stream.set_nonblocking(false);

            match result {
                Ok(()) => {
                    sub.failed_writes = 0;
                    sub.last_write = now;
                    if event_seq > 0 {
                        sub.last_seq = event_seq;
                    }
                    true
                }
                Err(_) => {
                    sub.failed_writes += 1;
                    if sub.failed_writes >= 3 {
                        eprintln!("[aterm-ipc] dropping subscriber after 3 failed writes");
                        false
                    } else {
                        true // keep for retry
                    }
                }
            }
        });
    }

    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
        eprintln!("[aterm-ipc] removed socket: {}", self.socket_path);
    }
}

fn handle_connection(
    stream: UnixStream,
    dispatcher: &(dyn Fn(SessionAction) -> ActionResponse + Send + Sync),
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
) {
    let reader = BufReader::new(&stream);
    let mut writer = &stream;
    let mut subscribed = false;

    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }

        // After subscribing, ignore further input — just block until disconnect
        if subscribed {
            continue;
        }

        match serde_json::from_str::<SessionAction>(&line) {
            Ok(SessionAction::Subscribe { ref events }) => {
                // Dispatch to app to get workspace snapshot + current seq
                let resp = dispatcher(SessionAction::Subscribe {
                    events: events.clone(),
                });
                let resp_json = serde_json::to_string(&resp).unwrap_or_default();
                if writeln!(writer, "{}", resp_json).is_err() {
                    return;
                }

                // Extract seq from snapshot response for initial last_seq
                let initial_seq = match &resp {
                    ActionResponse::Data { data } => {
                        data.get("data")
                            .or(Some(data))
                            .and_then(|d| d.get("seq"))
                            .and_then(|s| s.as_u64())
                            .unwrap_or(0)
                    }
                    _ => 0,
                };

                // Clone stream for broadcast writes
                if let Ok(sub_stream) = stream.try_clone() {
                    if let Ok(mut subs) = subscribers.lock() {
                        subs.push(Subscriber {
                            stream: sub_stream,
                            events: events.clone(),
                            failed_writes: 0,
                            last_write: Instant::now(),
                            last_seq: initial_seq,
                        });
                    }
                }

                subscribed = true;
                // Don't return — let the for-loop block on the next read,
                // keeping the thread alive until the client disconnects.
            }
            Ok(action) => {
                let response = dispatcher(action);
                let resp_json = serde_json::to_string(&response).unwrap_or_else(|_| {
                    r#"{"status":"Error","message":"serialize failed"}"#.to_string()
                });
                if writeln!(writer, "{}", resp_json).is_err() {
                    break;
                }
            }
            Err(e) => {
                let response = ActionResponse::error(format!("parse error: {}", e));
                let resp_json = serde_json::to_string(&response).unwrap_or_else(|_| {
                    r#"{"status":"Error","message":"serialize failed"}"#.to_string()
                });
                if writeln!(writer, "{}", resp_json).is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Build a server around one subscriber without binding a socket.
    /// The path does not exist, so `Drop` removes nothing.
    fn server_with(name: &str, stream: UnixStream, events: Vec<String>) -> IpcServer {
        let socket_path = std::env::temp_dir()
            .join(format!("aterm-ipc-test-{}-{}.sock", std::process::id(), name))
            .to_string_lossy()
            .into_owned();
        IpcServer {
            socket_path,
            subscribers: Arc::new(Mutex::new(vec![Subscriber {
                stream,
                events,
                failed_writes: 0,
                last_write: Instant::now(),
                last_seq: 0,
            }])),
        }
    }

    fn age_last_write(server: &IpcServer) {
        server.subscribers.lock().unwrap()[0].last_write =
            Instant::now().checked_sub(Duration::from_secs(31)).unwrap();
    }

    fn subscriber_count(server: &IpcServer) -> usize {
        server.subscribers.lock().unwrap().len()
    }

    /// Socket pair whose peer read times out after 1 s. The timeout is set
    /// up front: macOS rejects it (EINVAL) once the other end is closed.
    fn pair_with_1s_read() -> (UnixStream, UnixStream) {
        let (sub, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        (sub, peer)
    }

    fn read_line(peer: &UnixStream) -> String {
        let mut line = String::new();
        BufReader::new(peer).read_line(&mut line).unwrap();
        line
    }

    #[test]
    fn quiet_subscriber_still_receives_event() {
        let (sub, peer) = pair_with_1s_read();
        let server = server_with("quiet", sub, Vec::new());
        age_last_write(&server);

        server.broadcast(r#"{"type":"StatusChanged","seq":0}"#, &|| String::new());

        let line = read_line(&peer);
        assert!(line.contains("StatusChanged"), "got {:?}", line);
        assert_eq!(subscriber_count(&server), 1);
    }

    #[test]
    fn filtered_subscriber_survives_quiet_period() {
        let (sub, peer) = pair_with_1s_read();
        let server = server_with("filtered", sub, vec!["ShellReady".to_string()]);

        server.broadcast(r#"{"type":"StatusChanged","seq":0}"#, &|| String::new());
        age_last_write(&server);
        server.broadcast(r#"{"type":"ShellReady","seq":0}"#, &|| String::new());

        let line = read_line(&peer);
        assert!(line.contains("ShellReady"), "got {:?}", line);
        assert_eq!(subscriber_count(&server), 1);
    }

    #[test]
    fn closed_peer_evicted_after_three_failures() {
        let (sub, peer) = UnixStream::pair().unwrap();
        let server = server_with("closed", sub, Vec::new());
        drop(peer);

        for expected in [1, 1, 0] {
            server.broadcast(r#"{"type":"StatusChanged","seq":0}"#, &|| String::new());
            assert_eq!(subscriber_count(&server), expected);
        }
    }
}
