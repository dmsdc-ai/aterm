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
    /// answer with `respond(request)` as one NDJSON line, close. A `Null`
    /// response closes the connection without replying.
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
                if !response.is_null() {
                    let _ = (&stream).write_all(format!("{}\n", response).as_bytes());
                }
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
        self.run_env(args, with_socket, &[])
    }

    /// `run` with extra env vars on top of the explicit child env.
    fn run_env(&self, args: &[&str], with_socket: bool, envs: &[(&str, &str)]) -> Output {
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
        cmd.envs(envs.iter().copied());
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

/// The sub-session name dispatch sent in its first `CreateWorkspace`.
fn created_name(requests: &[Value]) -> String {
    let create = requests.iter().find(|r| r["action"] == "CreateWorkspace").expect("CreateWorkspace sent");
    create["name"].as_str().expect("CreateWorkspace name").to_string()
}

/// Files in `dir` whose name starts with `prefix`.
fn files_with_prefix(dir: &Path, prefix: &str) -> Vec<String> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(prefix))
                .collect()
        })
        .unwrap_or_default()
}

// No file-extension hint in the description, so dispatch takes the sub-session path.
const PLAN_DESC: &str = "implement the widget feature";

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
    let sub = created_name(&reqs);

    let inject = reqs.iter().find(|r| r["action"] == "Inject").expect("Inject sent");
    assert!(inject["text"].as_str().unwrap().contains("aterm done"), "inject: {}", inject);

    let wait_idx = reqs
        .iter()
        .position(|r| r["action"] == "WaitUntil" && r["workspace"] == sub.as_str() && r["state"] == "complete")
        .unwrap_or_else(|| panic!("WaitUntil state=complete missing: {:?}", actions(&reqs)));
    assert!(reqs[wait_idx]["timeout_ms"].as_u64().unwrap() <= 300_000);
    let close_idx = reqs
        .iter()
        .position(|r| r["action"] == "CloseWorkspace" && r["workspace"] == sub.as_str())
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

