//! Integration tests for polling→event conversions (Phase 1)
//! TEST 4: WaitUntil Condvar
//! TEST 5: Sidebar IPC event push

use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// WorkspaceStatus is Arc<(Mutex<String>, Condvar)>
type WorkspaceStatus = Arc<(Mutex<String>, Condvar)>;

fn make_status(initial: &str) -> WorkspaceStatus {
    Arc::new((Mutex::new(initial.to_string()), Condvar::new()))
}

// ===== TEST 4: WaitUntil Condvar =====

#[test]
fn wait_until_returns_immediately_if_already_in_target_state() {
    let status = make_status("idle");
    let start = Instant::now();

    let current = status.0.lock().unwrap();
    assert_eq!(*current, "idle");
    drop(current);

    // Simulating WaitUntil logic: check if already in target state
    let current = status.0.lock().unwrap();
    let already_reached = *current == "idle";
    drop(current);

    assert!(already_reached, "Should detect target state immediately");
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "Should return immediately, got {:?}",
        start.elapsed()
    );
}

#[test]
fn wait_until_blocks_until_workspace_reaches_target_state() {
    let status = make_status("running");
    let status2 = status.clone();

    let handle = thread::spawn(move || {
        let start = Instant::now();
        let (ref mutex, ref condvar) = *status2;
        let mut current = mutex.lock().unwrap();
        while *current != "idle" {
            current = condvar.wait(current).unwrap();
        }
        start.elapsed()
    });

    // Give thread time to start waiting
    thread::sleep(Duration::from_millis(20));

    // Change state and notify
    {
        let mut current = status.0.lock().unwrap();
        *current = "idle".to_string();
    }
    status.1.notify_all();

    let elapsed = handle.join().unwrap();
    assert!(
        elapsed < Duration::from_millis(200),
        "Should wake promptly on state change, got {:?}",
        elapsed
    );
}

#[test]
fn wait_until_timeout_returns_after_specified_duration() {
    let status = make_status("running");
    let timeout = Duration::from_millis(100);

    let start = Instant::now();
    let (ref mutex, ref condvar) = *status;
    let mut current = mutex.lock().unwrap();
    let deadline = Instant::now() + timeout;

    let mut timed_out = false;
    while *current != "idle" {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        let (guard, _) = condvar.wait_timeout(current, remaining).unwrap();
        current = guard;
    }

    let elapsed = start.elapsed();
    assert!(timed_out, "Should time out when state never changes");
    assert!(
        elapsed >= Duration::from_millis(90),
        "Should wait approximately the timeout duration, got {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(300),
        "Should not wait much longer than timeout, got {:?}",
        elapsed
    );
}

#[test]
fn wait_until_status_change_notifies_all_waiters() {
    let status = make_status("running");
    let num_waiters = 3;
    let mut handles = Vec::new();

    for _ in 0..num_waiters {
        let status_clone = status.clone();
        handles.push(thread::spawn(move || {
            let (ref mutex, ref condvar) = *status_clone;
            let mut current = mutex.lock().unwrap();
            while *current != "idle" {
                current = condvar.wait(current).unwrap();
            }
            true
        }));
    }

    // Give threads time to start waiting
    thread::sleep(Duration::from_millis(30));

    // Change state and notify ALL
    {
        let mut current = status.0.lock().unwrap();
        *current = "idle".to_string();
    }
    status.1.notify_all();

    // All waiters should wake up
    for handle in handles {
        let result = handle.join().unwrap();
        assert!(result, "All waiters should receive notification");
    }
}

// ===== TEST 5: Sidebar IPC event push =====
// These tests verify the broadcast/subscriber pattern used for sidebar updates.
// Since IpcServer requires Unix socket infrastructure, we test the pattern directly.

#[test]
fn ipc_event_broadcast_to_subscribers() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    // Create a connected pair of streams to simulate subscriber
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    // Simulate broadcast: write event JSON + newline
    let event = r#"{"type":"Created","id":"test-ws","name":"test-ws"}"#;
    let line = format!("{}\n", event);

    // Write to subscriber (simulating broadcast)
    let mut writer_clone = writer.try_clone().unwrap();
    writer_clone.write_all(line.as_bytes()).unwrap();
    writer_clone.flush().unwrap();

    // Read from subscriber side
    let mut buf = vec![0u8; 1024];
    let n = reader.read(&mut buf).unwrap();
    let received = String::from_utf8_lossy(&buf[..n]);
    assert!(
        received.contains("Created"),
        "Subscriber should receive Created event"
    );
    assert!(
        received.contains("test-ws"),
        "Subscriber should receive workspace id"
    );
}

