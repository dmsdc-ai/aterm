use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

/// Header the telepty daemon reads the auth token from.
const AUTH_HEADER: &str = "x-telepty-token";

/// Port of the production telepty daemon — the developer's live daemon on a dev
/// box. The only place this number is written; `hermetic_guards_still_in_source`
/// fails if it reappears elsewhere in the production half of this file.
const PRODUCTION_PORT: u16 = 3848;

/// Optional bridge to telepty daemon.
/// All methods are fire-and-forget — failures are logged but never block aterm.
pub struct TeleptyBridge {
    daemon_url: String,
}

impl TeleptyBridge {
    /// curl invocation with the telepty credential attached.
    /// Every authenticated call site goes through here so the token is resolved
    /// in exactly one place. Never logged — see `auth_token`.
    fn curl(args: &[&str]) -> std::io::Result<Output> {
        let mut cmd = Command::new("curl");
        cmd.args(args);
        if let Some(token) = Self::auth_token() {
            cmd.args(["-H", &format!("{}: {}", AUTH_HEADER, token)]);
        }
        cmd.output()
    }

    /// Resolve the telepty auth token: `TELEPTY_AUTH_TOKEN` override, else
    /// `authToken` from `~/.telepty/config.json`.
    ///
    /// The file is the path that must work unaided — a GUI launched from Finder
    /// inherits no shell environment, so env alone would pass every terminal
    /// test and fail every real user.
    ///
    /// Read fresh on each call rather than cached: on a fresh install the daemon
    /// writes that file moments after aterm launches, so a startup read would
    /// cache a permanent empty. A file read is noise next to the curl process
    /// spawn it accompanies, and it picks up token rotation for free.
    ///
    /// `None` when absent or malformed — the caller then sends no header, gets a
    /// 401, and takes the "daemon unavailable, running standalone" path it
    /// already has. Never returned into a log line.
    fn auth_token() -> Option<String> {
        if let Ok(token) = std::env::var("TELEPTY_AUTH_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }
        // $HOME first, passwd entry as fallback — matches the Swift resolver and
        // telepty's own choice of where it writes this file.
        let home = std::env::home_dir()?;
        Self::token_from_config(&home.join(".telepty/config.json"))
    }

    fn token_from_config(path: &Path) -> Option<String> {
        let body = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&body).ok()?;
        json["authToken"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    }

    /// True when this process is a `cargo test` binary.
    ///
    /// Unit tests get it from `cfg!(test)`. Integration tests link the lib built
    /// *without* `cfg(test)`, so they get it from `ATERM_HERMETIC`, which
    /// `.cargo/config.toml` sets for every cargo-spawned process. The shipped
    /// app is launched by Finder or `bin/aterm` — never by cargo — so it sees
    /// neither and behaves exactly as before.
    fn hermetic() -> bool {
        cfg!(test) || std::env::var_os("ATERM_HERMETIC").is_some_and(|v| !v.is_empty())
    }

    /// Resolve the daemon port from the `ATERM_TELEPTY_PORT` override.
    ///
    /// `None` means "do not connect at all". Under test the production default
    /// is refused, and so is an explicit 3848: a connected bridge talks to a
    /// daemon the test suite does not own. Tests that want a
    /// bridge point `ATERM_TELEPTY_PORT` at a stand-in they spawned.
    ///
    /// Pure so the guard test can cover both worlds without racing on the
    /// process-global environment.
    fn resolve_port(override_var: Option<&str>, hermetic: bool) -> Option<u16> {
        match override_var.and_then(|p| p.parse::<u16>().ok()) {
            Some(PRODUCTION_PORT) if hermetic => None,
            Some(port) => Some(port),
            None if hermetic => None,
            None => Some(PRODUCTION_PORT),
        }
    }

    /// Try to connect to telepty daemon. Returns None if unavailable.
    /// Retries up to 3 times with 500ms between attempts.
    pub fn try_connect() -> Option<Self> {
        let Some(port) = Self::resolve_port(
            std::env::var("ATERM_TELEPTY_PORT").ok().as_deref(),
            Self::hermetic(),
        ) else {
            log_stderr!(
                "[telepty-bridge] hermetic run: not connecting (point ATERM_TELEPTY_PORT at a stand-in daemon to test the bridge)"
            );
            return None;
        };
        let bridge = Self {
            daemon_url: format!("http://127.0.0.1:{}", port),
        };

        for attempt in 1..=3 {
            // Quick health check
            let output = Self::curl(&[
                "-s",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "--max-time",
                "1",
                &format!("{}/api/sessions", bridge.daemon_url),
            ]);

            match output {
                Ok(out) if out.status.success() => {
                    let code = String::from_utf8_lossy(&out.stdout);
                    if code.starts_with('2') {
                        let installed = Self::detect_version();
                        let daemon_ver = Self::detect_daemon_version(&bridge.daemon_url);

                        // Version skew is logged, never acted on: aterm does not own the
                        // daemon, and restarting it stops every session on it (#895).
                        if let (Some(ref inst), Some(ref dmn)) = (&installed, &daemon_ver) {
                            if inst != dmn {
                                log_stderr!(
                                    "[telepty-bridge] daemon v{} differs from installed CLI v{}; not restarting — aterm does not own the daemon (#895)",
                                    dmn, inst
                                );
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
            let output = Self::curl(&[
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
            ]);

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

            // Cleanup stale aterm sessions not in current workspace set (#129)
            let known_names: HashSet<&str> =
                workspaces.iter().map(|(name, _, _, _)| name.as_str()).collect();
            let sessions_url = format!("{}/api/sessions", daemon_url);
            if let Ok(output) = Self::curl(&["-s", "--max-time", "3", &sessions_url]) {
                if output.status.success() {
                    let body = String::from_utf8_lossy(&output.stdout);
                    if let Ok(sessions) =
                        serde_json::from_str::<Vec<serde_json::Value>>(body.trim())
                    {
                        let mut cleaned = 0usize;
                        for session in &sessions {
                            let id = session
                                .get("session_id")
                                .or_else(|| session.get("id"))
                                .and_then(|v| v.as_str());
                            let is_aterm = session
                                .get("term_program")
                                .and_then(|v| v.as_str())
                                == Some("aterm")
                                || session
                                    .get("delivery_type")
                                    .and_then(|v| v.as_str())
                                    == Some("aterm");
                            if let Some(id) = id {
                                if is_aterm && !known_names.contains(id) {
                                    let del_url =
                                        format!("{}/api/sessions/{}", daemon_url, id);
                                    let _ = Self::curl(&[
                                        "-s", "-X", "DELETE", &del_url, "--max-time", "2",
                                    ]);
                                    cleaned += 1;
                                }
                            }
                        }
                        if cleaned > 0 {
                            log_stderr!(
                                "[telepty-bridge] cleaned {} stale session(s)",
                                cleaned
                            );
                        }
                    }
                }
            }
        });
    }

    /// Deregister a workspace from telepty. Fire-and-forget.
    pub fn deregister(&self, session_id: &str) {
        let url = format!("{}/api/sessions/{}", self.daemon_url, session_id);

        std::thread::spawn(move || {
            let _ = Self::curl(&["-s", "-X", "DELETE", &url, "--max-time", "2"]);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::mpsc;

    fn write_config(dir: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn token_read_from_config_file() {
        let path = write_config(
            "aterm-telepty-token-ok",
            r#"{"authToken":"tok-123","createdAt":"2026-03-11T01:17:37.274Z"}"#,
        );
        assert_eq!(
            TeleptyBridge::token_from_config(&path).as_deref(),
            Some("tok-123")
        );
    }

    /// Absent or malformed config must degrade to "no token", never panic —
    /// this runs inside a GUI process.
    #[test]
    fn missing_or_malformed_config_degrades_to_none() {
        let missing = std::env::temp_dir().join("aterm-telepty-token-absent.json");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(TeleptyBridge::token_from_config(&missing), None);

        for body in [r#"{ not json"#, "{}", r#"{"authToken":""}"#, r#"{"authToken":42}"#] {
            let path = write_config("aterm-telepty-token-bad", body);
            assert_eq!(
                TeleptyBridge::token_from_config(&path),
                None,
                "expected None for config body: {}",
                body
            );
        }
    }

    /// `cargo test` reached the production daemon on :3848 before this guard —
    /// four pty tests construct the app singleton, which connects. The port must
    /// stay unresolvable from a test process, including when a test asks for it
    /// by name.
    #[test]
    fn production_port_never_resolves_under_test() {
        assert_eq!(TeleptyBridge::resolve_port(None, true), None);
        assert_eq!(TeleptyBridge::resolve_port(Some("3848"), true), None);
        assert_eq!(TeleptyBridge::resolve_port(Some("junk"), true), None);
        // A stand-in the test spawned itself is fine.
        assert_eq!(TeleptyBridge::resolve_port(Some("49152"), true), Some(49152));
        // Production is untouched: same default, same override.
        assert_eq!(TeleptyBridge::resolve_port(None, false), Some(3848));
        assert_eq!(TeleptyBridge::resolve_port(Some("49152"), false), Some(49152));
    }

    #[test]
    fn hermetic_is_armed_in_this_binary() {
        assert!(TeleptyBridge::hermetic());
    }

    /// The port guard is one deleted line away from gone, and no behavioural test
    /// can see a guard that is no longer there. Source assertion, same shape as
    /// `ffi_tests::all_ffi_entry_points_wrapped_in_catch_unwind`.
    #[test]
    fn hermetic_guards_still_in_source() {
        let src = include_str!("telepty_bridge.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();

        let in_code = production
            .lines()
            .filter(|l| !l.trim_start().starts_with("//") && l.contains("3848"))
            .count();
        assert_eq!(
            in_code, 1,
            "3848 must appear in code exactly once, as PRODUCTION_PORT (comments are free to mention it)"
        );

        let (_, connect) = production
            .split_once("pub fn try_connect()")
            .expect("try_connect must exist");
        assert!(
            connect[..connect.len().min(400)].contains("Self::resolve_port("),
            "try_connect no longer resolves its port through the hermetic guard"
        );
    }

    /// The check that matters: the header is on the wire, not merely compiled in.
    #[test]
    fn auth_header_reaches_the_daemon() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = sock.read(&mut buf).unwrap();
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]");
            tx.send(String::from_utf8_lossy(&buf[..n]).to_string()).unwrap();
        });

        std::env::set_var("TELEPTY_AUTH_TOKEN", "tok-on-the-wire");
        let out = TeleptyBridge::curl(&[
            "-s",
            "--max-time",
            "3",
            &format!("http://127.0.0.1:{}/api/sessions", port),
        ]);
        std::env::remove_var("TELEPTY_AUTH_TOKEN");
        assert!(out.unwrap().status.success());

        let request = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            request.contains("x-telepty-token: tok-on-the-wire"),
            "auth header missing from request:\n{}",
            request
        );
    }
}
