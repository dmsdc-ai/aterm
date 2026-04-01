use std::process::Command;

/// Optional bridge to telepty daemon.
/// All methods are fire-and-forget — failures are logged but never block aterm.
pub struct TeleptyBridge {
    daemon_url: String,
}

impl TeleptyBridge {
    /// Try to connect to telepty daemon. Returns None if unavailable.
    pub fn try_connect() -> Option<Self> {
        let bridge = Self {
            daemon_url: "http://127.0.0.1:3848".to_string(),
        };

        // Quick health check
        let output = Command::new("curl")
            .args([
                "-s",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "--max-time",
                "1",
                &format!("{}/api/sessions", bridge.daemon_url),
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let code = String::from_utf8_lossy(&out.stdout);
                if code.starts_with('2') {
                    eprintln!("[telepty-bridge] connected to daemon");
                    Some(bridge)
                } else {
                    eprintln!(
                        "[telepty-bridge] daemon returned {}, running standalone",
                        code
                    );
                    None
                }
            }
            _ => {
                eprintln!("[telepty-bridge] daemon not available, running standalone");
                None
            }
        }
    }

    /// Register an aterm workspace with telepty. Fire-and-forget.
    pub fn register(
        &self,
        session_id: &str,
        alias: &str,
        command: &str,
        cwd: &str,
        socket_path: &str,
    ) {
        let payload = serde_json::json!({
            "session_id": session_id,
            "alias": alias,
            "command": command,
            "cwd": cwd,
            "delivery_type": "aterm",
            "delivery": {
                "transport": "unix_socket",
                "address": socket_path,
            },
            "term_program": "aterm",
            "term": "xterm-256color",
        });

        let url = format!("{}/api/sessions/register", self.daemon_url);
        let body = payload.to_string();

        // Fire-and-forget in background thread
        std::thread::spawn(move || {
            let _ = Command::new("curl")
                .args([
                    "-s",
                    "-X",
                    "POST",
                    &url,
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                    "--max-time",
                    "2",
                ])
                .output();
        });
    }

    /// Deregister a workspace from telepty. Fire-and-forget.
    pub fn deregister(&self, session_id: &str) {
        let url = format!("{}/api/sessions/{}", self.daemon_url, session_id);

        std::thread::spawn(move || {
            let _ = Command::new("curl")
                .args(["-s", "-X", "DELETE", &url, "--max-time", "2"])
                .output();
        });
    }
}