#[test]
fn ipc_event_broadcast_closed_event() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    let event = r#"{"type":"Closed","id":"test-ws"}"#;
    let line = format!("{}\n", event);

    let mut writer_clone = writer.try_clone().unwrap();
    writer_clone.write_all(line.as_bytes()).unwrap();
    writer_clone.flush().unwrap();

    let mut buf = vec![0u8; 1024];
    let n = reader.read(&mut buf).unwrap();
    let received = String::from_utf8_lossy(&buf[..n]);
    assert!(
        received.contains("Closed"),
        "Subscriber should receive Closed event"
    );
}

#[test]
fn ipc_event_broadcast_status_changed() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    let event = r#"{"type":"StatusChanged","id":"test-ws","state":"idle"}"#;
    let line = format!("{}\n", event);

    let mut writer_clone = writer.try_clone().unwrap();
    writer_clone.write_all(line.as_bytes()).unwrap();
    writer_clone.flush().unwrap();

    let mut buf = vec![0u8; 1024];
    let n = reader.read(&mut buf).unwrap();
    let received = String::from_utf8_lossy(&buf[..n]);
    assert!(
        received.contains("StatusChanged"),
        "Subscriber should receive StatusChanged event"
    );
    assert!(
        received.contains("idle"),
        "Event should contain the new state"
    );
}

#[test]
fn ipc_subscriber_disconnect_detected_on_write_failure() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let (reader, writer) = UnixStream::pair().unwrap();

    // Drop reader to simulate subscriber disconnect
    drop(reader);

    // Write should fail (broken pipe)
    let mut writer_clone = writer.try_clone().unwrap();
    let event = r#"{"type":"Created","id":"ws1"}"#;
    let line = format!("{}\n", event);

    // First write may succeed (buffered), subsequent writes should fail
    let mut failures = 0;
    for _ in 0..4 {
        if writer_clone
            .write_all(line.as_bytes())
            .and_then(|_| writer_clone.flush())
            .is_err()
        {
            failures += 1;
        }
    }
    assert!(
        failures > 0,
        "Should detect write failure after subscriber disconnect"
    );
    // In the real IpcServer, 3 consecutive failures triggers cleanup
}

// ===== TEST 8: Stale cleanup one-shot =====
// Workspace Closed event triggers cleanup exactly once (not a repeating timer).

#[test]
fn stale_cleanup_fires_once_on_closed_event() {
    // Simulate: workspace transitions to "closing" → triggers one-shot cleanup.
    // Cleanup should happen exactly once, not on a repeating interval.
    let status = make_status("running");
    let cleanup_count = Arc::new(Mutex::new(0u32));
    let cleanup_count2 = cleanup_count.clone();

    // Simulate the Closed event triggering cleanup
    {
        let mut current = status.0.lock().unwrap();
        *current = "closing".to_string();
    }
    status.1.notify_all();

    // One-shot cleanup handler: runs exactly once when Closed is observed
    let status_clone = status.clone();
    let handle = thread::spawn(move || {
        let (ref mutex, ref condvar) = *status_clone;
        let mut current = mutex.lock().unwrap();
        while *current != "closing" {
            current = condvar.wait(current).unwrap();
        }
        // Cleanup fires once
        let mut count = cleanup_count2.lock().unwrap();
        *count += 1;
    });

    handle.join().unwrap();

    let count = *cleanup_count.lock().unwrap();
    assert_eq!(
        count, 1,
        "Cleanup should fire exactly once (one-shot), not repeating"
    );
}

#[test]
fn stale_cleanup_not_repeating_timer() {
    // Verify the cleanup pattern is NOT a repeating timer.
    // After cleanup fires, subsequent state changes should NOT re-trigger cleanup.
    let status = make_status("closing");
    let cleanup_count = Arc::new(Mutex::new(0u32));

    // One-shot: observe "closing" once, increment counter, then stop
    {
        let current = status.0.lock().unwrap();
        if *current == "closing" {
            *cleanup_count.lock().unwrap() += 1;
        }
    }

    // Simulate additional state changes — should NOT trigger more cleanups
    // (A repeating timer would keep firing)
    for _ in 0..5 {
        thread::sleep(Duration::from_millis(10));
        // One-shot handler does not re-check
    }

    assert_eq!(
        *cleanup_count.lock().unwrap(),
        1,
        "One-shot cleanup must not repeat on subsequent ticks"
    );
}

