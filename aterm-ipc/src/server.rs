use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::Arc;

use aterm_session::action::{ActionResponse, SessionAction};

use crate::auth;

pub struct IpcServer {
    socket_path: String,
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
            let _ = std::fs::set_permissions(
                socket_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }

        eprintln!("[aterm-ipc] listening on {}", socket_path);

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
                std::thread::spawn(move || {
                    handle_connection(stream, dispatcher.as_ref());
                });
            }
            eprintln!("[aterm-ipc] listener closed: {}", path);
        });

        Ok(Self {
            socket_path: socket_path.to_string(),
        })
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
    stream: std::os::unix::net::UnixStream,
    dispatcher: &(dyn Fn(SessionAction) -> ActionResponse + Send + Sync),
) {
    let reader = BufReader::new(&stream);
    let mut writer = &stream;

    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }

        let response = match serde_json::from_str::<SessionAction>(&line) {
            Ok(action) => dispatcher(action),
            Err(e) => ActionResponse::error(format!("parse error: {}", e)),
        };

        let resp_json = serde_json::to_string(&response).unwrap_or_else(|_| {
            r#"{"status":"Error","message":"serialize failed"}"#.to_string()
        });

        if writeln!(writer, "{}", resp_json).is_err() {
            break;
        }
    }
}