#[test]
fn dispatch_wait_error_reports_dead_not_left_running() {
    let sb = Sandbox::new("t7");
    let server = FakeServer::start(
        &sb.sock(),
        dispatch_responder(
            json!({"status":"Data","data":{"ready":true}}),
            json!({"status":"Error","message":"workspace 'dispatch-plan-sub0' not found"}),
        ),
    );
    let out = sb.run(&["dispatch", "--plan", PLAN_DESC], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(!actions(&server.requests()).contains(&"CloseWorkspace"));
    let result: Value = serde_json::from_str(&out.stdout).expect("dispatch stdout is JSON");
    assert_eq!(result["reports"][0]["status"], "dead", "{}", result);
}

#[test]
fn unsupported_reply_is_an_error() {
    let sb = Sandbox::new("t8");
    let server = FakeServer::start(&sb.sock(), |_| json!({"status":"Unsupported"}));
    let out = sb.run(&["focus", "w1"], true);
    assert_eq!(out.code, Some(2), "stdout: {} stderr: {}", out.stdout, out.stderr);
    assert!(out.stderr.contains("not supported"), "stderr: {}", out.stderr);
    assert!(!out.stdout.contains("\"ok\""), "stdout: {}", out.stdout);
    assert_eq!(server.requests(), vec![json!({"action":"FocusWorkspace","workspace":"w1"})]);
}

#[test]
fn export_unsupported_writes_no_file() {
    let sb = Sandbox::new("t9");
    let server = FakeServer::start(&sb.sock(), |_| json!({"status":"Unsupported"}));
    let out = sb.run(&["export", "w1"], true);
    assert_eq!(out.code, Some(2), "stdout: {} stderr: {}", out.stdout, out.stderr);
    assert!(out.stderr.contains("not supported"), "stderr: {}", out.stderr);
    assert_eq!(actions(&server.requests()), vec!["ReadScreenText"]);
    let exported: Vec<String> =
        files_with_prefix(&sb.dir, "w1-").into_iter().filter(|n| n.ends_with(".txt")).collect();
    assert!(exported.is_empty(), "export wrote {:?}", exported);
}

#[test]
fn dispatch_names_carry_run_id() {
    let sb = Sandbox::new("ta");
    let server = FakeServer::start(
        &sb.sock(),
        dispatch_responder(
            json!({"status":"Data","data":{"ready":true}}),
            json!({"status":"Data","data":{"reached":true,"state":"complete"}}),
        ),
    );
    let out = sb.run(&["dispatch", "--plan", PLAN_DESC], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let name = created_name(&server.requests());
    // ^dispatch-plan-\d+-sub0$
    let run = name
        .strip_prefix("dispatch-plan-")
        .and_then(|rest| rest.strip_suffix("-sub0"))
        .unwrap_or_else(|| panic!("unexpected sub name: {}", name));
    assert!(!run.is_empty() && run.chars().all(|c| c.is_ascii_digit()), "run id in {}", name);
}

#[test]
fn dispatch_closes_inject_failed_subs() {
    let sb = Sandbox::new("tb");
    let respond = dispatch_responder(
        json!({"status":"Data","data":{"ready":true}}),
        json!({"status":"Data","data":{"reached":true,"state":"complete"}}),
    );
    // Inject: close the connection without replying.
    let server = FakeServer::start(&sb.sock(), move |req: &Value| {
        if req["action"] == "Inject" { Value::Null } else { respond(req) }
    });
    let out = sb.run(&["dispatch", "--plan", PLAN_DESC], true);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let reqs = server.requests();
    let sub = created_name(&reqs);
    assert!(!actions(&reqs).contains(&"WaitUntil"), "{:?}", actions(&reqs));
    assert!(
        reqs.iter().any(|r| r["action"] == "CloseWorkspace" && r["workspace"] == sub.as_str()),
        "CloseWorkspace missing: {:?}",
        actions(&reqs)
    );
    let result: Value = serde_json::from_str(&out.stdout).expect("dispatch stdout is JSON");
    assert_eq!(result["reports"][0]["status"], "inject_failed", "{}", result);
}

#[test]
fn tasks_add_replaces_inode() {
    use std::os::unix::fs::MetadataExt;
    let sb = Sandbox::new("tc");
    let board_path = sb.dir.join("state/task-queue.json");
    let first = sb.run(&["tasks", "add", "first task"], false);
    assert_eq!(first.code, Some(0), "stderr: {}", first.stderr);
    let ino_before = fs::metadata(&board_path).unwrap().ino();
    let second = sb.run(&["tasks", "add", "second task"], false);
    assert_eq!(second.code, Some(0), "stderr: {}", second.stderr);
    let ino_after = fs::metadata(&board_path).unwrap().ino();
    assert_ne!(ino_before, ino_after, "tasks add rewrote the board in place");
    let board: Value = serde_json::from_str(&fs::read_to_string(&board_path).unwrap()).unwrap();
    assert_eq!(board["tasks"].as_array().unwrap().len(), 2);
    let leftovers = files_with_prefix(&sb.dir.join("state"), ".tmp-");
    assert!(leftovers.is_empty(), "temp files left behind: {:?}", leftovers);
}

#[test]
fn lessons_add_is_atomic() {
    use std::os::unix::fs::MetadataExt;
    let sb = Sandbox::new("td");
    let lessons_path = sb.dir.join("state/lessons.json");
    let first = sb.run(&["lessons", "add", "first lesson"], false);
    assert_eq!(first.code, Some(0), "stderr: {}", first.stderr);
    let ino_before = fs::metadata(&lessons_path).unwrap().ino();
    let second = sb.run(&["lessons", "add", "second lesson", "--type", "failed"], false);
    assert_eq!(second.code, Some(0), "stderr: {}", second.stderr);
    let ino_after = fs::metadata(&lessons_path).unwrap().ino();
    assert_ne!(ino_before, ino_after, "lessons add rewrote the file in place");
    let lessons: Value = serde_json::from_str(&fs::read_to_string(&lessons_path).unwrap()).unwrap();
    assert_eq!(lessons["invariants"], json!(["first lesson"]));
    assert_eq!(lessons["failed"], json!(["second lesson"]));
    let leftovers = files_with_prefix(&sb.dir.join("state"), ".tmp-");
    assert!(leftovers.is_empty(), "temp files left behind: {:?}", leftovers);
}

#[test]
fn help_has_no_hardcoded_installed_block() {
    let sb = Sandbox::new("te");
    for (lang, marker) in [("en", "## Commands"), ("ko", "## 명령어")] {
        let out = sb.run_env(&["help"], false, &[("ATERM_UI_LANG", lang)]);
        assert_eq!(out.code, Some(0), "{}: stderr: {}", lang, out.stderr);
        assert!(out.stdout.contains(marker), "{}: help not in that language: {}", lang, out.stdout);
        assert!(!out.stdout.contains("already installed"), "{}: {}", lang, out.stdout);
        assert!(!out.stdout.contains("이미 설치됨"), "{}: {}", lang, out.stdout);
    }
}

#[test]
fn subscribe_help_lists_emitted_types() {
    let sb = Sandbox::new("tf");
    for lang in ["en", "ko"] {
        let out = sb.run_env(&["help"], false, &[("ATERM_UI_LANG", lang)]);
        assert_eq!(out.code, Some(0), "{}: stderr: {}", lang, out.stderr);
        let row = out
            .stdout
            .lines()
            .find(|l| l.contains("aterm subscribe"))
            .unwrap_or_else(|| panic!("{}: no subscribe row", lang));
        assert!(
            row.contains("(WorkspaceCreated,WorkspaceClosed,StatusChanged,ShellReady,TrustPromptDetected)"),
            "{}: {}",
            lang,
            row
        );
        assert!(!row.contains("TitleChanged"), "{}: {}", lang, row);
    }
}