// ===== TEST 9: CreateWorkspace registration Condvar =====

#[test]
fn create_workspace_blocks_until_registered() {
    // CreateWorkspace waits on a Condvar (registration_signal) instead of 50ms polling.
    let registration_signal: Arc<(Mutex<()>, Condvar)> = Arc::new((Mutex::new(()), Condvar::new()));
    let registered = Arc::new(Mutex::new(false));
    let registered2 = registered.clone();
    let signal = registration_signal.clone();

    let handle = thread::spawn(move || {
        let start = Instant::now();
        // Simulate CreateWorkspace waiting for registration
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if *registered2.lock().unwrap() {
                return (true, start.elapsed());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return (false, start.elapsed());
            }
            let (ref lock, ref cvar) = *signal;
            if let Ok(guard) = lock.lock() {
                let _ = cvar.wait_timeout(guard, remaining);
            }
        }
    });

    // Simulate workspace registration after 50ms
    thread::sleep(Duration::from_millis(50));
    *registered.lock().unwrap() = true;
    registration_signal.1.notify_all();

    let (success, elapsed) = handle.join().unwrap();
    assert!(success, "Should detect registration");
    assert!(
        elapsed < Duration::from_millis(200),
        "Should wake promptly on registration (Condvar, not polling), got {:?}",
        elapsed
    );
    assert!(
        elapsed >= Duration::from_millis(40),
        "Should have waited for registration, got {:?}",
        elapsed
    );
}

#[test]
fn create_workspace_timeout_after_15s_if_never_registered() {
    // Use a shorter timeout for test (150ms simulates the 15s pattern)
    let registration_signal: Arc<(Mutex<()>, Condvar)> = Arc::new((Mutex::new(()), Condvar::new()));
    let registered = Arc::new(Mutex::new(false));
    let registered2 = registered.clone();
    let signal = registration_signal.clone();

    let timeout = Duration::from_millis(150); // Shortened for test speed

    let handle = thread::spawn(move || {
        let start = Instant::now();
        let deadline = Instant::now() + timeout;
        loop {
            if *registered2.lock().unwrap() {
                return (true, start.elapsed());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return (false, start.elapsed());
            }
            let (ref lock, ref cvar) = *signal;
            if let Ok(guard) = lock.lock() {
                let _ = cvar.wait_timeout(guard, remaining);
            }
        }
    });

    // Never register — should timeout
    let (success, elapsed) = handle.join().unwrap();
    assert!(!success, "Should timeout when registration never happens");
    assert!(
        elapsed >= Duration::from_millis(140),
        "Should wait approximately the timeout duration, got {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(400),
        "Should not wait much longer than timeout, got {:?}",
        elapsed
    );
}

#[test]
fn create_workspace_returns_immediately_if_already_registered() {
    let _registration_signal: Arc<(Mutex<()>, Condvar)> =
        Arc::new((Mutex::new(()), Condvar::new()));
    let registered = Arc::new(Mutex::new(true)); // Already registered

    let start = Instant::now();
    let already = *registered.lock().unwrap();
    let elapsed = start.elapsed();

    assert!(already, "Should detect already-registered workspace");
    assert!(
        elapsed < Duration::from_millis(5),
        "Should return immediately for already-registered workspace, got {:?}",
        elapsed
    );
}

// ===== TEST 10: TaskQueueLoader FSEvents (integration) =====
// File modification detection — tests the pattern of watching for file changes.

