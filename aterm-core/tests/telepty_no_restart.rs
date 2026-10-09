//! #895: aterm must never restart a telepty daemon, whatever versions the CLI
//! and the daemon report. Own binary because it mutates the process
//! environment; keep it to a single test.
//!
//! Hermetic by construction: the daemon is a stand-in on 127.0.0.1:0 and
//! `telepty` resolves to a script in CARGO_TARGET_TMPDIR, checked before
//! `try_connect` runs. The real daemon and the real CLI are never reached.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use aterm_core::telepty_bridge::TeleptyBridge;

const CLI_VERSION: &str = "9.9.9-cli";
const DAEMON_VERSION: &str = "0.0.1-daemon";

fn serve(mut sock: TcpStream, requests: &Mutex<Vec<String>>) {
    let _ = sock.set_nonblocking(false);
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let line = head.lines().next().unwrap_or("").to_string();
    let body = if line.starts_with("GET /api/sessions ") {
        Some("[]".to_string())
    } else if line.starts_with("GET /api/health ") {
        Some(format!(r#"{{"version":"{}"}}"#, DAEMON_VERSION))
    } else {
        None
    };
    requests.lock().unwrap().push(line);
    let response = match body {
        Some(b) => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            b.len(),
            b
        ),
        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
    };
    let _ = sock.write_all(response.as_bytes());
}

#[test]
fn version_mismatch_never_restarts_the_daemon() {
    // 1. Fake daemon.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 3848, "stand-in must not sit on the production port");
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let requests = Arc::clone(&requests);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((sock, _)) => serve(sock, &requests),
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
        })
    };

    // 2. Fake `telepty` CLI: `--version` answers, anything else is recorded.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fake-telepty");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("marker");
    let script = dir.join("telepty");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo {}\n  exit 0\nfi\necho \"$@\" >> '{}'\nexit 0\n",
            CLI_VERSION,
            marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    // 3. Environment. PATH keeps the original entries so curl still resolves.
    let original = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(dir.clone()).chain(std::env::split_paths(&original)),
    )
    .unwrap();
    std::env::set_var("PATH", &path);
    std::env::set_var("ATERM_TELEPTY_PORT", port.to_string());
    std::env::set_var("TELEPTY_AUTH_TOKEN", "test-token");
    // Exercise the shipped app's path, not the #886 test-only guard: without
    // this, cargo's ATERM_HERMETIC would mask a restart on the old code.
    std::env::remove_var("ATERM_HERMETIC");

    // Live-daemon guard: `telepty` must resolve to the fake before anything
    // can call it with real arguments.
    let probe = Command::new("telepty").arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&probe.stdout).trim(),
        CLI_VERSION,
        "`telepty` on PATH is not the fake script"
    );

    // 4. Connect.
    let bridge = TeleptyBridge::try_connect();

    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let seen = requests.lock().unwrap().clone();

    // 5. Assertions.
    assert!(bridge.is_some(), "bridge did not connect; requests: {:?}", seen);
    assert!(
        seen.iter().any(|l| l.starts_with("GET /api/health ")),
        "daemon version never read, so the mismatch branch was not evaluated; requests: {:?}",
        seen
    );
    let restarted = std::fs::read_to_string(&marker).ok();
    assert!(
        restarted.is_none(),
        "aterm invoked `telepty {}` on a daemon it does not own (#895)",
        restarted.unwrap_or_default().trim()
    );
}
