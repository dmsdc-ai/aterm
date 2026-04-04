use std::process::Command;
use std::thread;
use std::time::Duration;

/// Optional bridge to telepty daemon.
/// All methods are fire-and-forget — failures are logged but never block aterm.
pub struct TeleptyBridge {
    daemon_url: String,
}

impl TeleptyBridge {
    /// Try to connect to telepty daemon. Returns None if unavailable.
    /// Retries up to 3 times with 500ms between attempts.
    pub fn try_connect() -> Option<Self> {
        let port = std::env::var("ATERM_TELEPTY_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3848);
        let bridge = Self {
            daemon_url: format!("http://127.0.0.1:{}", port),
        };

        for attempt in 1..=3 {
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
                        let installed = Self::detect_version();
                        let mut daemon_ver = Self::detect_daemon_version(&bridge.daemon_url);

                        // Auto-restart if installed CLI is newer than running daemon
                        if let (Some(ref inst), Some(ref dmn)) = (&installed, &daemon_ver) {
                            if inst != dmn {
                                log_stderr!(
                                    "[telepty-bridge] daemon v{} outdated (installed v{}), restarting...",
                                    dmn, inst
                                );
                                if Self::restart_daemon() {
                                    // Wait for daemon to come back up after restart.
                                    // One-time cost at app launch, not in a hot path.
                                    thread::sleep(Duration::from_secs(2));
                                    daemon_ver = Self::detect_daemon_version(&bridge.daemon_url);
                                    log_stderr!(
                                        "[telepty-bridge] daemon restarted → v{}",
                                        daemon_ver.as_deref().unwrap_or("unknown")
                                    );
                                } else {
                                    log_stderr!(
                                        "[telepty-bridge] daemon restart failed, continuing with v{}",
                                        dmn
                                    );
                                }
                            }
                        }

                        let display_ver = daemon_ver.or(installed);
                        match display_ver {
                            Some(v) => log_stderr!(
                                "[telepty-bridge] connected to daemon v{} (attempt {})",
                                v, attempt
                            ),
                            None => log_stderr!(
                                "[telepty-bridge] connected to daemon (version unknown) (attempt {})",
                                attempt
                            ),
                        }
                        return Some(bridge);
                    } else {
                        log_stderr!(
                            "[telepty-bridge] daemon returned {}, running standalone",
                            code
                        );
                        return None;
                    }
                }
                _ => {
                    if attempt < 3 {
                        // One-time startup cost: 500ms retry delay × max 2 retries = 1.5s worst case.
                        // Acceptable because try_connect() runs once at app launch, not in a loop.
                        thread::sleep(Duration::from_millis(500));
                    } else {
                        log_stderr!("[telepty-bridge] daemon not available, running standalone");
                    }
                }
            }
        }

        None
    }

    /// Detect installed telepty CLI version.
    fn detect_version() -> Option<String> {
        let output = Command::new("telepty").arg("--version").output().ok()?;
        if !output.status.success() {
            return None;
        }
        let raw = String::from_utf8_lossy(&output.stdout);
        let version = raw.trim().to_string();
        if version.is_empty() {
            None
        } else {
            Some(version)
        }
    }

    /// Detect running daemon version from /api/health endpoint.
    fn detect_daemon_version(daemon_url: &str) -> Option<String> {
        let output = Command::new("curl")
            .args([
                "-s",
                "--max-time",
                "2",
                &format!("{}/api/health", daemon_url),
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let body = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&body).ok()?;
        json["version"].as_str().map(|s| s.to_string())
    }

    /// Restart telepty daemon. Returns true if the command succeeded.
    fn restart_daemon() -> bool {
        match Command::new("telepty").args(["daemon", "restart"]).output() {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    }

    /// Register an aterm workspace with telepty.
    /// Retries up to 3 times with backoff on failure.
    pub fn register(
        &self,
        session_id: &str,
        alias: &str,
        command: &str,
        cwd: &str,
        socket_path: &str,
    ) {
        let url = format!("{}/api/sessions/register", self.daemon_url);
        let body = Self::registration_payload(session_id, alias, command, cwd, socket_path);
        let sid = session_id.to_string();

        std::thread::spawn(move || {
            Self::register_with_retry(&url, &body, &sid);
        });
    }

    /// Build JSON payload for telepty registration.
    fn registration_payload(
        session_id: &str,
        alias: &str,
        command: &str,
        cwd: &str,
        socket_path: &str,
    ) -> String {
        serde_json::json!({
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
        })
        .to_string()
    }

    /// POST registration to telepty with retry. Returns true on success.
    fn register_with_retry(url: &str, body: &str, label: &str) -> bool {
        for attempt in 1..=3u64 {
            let output = Command::new("curl")
                .args([
                    "-s",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    "-X",
                    "POST",
                    url,
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    body,
                    "--max-time",
                    "3",
                ])
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let code = String::from_utf8_lossy(&out.stdout);
                    if code.starts_with('2') {
                        log_stderr!(
                            "[telepty-bridge] registered '{}' (attempt {})",
                            label,
                            attempt
                        );
                        return true;
                    }
                    log_stderr!(
                        "[telepty-bridge] register '{}' HTTP {} (attempt {})",
                        label,
                        code,
                        attempt
                    );
                }
                Ok(out) => {
                    log_stderr!(
                        "[telepty-bridge] register '{}' curl exit {} (attempt {})",
                        label,
                        out.status,
                        attempt
                    );
                }
                Err(e) => {
                    log_stderr!(
                        "[telepty-bridge] register '{}' curl error: {} (attempt {})",
                        label,
                        e,
                        attempt
                    );
                }
            }

            if attempt < 3 {
                // Exponential backoff in fire-and-forget background thread.
                // Does not block main thread or PTY I/O.
                thread::sleep(Duration::from_millis(300 * attempt));
            }
        }
        log_stderr!(
            "[telepty-bridge] register '{}' FAILED after 3 attempts",
            label
        );
        false
    }

    /// Re-register multiple workspaces with telepty after session restore.
    /// Runs in a background thread with staggered requests to avoid
    /// overwhelming the daemon.
    pub fn sync_all(&self, workspaces: Vec<(String, String, String, String)>) {
        let daemon_url = self.daemon_url.clone();

        std::thread::spawn(move || {
            // Let all sessions finish spawning first
            // Startup settle: wait for all sessions to finish spawning before
            // re-registering with telepty. Runs in background thread.
            thread::sleep(Duration::from_secs(2));

            let url = format!("{}/api/sessions/register", daemon_url);
            let total = workspaces.len();
            let mut ok = 0usize;

            for (i, (session_id, command, cwd, socket_path)) in workspaces.iter().enumerate() {
                if i > 0 {
                    // Stagger requests to avoid overwhelming telepty daemon.
                    // Background thread — does not block any critical path.
                    thread::sleep(Duration::from_millis(200));
                }
                let body =
                    Self::registration_payload(session_id, session_id, command, cwd, socket_path);
                if Self::register_with_retry(&url, &body, session_id) {
                    ok += 1;
                }
            }
            log_stderr!(
                "[telepty-bridge] sync_all complete: {}/{} registered",
                ok,
                total
            );
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
