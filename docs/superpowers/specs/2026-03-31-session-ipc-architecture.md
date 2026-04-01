# aterm v3 세션 IPC 아키텍처 — 최종 설계

> 3차례 멀티AI 토론 (58턴, 12명) + Grok 비판 반영 + 오케스트레이터 종합

## Phase 1 범위 (이 문서)

aterm 내부 세션 간 통신을 가능하게 하는 최소 IPC 인프라.

## 핵심 결정 (전원 합의)

1. **AtermApp singleton** — Ghostty App / Alacritty Processor 패턴
2. **Embedded IPC server** — telepty 비의존, <5us, fate-sharing
3. **PlatformHost 6메서드** — Ghostty 6 callbacks 동형
4. **C ABI 2함수** — `aterm_dispatch` + `aterm_set_host`
5. **2-crate 분리** — `aterm-session` (types) + `aterm-ipc` (server)
6. **Flat WorkspaceManager** — auto-split은 Phase 2 policy layer
7. **SessionAction append-only enum** — serde JSON, capability negotiation 없음
8. **telepty = optional external bridge** — 내부 상태 0, byte relay only

## 아키텍처

```
aterm process
+-----------------------------------------------------------+
|  AtermApp (singleton, Rust)                                |
|  +-- WorkspaceManager (flat HashMap<WorkspaceId, Workspace>)|
|  +-- IpcServer (UDS listener, NDJSON, getpeereid)          |
|  +-- TeleptyBridge (optional, register/deregister)         |
|                                                            |
|  C ABI: aterm_dispatch(action_json) -> response_json       |
|         aterm_set_host(callbacks)                          |
+----------------------------+------------------------------+
                             |
              +--------------+--------------+
              |                             |
     PlatformHost callbacks          UDS socket
     (Swift/GTK/Kotlin)         /tmp/aterm-{pid}.sock
              |                             |
     Native UI (sidebar,             External callers:
     terminal views)                 - CLI inside PTY
                                     - telepty daemon
                                     - other aterm instances
```

## Crate 구조

```
aterm-session/                 # pure types, 0 deps, 0 cfg
  src/
    action.rs                  # SessionAction enum
    host.rs                    # PlatformHost trait (6 methods)
    transport.rs               # SessionTransport trait
    types.rs                   # SessionId, WorkspaceInfo, ActionResponse
  include/
    aterm.h                    # C ABI header

aterm-ipc/                     # embedded IPC server
  src/
    server.rs                  # UDS listener + dispatch loop
    ndjson.rs                  # line-delimited JSON codec
    auth.rs                    # getpeereid / SO_PEERCRED

aterm-core/                    # existing, changes marked NEW
  src/
    app.rs                     # NEW: AtermApp singleton
    workspace.rs               # NEW: flat WorkspaceManager
    pty.rs                     # MODIFY: env vars + workspace integration
    telepty_bridge.rs          # NEW: optional registration
    session.rs                 # existing persistence
    lib.rs                     # MODIFY: C ABI exports
```

## SessionAction enum

```rust
// aterm-session/src/action.rs
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(tag = "action")]
pub enum SessionAction {
    // In-Band (idle-gated, queue_inject)
    Inject { workspace: String, text: String, from: Option<String> },

    // OOB Control (immediate, PlatformHost callback)
    ListWorkspaces,
    WorkspaceStatus { workspace: String },
    FocusWorkspace { workspace: String },
    CloseWorkspace { workspace: String },
    CreateWorkspace { name: String, cli: String, cwd: String },

    // Output
    ReadScreenText { workspace: String, max_bytes: Option<usize> },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum ActionResponse {
    Ok,
    Data(serde_json::Value),
    Unsupported,
    Error(String),
}
```

새 action은 enum 끝에 append. 모르는 action -> `Unsupported` 반환.

## PlatformHost trait

```rust
// aterm-session/src/host.rs
pub trait PlatformHost: Send + Sync {
    fn create_workspace_view(&self, id: &str, config: &WorkspaceConfig);
    fn close_workspace_view(&self, id: &str);
    fn focus_workspace(&self, id: &str);
    fn list_workspaces(&self) -> Vec<WorkspaceInfo>;
    fn on_workspace_event(&self, event: WorkspaceEvent);
    fn request_redraw(&self);
}
```

Native shell(Swift/GTK)이 구현. Rust core -> callback -> native shell.
Native shell이 workspace source of truth 유지.

## C ABI

```c
// aterm-session/include/aterm.h

typedef struct {
    void* userdata;
    void (*create_workspace_view)(void* userdata, const char* id, const char* config_json);
    void (*close_workspace_view)(void* userdata, const char* id);
    void (*focus_workspace)(void* userdata, const char* id);
    const char* (*list_workspaces)(void* userdata);  // returns JSON
    void (*on_workspace_event)(void* userdata, const char* event_json);
    void (*request_redraw)(void* userdata);
} AtermHostCallbacks;

// Host -> Core (2 functions only)
void aterm_set_host(void* core, AtermHostCallbacks callbacks);
const char* aterm_dispatch(void* core, const char* action_json);  // returns response JSON
```

## Embedded IPC Server

