# aterm v3 — GPU-Accelerated Terminal Emulator

aigentry 에코시스템의 전용 터미널. 5-platform native.

## Architecture

```
aterm-core (Rust cdylib + lib, C-FFI)
  ├── alacritty_terminal (VTE parser + Term state)
  ├── portable-pty (PTY process management)
  ├── unicode-normalization (PTY reader NFC)
  ├── aterm-session + aterm-ipc (IPC contract + server)
  ├── tsnet (embedded Tailscale node; Go 1.20.14 at build time)
  └── C header: hand-written macos/aterm-bridge.h
        (drift test: lib.rs ffi_tests::bridge_header_matches_exports)

aterm-session (Rust lib) — IPC wire contract: SessionAction / ActionResponse / AtermEvent, PlatformHost
aterm-ipc     (Rust lib) — Unix-socket NDJSON server ($ATERM_IPC_SOCKET), peer-UID auth

macOS Shell (Swift/AppKit)
  ├── Metal renderer: MetalRenderer + Shaders.metal, GlyphAtlas (Core Text)
  │     └── cells from aterm_core_get_render_data (Rust does no rendering)
  ├── NSView + CAMetalLayer + NSTextInputClient (한글 IME)
  ├── 사이드바 + 터미널 컨테이너 (AppDelegate)
  └── .app bundle 필수 (bare binary는 IME 미동작)
```

No wgpu, glyphon, renderer.rs or cbindgen: rendering moved to Swift Metal, and the header is maintained by hand.

## Directory

```
Cargo.toml           — workspace only (aterm-core, aterm-session, aterm-ipc), resolver 2; no root crate
aterm-core/src/      — Rust: lib.rs (C-FFI), app.rs (IPC action routing), pty.rs, terminal.rs, inject.rs,
                       session.rs, telepty_bridge.rs, tailscale.rs, mailbox/
aterm-core/tests/    — cli_dispatch.rs (bin/aterm), hermetic_guard.rs, telepty_no_restart.rs
aterm-session/src/   — action.rs, host.rs, types.rs
aterm-ipc/src/       — server.rs, auth.rs
macos/aterm-bridge.h — C header imported by Swift
macos/Sources/       — Swift: AppDelegate, TerminalView, MetalRenderer, GlyphAtlas, Shaders.metal,
                       SessionSidebarView, SettingsView, OnboardingView, Orchestrator*, Telepty*
bin/aterm            — CLI (bash + python3), bundled as aterm.app/Contents/Resources/bin/aterm
npm/                 — aterm launcher package + aterm-darwin-arm64 native package
state/               — tests.md (test inventory) + verification-families/ (runner.sh)
Makefile             — make run (.app bundle)
```

## Build

```bash
# 항상 레포 루트에서 실행 (/Users/duckyoungkim/projects/aigentry-aterm)
make app             # 빌드만 (build/aterm.app)
make run             # 빌드 + 실행 (.app bundle 필수 — IME + codesign)
make rust            # Rust cdylib만 빌드
make swift           # Swift만 빌드 (rust 선행 필요)
make install         # ~/Applications/aterm.app 설치
make dist            # build/aterm.zip 배포 아카이브
make clean           # 빌드 정리
```

## npm 배포

```bash
cd npm/aterm-darwin-arm64 && npm publish --access public
cd npm/aterm && npm publish --access public
# 버전: npm/aterm/package.json + npm/aterm-darwin-arm64/package.json 동시 범프
```

## CI/CD

- `.github/workflows/test-install.yml` — push/PR 시 자동 실행 (macos-14 러너)
- CI = `ATERM_TELEPTY_PORT=9 cargo test --workspace --locked` + npm pack + TTY-emulated `npm install -g` (user layout, `aterm --version` 검증)
- Go 1.20.14 setup은 tsnet 빌드용
- 버전은 package.json에서 동적 읽기

## Role Boundaries (HARD RULE — SAWP)
- This session does CODE ONLY. No build, no test, no app launch.
- Builder session handles: make app, cargo build
- Tester session handles: sandbox app launch, testing, verification
- After code changes, report immediately. Do NOT run cargo test, cargo build, or make.
- If you need to verify compilation, use cargo check (not cargo build).

This is SAWP: Code only → Report → Builder builds → Tester tests.

## Work Principles

- **모든 것은 configurable (HARD RULE)**: 하드코딩 금지. 합리적 기본값 제공하되 잠그지 않는다. Settings UI (Cmd+,) + ~/.aigentry/config/aterm.json으로 변경 가능해야 하는 항목:
  - 외관: 테마/컬러, 폰트(크기/패밀리), 줄간격, 윈도우 투명도, 터미널 패딩, 커서(스타일/블링크), 사이드바(위치/너비)
  - 터미널: 기본 CLI, 기본 CWD, 스크롤백 라인, 탭 크기, 벨 사운드, 우클릭 붙여넣기, 선택시 자동복사, 스크롤 방향
  - 세션: 자동 복원, 자동 재시작, 최대 재시작 횟수, /init 자동 실행, 세션 이름 형식
  - 통합: telepty 브릿지 on/off, devkit auto-init on/off, tailscale on/off
  - 키보드: 키바인딩 커스텀
  - 언어: UI 언어 (한/영)