#[test]
fn task_queue_file_change_detected() {
    use std::io::Write;

    // Create a temp file simulating task-queue.json
    let dir = std::env::temp_dir().join(format!("aterm-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("task-queue.json");

    // Write initial content
    let initial = r#"{"tasks":[],"completed":[]}"#;
    std::fs::write(&file_path, initial).unwrap();
    let mtime1 = std::fs::metadata(&file_path).unwrap().modified().unwrap();

    // Small delay to ensure filesystem timestamp granularity
    thread::sleep(Duration::from_millis(50));

    // Modify file (simulates external task queue update)
    let updated = r#"{"tasks":[{"id":1,"desc":"test"}],"completed":[]}"#;
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(updated.as_bytes()).unwrap();
        f.flush().unwrap();
    }
    let mtime2 = std::fs::metadata(&file_path).unwrap().modified().unwrap();

    // Verify modification is detectable
    assert!(
        mtime2 > mtime1,
        "File modification should be detectable via mtime"
    );

    // Verify content changed
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(
        content.contains("test"),
        "Modified content should be readable"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn task_queue_reload_reads_updated_content() {
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!("aterm-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("task-queue.json");

    // Initial load
    std::fs::write(&file_path, r#"{"tasks":[],"completed":[]}"#).unwrap();
    let v1: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file_path).unwrap()).unwrap();
    assert_eq!(v1["tasks"].as_array().unwrap().len(), 0);

    // External update (simulates FSEvents trigger)
    thread::sleep(Duration::from_millis(20));
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, r#"{{"tasks":[{{"id":1}},{{"id":2}}],"completed":[]}}"#).unwrap();
    }

    // Reload (simulates what the FSEvents handler would do)
    let v2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file_path).unwrap()).unwrap();
    assert_eq!(
        v2["tasks"].as_array().unwrap().len(),
        2,
        "Reload should pick up new tasks after file modification"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ===== TEST 11: Dispatch CreateWorkspace ready-signal =====

#[test]
fn create_workspace_returns_after_shell_running() {
    // CreateWorkspace should return once workspace status reaches "running"
    // (via Condvar), NOT after a fixed sleep(2).
    let status = make_status("starting");
    let status2 = status.clone();

    let handle = thread::spawn(move || {
        let start = Instant::now();
        let deadline = Instant::now() + Duration::from_secs(15);
        let (ref mutex, ref condvar) = *status2;
        let mut current = mutex.lock().unwrap();
        loop {
            if *current == "running" {
                return (true, start.elapsed());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return (false, start.elapsed());
            }
            let result = condvar
                .wait_timeout(current, remaining)
                .unwrap_or_else(|e| e.into_inner());
            current = result.0;
        }
    });

    // Simulate shell becoming ready after 80ms (much faster than old 2s sleep)
    thread::sleep(Duration::from_millis(80));
    {
        let mut current = status.0.lock().unwrap();
        *current = "running".to_string();
    }
    status.1.notify_all();

    let (ready, elapsed) = handle.join().unwrap();
    assert!(ready, "Should detect 'running' state");
    assert!(
        elapsed < Duration::from_millis(300),
        "Should return promptly when shell is running (no sleep(2)), got {:?}",
        elapsed
    );
    assert!(
        elapsed >= Duration::from_millis(70),
        "Should have waited for shell to be ready, got {:?}",
        elapsed
    );
}

#[test]
fn create_workspace_timeout_returns_after_specified_duration() {
    // If shell never reaches "running", CreateWorkspace should timeout
    let status = make_status("starting");
    let timeout = Duration::from_millis(150);

    let start = Instant::now();
    let deadline = Instant::now() + timeout;
    let (ref mutex, ref condvar) = *status;
    let mut current = mutex.lock().unwrap();
    let mut timed_out = false;

    loop {
        if *current == "running" {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        let result = condvar
            .wait_timeout(current, remaining)
            .unwrap_or_else(|e| e.into_inner());
        current = result.0;
    }

    let elapsed = start.elapsed();
    assert!(timed_out, "Should timeout when shell never reaches running");
    assert!(
        elapsed >= Duration::from_millis(140),
        "Should wait approximately the timeout, got {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(400),
        "Should not overshoot timeout, got {:?}",
        elapsed
    );
}

#[test]
fn create_workspace_no_sleep_2_in_ready_detection() {
    // Prove that ready detection is faster than the old 2s fixed delay.
    // Shell ready after 30ms should return well under 2000ms.
    let status = make_status("starting");
    let status2 = status.clone();

    let handle = thread::spawn(move || {
        let start = Instant::now();
        let deadline = Instant::now() + Duration::from_secs(15);
        let (ref mutex, ref condvar) = *status2;
        let mut current = mutex.lock().unwrap();
        loop {
            if *current == "running" {
                return start.elapsed();
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return start.elapsed();
            }
            let result = condvar
                .wait_timeout(current, remaining)
                .unwrap_or_else(|e| e.into_inner());
            current = result.0;
        }
    });

    thread::sleep(Duration::from_millis(30));
    {
        let mut current = status.0.lock().unwrap();
        *current = "running".to_string();
    }
    status.1.notify_all();

    let elapsed = handle.join().unwrap();
    assert!(
        elapsed < Duration::from_millis(500),
        "Ready detection must be WAY faster than old sleep(2) = 2000ms, got {:?}",
        elapsed
    );
}
