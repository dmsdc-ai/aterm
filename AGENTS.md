# aterm v3 — GPU-Accelerated Terminal Emulator

aigentry 에코시스템의 전용 터미널. 5-platform native.

## Architecture

```
aterm-core (Rust cdylib + C-FFI)
  ├── wgpu (GPU rendering — Metal/Vulkan/DX12)
  │     └── glyphon (text shaping/rasterizing)
  ├── portable-pty (PTY process management)
  ├── alacritty_terminal (VTE parser + Term state)
  └── cbindgen → aterm_core.h

macOS Shell (Swift/AppKit)
  ├── NSView + CAMetalLayer + NSTextInputClient (한글 IME)
  ├── NSSplitView (사이드바 + 터미널)
  └── .app bundle 필수 (bare binary는 IME 미동작)
```

## Directory

```
aterm-core/src/     — Rust: lib.rs, renderer.rs, terminal.rs, pty.rs, inject.rs
macos/Sources/      — Swift: AppDelegate, TerminalView, SessionSidebarView, TeleptyBusClient
Makefile            — make run (.app bundle)
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

- `.github/workflows/test-install.yml` — push/PR 시 자동 실행
- macOS 14 ARM 러너, winit은 git clone (v0.30.13 tag)
- 버전은 package.json에서 동적 읽기

## Work Principles

- 기술 결정은 자율 판단. 사용자에게 물어보지 않는다.
- 에러 시 멈추지 않고 자율 해결. 3회 실패 시 오케스트레이터에 보고.
- 교훈(invariants + failed): `~/projects/aigentry-orchestrator/state/lessons.json` 참조.
- 태스크 현황: `~/projects/aigentry-orchestrator/state/task-queue.json` 참조.
- 보고: `telepty inject --from {session-id} aigentry-orchestrator-claude "report"`
- 헌법: `~/projects/aigentry/docs/CONSTITUTION.md`

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