```rust
// aterm-ipc/src/server.rs
pub struct IpcServer {
    socket_path: String,
}

impl IpcServer {
    pub fn start(
        socket_path: &str,
        dispatcher: Arc<dyn Fn(SessionAction) -> ActionResponse + Send + Sync>,
    ) -> io::Result<Self> {
        let listener = UnixListener::bind(socket_path)?;

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // getpeereid auth
                if !auth::verify_peer(&stream) { continue; }

                let dispatcher = dispatcher.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(&stream);
                    for line in reader.lines().flatten() {
                        match serde_json::from_str::<SessionAction>(&line) {
                            Ok(action) => {
                                let resp = dispatcher(action);
                                let resp_json = serde_json::to_string(&resp).unwrap();
                                let _ = writeln!(&stream, "{}", resp_json);
                            }
                            Err(e) => {
                                let _ = writeln!(&stream, r#"{{"error":"{}"}}"#, e);
                            }
                        }
                    }
                });
            }
        });

        Ok(Self { socket_path: socket_path.to_string() })
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
```

## AtermApp singleton

```rust
// aterm-core/src/app.rs
pub struct AtermApp {
    workspaces: HashMap<String, Workspace>,
    ipc_server: Option<IpcServer>,
    telepty_bridge: Option<TeleptyBridge>,
    host: Option<Box<dyn PlatformHost>>,
}

impl AtermApp {
    pub fn new() -> Self {
        Self {
            workspaces: HashMap::new(),
            ipc_server: None,
            telepty_bridge: None,
            host: None,
        }
    }

    pub fn start_ipc(&mut self) {
        let socket_path = format!("/tmp/aterm-{}.sock", std::process::id());
        let dispatcher = self.make_dispatcher();
        self.ipc_server = IpcServer::start(&socket_path, dispatcher).ok();
    }

    pub fn dispatch(&mut self, action: SessionAction) -> ActionResponse {
        match action {
            SessionAction::Inject { workspace, text, from } => {
                if let Some(ws) = self.workspaces.get(&workspace) {
                    ws.queue_inject(&text, from.as_deref());
                    ActionResponse::Ok
                } else {
                    ActionResponse::Error(format!("workspace '{}' not found", workspace))
                }
            }
            SessionAction::ListWorkspaces => {
                if let Some(host) = &self.host {
                    let list = host.list_workspaces();
                    ActionResponse::Data(serde_json::to_value(list).unwrap())
                } else {
                    ActionResponse::Unsupported
                }
            }
            SessionAction::FocusWorkspace { workspace } => {
                if let Some(host) = &self.host {
                    host.focus_workspace(&workspace);
                    ActionResponse::Ok
                } else {
                    ActionResponse::Unsupported
                }
            }
            // ... other actions
            _ => ActionResponse::Unsupported,
        }
    }
}
```

## PTY 환경변수

spawn 시 주입:

| 변수 | 값 | 용도 |
|------|---|------|
| `ATERM_WORKSPACE_NAME` | workspace name | 세션 식별 |
| `ATERM_WORKSPACE_CLI` | cli type | claude/codex/gemini |
| `ATERM_IPC_SOCKET` | socket path | IPC 접근 |
| `ATERM_IPC_TOKEN` | auth token | telepty delivery용 |
| `TELEPTY_SESSION_ID` | opaque ID | telepty 등록 ID |

## telepty Optional Bridge

```rust
// aterm-core/src/telepty_bridge.rs
pub struct TeleptyBridge {
    daemon_url: String, // http://127.0.0.1:3848
}

impl TeleptyBridge {
    /// 실패해도 aterm 정상 동작. fire-and-forget.
    pub fn register(&self, ws_id: &str, alias: &str, socket_path: &str) {
        let payload = serde_json::json!({
            "session_id": ws_id,
            "alias": alias,
            "delivery": {
                "transport": "unix_socket",
                "address": socket_path,
            }
        });
        let _ = ureq::post(&format!("{}/api/sessions/register", self.daemon_url))
            .send_json(&payload);
    }

    pub fn deregister(&self, ws_id: &str) {
        let _ = ureq::delete(
            &format!("{}/api/sessions/{}", self.daemon_url, ws_id)
        ).call();
    }
}
```

## IPC 흐름

| 경로 | 지연 |
|------|:----:|
| aterm 내부 (workspace A -> B) | <5us (in-process dispatch) |
| CLI inside PTY -> aterm | ~1ms (UDS) |
| 같은 머신 다른 터미널 -> aterm | ~2ms (telepty -> UDS) |
| 원격 -> aterm | ~50ms (telepty network -> UDS) |

## 구현 순서

| 순서 | 컴포넌트 | 파일 | LOC |
|:----:|---------|------|:---:|
| 1 | aterm-session crate | action.rs, host.rs, transport.rs, types.rs, aterm.h | ~120 |
| 2 | aterm-ipc crate | server.rs, ndjson.rs, auth.rs | ~150 |
| 3 | AtermApp singleton + flat WorkspaceManager | app.rs, workspace.rs | ~100 |
| 4 | C ABI exports | lib.rs | ~40 |
| 5 | Swift AtermHostCallbacks | AppDelegate.swift | ~60 |
| 6 | PTY env vars | pty.rs | ~20 |
| 7 | telepty optional bridge | telepty_bridge.rs | ~40 |
| **합계** | | | **~530** |

## 성공 기준

1. aterm 내부 CLI에서 `echo '{"action":"Inject","workspace":"other","text":"hello"}' | socat - UNIX-CONNECT:/tmp/aterm-{pid}.sock` 로 다른 워크스페이스에 메시지 전달
2. `telepty inject orchestrator "hello"` 로 aterm 워크스페이스에 메시지 전달
3. telepty 없이도 aterm 독립 동작 (IPC server 자체 내장)
4. `ListWorkspaces` action으로 현재 워크스페이스 목록 조회
5. `FocusWorkspace` action으로 워크스페이스 포커스 변경