- 기술 결정은 자율 판단. 사용자에게 물어보지 않는다.
- 에러 시 멈추지 않고 자율 해결. 3회 실패 시 오케스트레이터에 보고.
- 교훈(invariants + failed): `~/projects/aigentry-orchestrator/state/lessons.json` 참조.
- 태스크 현황: `~/projects/aigentry-orchestrator/state/task-queue.json` 참조.
- 보고: `telepty inject --from {session-id} aigentry-orchestrator-claude "report"`
- 헌법: `~/projects/aigentry/docs/CONSTITUTION.md`

## Debugging Methodology

### Evidence-Based Benchmark-First Debugging (HARD RULE)
When fixing rendering, font, performance, or input bugs:
1. Logger captures raw evidence (PTY bytes, hexdump, timing, GPU state)
2. Terminal benchmark sessions (ghostty/alacritty/kitty/wezterm/contour/cmux) research how competitors solved the same problem — code + git history
3. Analyst verifies fix is architecturally feasible with current libraries
4. Implementation based on benchmarked, verified solutions only
5. Tester verifies fix with runtime evidence (not just compile success)

DO NOT attempt fixes based on speculation. DO NOT assume a library can do something without verifying its API surface.

### Architecture Constraints
- fontdue: pure Rust single-TTF rasterizer — NO system font access. Use Core Text FFI for fallback.
- wgpu: clear_color must match terminal background or be transparent. Never use debug colors.
- Resize: atlas persists, surface.configure deferred to render time, PTY SIGWINCH coalesced.
- Resize new cells = default bg. Erase ops (EL/ECH/ED) = cursor SGR bg. Never mix these. After Term::resize(), cursor template bg is reset to default to prevent child process post-SIGWINCH erases from inheriting stale SGR bg (fix #157).

## Session Detection

CLI/AI가 세션을 탐지할 때 아래 우선순위를 반드시 따른다.

### Priority 1: aterm internal sessions (`$ATERM_IPC_SOCKET` set)
```bash
aterm list
```
Output format: `NAME | CLI | CWD | TERMINAL`
```
orchestrator | claude | ~/projects/aigentry-orchestrator | aterm
alpasim      | claude | ~/projects/alpasim              | aterm
```
- TERMINAL column is always `aterm` for internal sessions
- NAME is the workspace/project name (not TTY like s013)

### Priority 2: telepty external sessions (`telepty` installed)
```bash
telepty list
```
Output format: `NAME | CLI | CWD | TERMINAL`
```
aigentry-brain-claude | claude | ~/projects/brain | ghostty
deliberation-claude   | claude | ~/projects/delib | kitty
```
- Shows sessions in OTHER terminals (ghostty, kitty, etc.)
- Requires BOTH: telepty installed + sessions registered via `telepty allow`
- Only used when `$ATERM_IPC_SOCKET` is not available, or to discover non-aterm sessions

### Priority 3: NEVER use as primary
```bash
# DO NOT USE: ps aux | grep claude
# DO NOT USE: who / w / TTY detection
```
- `ps aux` shows ALL system processes — no project names, no terminal info
- TTY names (s003, s013, s016) are meaningless identifiers
- NEVER acceptable — removed from detection chain entirely

### Detection Rules for AI/CLI
1. **FIRST**: `aterm list` — returns project names + terminal=aterm
2. **SECOND**: `telepty list` — returns project names + terminal type
3. **NEVER primary**: `ps aux | grep` — no project names, no terminal info

### Session Inject Rules
- `terminal=aterm` → `aterm inject <workspace> 'message'` (direct IPC, no telepty needed)
- `terminal=other` → `telepty inject <session> 'message'` (requires telepty)
- If telepty not installed → only aterm internal sessions are reachable

### Reachable Sessions Only
Session list only shows reachable sessions. If you can't inject to it, don't show it.

External sessions (other terminals) require:
1. `npm i -g @dmsdc-ai/aigentry-telepty` — install telepty
2. `telepty allow --id <name> <cli>` — register each session

Without both, only aterm internal sessions are visible. No `ps aux` fallback. No 'unknown terminal' entries.

## Dispatch

Autonomous task execution via sub-session orchestration.

### Usage

```bash
aterm dispatch <task-id>              # Read task from state/task-queue.json, break down, execute
aterm dispatch --plan '<description>' # Free-text task → same dispatch flow
```

### Flow

1. **Parse**: `<task-id>` reads from `state/task-queue.json`; `--plan` takes free text
2. **Breakdown**: Tries `aigentry-devkit breakdown '<desc>'`. Falls back to single subtask if devkit unavailable
3. **Judgment**: If only 1 subtask referencing a single file → returns `{"recommendation": "subagent"}` and exits (no sessions created)
4. **CLI selection**: Per subtask, keywords in description select CLI:
   - `implement`, `code`, `build`, `write`, `fix bug` → `codex`
   - `architect`, `debug`, `analyze`, `design` → `claude`
   - `research`, `document`, `search`, `summarize` → `gemini`
   - Default → `claude`
5. **Execute**: For each subtask: `aterm create` → wait 2s → `aterm inject`
6. **Poll**: 10s intervals, max 300s. Looks for `idle` state in workspace status
7. **Collect**: Gathers status reports from all sub-sessions
8. **Cleanup**: `aterm kill` for each created session

### Output

```json
{
  "task_id": 34,
  "subtasks": 3,
  "sessions_created": ["dispatch-34-sub0", "dispatch-34-sub1", "dispatch-34-sub2"],
  "status": "all_complete",
  "reports": [{"name": "...", "status": "complete", "cli": "..."}, ...]
}
```

### Requirements

- `$ATERM_IPC_SOCKET` must be set (runs inside aterm only)
- Python 3 available
- Optional: `aigentry-devkit` for intelligent task breakdown
