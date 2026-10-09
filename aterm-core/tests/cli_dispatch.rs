//! Hermetic integration tests for `bin/aterm` (`done`, `dispatch`, `tasks`).
//! Each test drives the bash CLI against a fake NDJSON Unix-socket server that
//! implements the PLAN §2.0 wire contract. No real aterm app, IPC socket or
//! telepty daemon is ever contacted: children run with `env_clear()` and an
//! explicit env (PATH = <tmp>/bin:/usr/bin:/bin, no aigentry-devkit).
#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const CHILD_TIMEOUT: Duration = Duration::from_secs(60);

// ── Fake IPC server ──────────────────────────────────────────

struct FakeServer {
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FakeServer {
    /// One request line per connection (as `bin/aterm` sends): record it,
    /// answer with `respond(request)` as one NDJSON line, close.
    fn start(sock: &Path, respond: impl Fn(&Value) -> Value + Send + 'static) -> Self {
        let listener = UnixListener::bind(sock).expect("bind fake socket");
        listener.set_nonblocking(true).expect("nonblocking listener");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (reqs, stop_flag) = (requests.clone(), stop.clone());
        let handle = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                let request: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
                let response = respond(&request);
                reqs.lock().unwrap().push(request);
                let _ = (&stream).write_all(format!("{}\n", response).as_bytes());
            }
        });
        Self { requests, stop, handle: Some(handle) }
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// ── Sandbox + child runner ───────────────────────────────────

struct Sandbox {
    dir: PathBuf,
}

struct Output {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Sandbox {
    /// `<tmp>` with `<tmp>/bin/python3` → the resolved python3. The short
    /// `tag` keeps `<tmp>/s.sock` under the Unix socket path limit (104 B on macOS).
    fn new(tag: &str) -> Self {
        let mut base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        if base.as_os_str().len() > 80 {
            base = std::env::temp_dir();
        }
        let dir = base.join(format!("cd-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).expect("create sandbox bin");
        std::os::unix::fs::symlink(resolve_python3(), dir.join("bin/python3"))
            .expect("symlink python3");
        Self { dir }
    }

    fn sock(&self) -> PathBuf {
        self.dir.join("s.sock")
    }

    /// Run `bin/aterm <args>`. `with_socket` controls `ATERM_IPC_SOCKET`.
    /// stdout/stderr go to files (a child can never hang on a pipe).
    fn run(&self, args: &[&str], with_socket: bool) -> Output {
        let aterm = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/aterm");
        let (out_path, err_path) = (self.dir.join("stdout.log"), self.dir.join("stderr.log"));
        let mut cmd = Command::new("/bin/bash");
        cmd.arg(&aterm)
            .args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.dir.join("bin").display()))
            .env("HOME", &self.dir)
            .env("ATERM_WORKSPACE_NAME", "parent")
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(fs::File::create(&out_path).unwrap())
            .stderr(fs::File::create(&err_path).unwrap());
        if with_socket {
            cmd.env("ATERM_IPC_SOCKET", self.sock());
        }
        let mut child = cmd.spawn().expect("spawn bin/aterm");
        let deadline = Instant::now() + CHILD_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("bin/aterm {:?} did not exit within {:?}", args, CHILD_TIMEOUT);
            }
            thread::sleep(Duration::from_millis(10));
        };
        Output {
            code: status.code(),
            stdout: fs::read_to_string(&out_path).unwrap_or_default(),
            stderr: fs::read_to_string(&err_path).unwrap_or_default(),
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn resolve_python3() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("python3"))
        .find(|p| p.is_file())
        .map(|p| fs::canonicalize(&p).unwrap_or(p))
        .expect("python3 not found on PATH")
}

fn actions(requests: &[Value]) -> Vec<&str> {
    requests.iter().map(|r| r["action"].as_str().unwrap_or("")).collect()
}

/// Fake aterm for `dispatch`; `wait_until` is the WaitUntil reply.
fn dispatch_responder(create: Value, wait_until: Value) -> impl Fn(&Value) -> Value + Send {
    move |req: &Value| match req["action"].as_str().unwrap_or("") {
        "CreateWorkspace" => create.clone(),
        "WaitUntil" => wait_until.clone(),
        "WorkspaceStatus" => json!({"status":"Data","data":{
            "alive": true, "state": "complete", "report": "done: widget"}}),
        _ => json!({"status":"Ok"}),
    }
}

// No file-extension hint in the description, so dispatch takes the sub-session path.
const PLAN_DESC: &str = "implement the widget feature";
const SUB: &str = "dispatch-plan-sub0";

// ── Tests ────────────────────────────────────────────────────

#[test]
fn done_sends_mark_complete_with_workspace_and_report() {
    let sb = Sandbox::new("t1");
    let server = FakeServer::start(&sb.sock(), |_| json!({"status":"Ok"}));
    let out = sb.run(&["done", "ok 1"], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(
        server.requests(),
        vec![json!({"action":"MarkComplete","workspace":"parent","report":"ok 1"})]
    );
}

#[test]
fn dispatch_waits_for_complete_and_closes_completed() {
    let sb = Sandbox::new("t2");
    let server = FakeServer::start(
        &sb.sock(),
        dispatch_responder(
            json!({"status":"Data","data":{"ready":true}}),
            json!({"status":"Data","data":{"reached":true,"state":"complete"}}),
        ),
    );
    let out = sb.run(&["dispatch", "--plan", PLAN_DESC], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let reqs = server.requests();

    let inject = reqs.iter().find(|r| r["action"] == "Inject").expect("Inject sent");
    assert!(inject["text"].as_str().unwrap().contains("aterm done"), "inject: {}", inject);

    let wait_idx = reqs
        .iter()
        .position(|r| r["action"] == "WaitUntil" && r["workspace"] == SUB && r["state"] == "complete")
        .unwrap_or_else(|| panic!("WaitUntil state=complete missing: {:?}", actions(&reqs)));
    assert!(reqs[wait_idx]["timeout_ms"].as_u64().unwrap() <= 300_000);
    let close_idx = reqs
        .iter()
        .position(|r| r["action"] == "CloseWorkspace" && r["workspace"] == SUB)
        .unwrap_or_else(|| panic!("CloseWorkspace missing: {:?}", actions(&reqs)));
    assert!(wait_idx < close_idx, "order: {:?}", actions(&reqs));

    let result: Value = serde_json::from_str(&out.stdout).expect("dispatch stdout is JSON");
    assert_eq!(result["status"], "all_complete", "{}", result);
    assert_eq!(result["reports"][0]["status"], "complete");
    assert_eq!(result["reports"][0]["report"], "done: widget");
}

#[test]
fn dispatch_timeout_leaves_session_running() {
    let sb = Sandbox::new("t3");
    let server = FakeServer::start(
        &sb.sock(),
        dispatch_responder(
            json!({"status":"Data","data":{"ready":true}}),
            json!({"status":"Data","data":{"reached":false,"state":"running","timeout":true}}),
        ),
    );
    let out = sb.run(&["dispatch", "--plan", PLAN_DESC], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let reqs = server.requests();
    assert!(actions(&reqs).contains(&"WaitUntil"), "{:?}", actions(&reqs));
    assert!(!actions(&reqs).contains(&"CloseWorkspace"), "{:?}", actions(&reqs));

    let result: Value = serde_json::from_str(&out.stdout).expect("dispatch stdout is JSON");
    assert_eq!(result["reports"][0]["status"], "left_running", "{}", result);
    assert_eq!(result["status"], "all_timeout", "{}", result);
}

#[test]
fn dispatch_create_unsupported_skips_inject() {
    let sb = Sandbox::new("t4");
    let server = FakeServer::start(
        &sb.sock(),
        dispatch_responder(
            json!({"status":"Unsupported"}),
            json!({"status":"Data","data":{"reached":true,"state":"complete"}}),
        ),
    );
    let out = sb.run(&["dispatch", "--plan", PLAN_DESC], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let reqs = server.requests();
    assert_eq!(actions(&reqs), vec!["CreateWorkspace"], "no Inject/WaitUntil/Close after Unsupported");

    let result: Value = serde_json::from_str(&out.stdout).expect("dispatch stdout is JSON");
    assert_eq!(result["reports"][0]["status"], "create_failed", "{}", result);
    assert_eq!(result["sessions_created"], json!([]));
}

#[test]
fn tasks_add_ids_unique_and_atomic() {
    let sb = Sandbox::new("t5");
    let first = sb.run(&["tasks", "add", "first task"], false);
    assert_eq!(first.code, Some(0), "stderr: {}", first.stderr);
    let second = sb.run(&["tasks", "add", "second task"], false);
    assert_eq!(second.code, Some(0), "stderr: {}", second.stderr);

    let id = |o: &Output| serde_json::from_str::<Value>(&o.stdout).expect("JSON")["id"].clone();
    assert_eq!(id(&first), json!(1));
    assert_eq!(id(&second), json!(2));

    let board_path = sb.dir.join("state/task-queue.json");
    let board: Value = serde_json::from_str(&fs::read_to_string(&board_path).unwrap()).unwrap();
    assert_eq!(board["tasks"].as_array().unwrap().len(), 2);
    assert!(!sb.dir.join("state/task-queue.json.tmp").exists(), ".tmp left behind");
}

#[test]
fn tasks_workspace_flag_uses_list_tasks() {
    let sb = Sandbox::new("t6");
    let server = FakeServer::start(&sb.sock(), |_| {
        json!({"status":"Data","data":{
            "tasks":[{"id":7,"description":"remote task"}],"completed":[]}})
    });
    let out = sb.run(&["tasks", "--workspace", "w1"], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(server.requests(), vec![json!({"action":"ListTasks","workspace":"w1"})]);
    assert!(out.stdout.contains("[7] remote task"), "stdout: {}", out.stdout);
    assert!(!sb.dir.join("state").exists(), "--workspace must not touch the local board");
}
