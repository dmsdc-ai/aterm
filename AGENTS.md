# AGENTS.md — aterm v3

## Overview

aterm v3 is a GPU-accelerated terminal emulator. Pure Rust, no framework.

## Architecture

```
aterm-core (Rust cdylib, 공유 코어)
  ├── wgpu (GPU rendering — Metal/Vulkan/DX12)
  │     └── glyphon (text shaping/rasterizing)
  ├── portable-pty (PTY process management)
  ├── alacritty_terminal (VTE parser + Term state)
  └── C-FFI 인터페이스 (cbindgen → aterm_core.h)

macOS Shell (Swift/AppKit — 네이티브 쉘)
  ├── NSView + CAMetalLayer (wgpu Metal 백엔드)
  ├── NSTextInputClient (한글 IME — Ghostty 패턴)
  ├── NSSplitView (세션보드 사이드바 + 터미널)
  ├── SwiftUI SessionSidebarView (telepty bus WebSocket 실시간 세션 상태)
  └── CVDisplayLink (vsync 렌더링)

Legacy (src-v3/ — winit 기반, 유지 중)
  └── winit = { path = "../winit" } (로컬 패치)
```

5-platform: macOS/Linux/Windows (데스크탑 풀 터미널) + Android/iOS (tailscale mesh로 원격 PTY 접속)
macOS는 winit 제거, Swift/AppKit 네이티브 쉘로 전환 완료. IME는 NSTextInputClient로 OS 네이티브 처리 (winit IME 한글 조합 불가 7회+ 실패 확인).

## Directory Structure

```
aterm-core/                # Rust 코어 (cdylib + C-FFI)
  Cargo.toml
  cbindgen.toml
  build.rs                 — cbindgen 자동 헤더 생성
  src/
    lib.rs                 — AtermCore struct + #[no_mangle] extern "C" fn FFI
    renderer.rs            — wgpu + glyphon terminal grid renderer
    terminal.rs            — TerminalState: incremental VTE parsing
    pty.rs                 — PtyManager, PtyOutputSignal
    inject.rs              — InjectQueue, injector polling loop
    session.rs             — Session persistence
    telepty.rs             — telepty CLI integration
    cli_presets.rs         — claude/codex/gemini CLI presets

macos/                     # Swift 쉘 (macOS 네이티브)
  aterm-bridge.h           — C-FFI bridging header
  Sources/
    main.swift             — NSApplication entry point
    AppDelegate.swift      — NSWindow + NSSplitView 레이아웃
    TerminalView.swift     — NSView + CAMetalLayer + NSTextInputClient
    SessionSidebarView.swift — SwiftUI 세션보드 사이드바
    TeleptyBusClient.swift — telepty bus WebSocket 클라이언트

src-v3/                    # Legacy (winit 기반, 점진적 제거 예정)
  main.rs                  — winit ApplicationHandler
  renderer.rs, terminal/, core/, ime/

Makefile                   — make run (Rust build → Swift compile → .app bundle)
```

## Build / Run

```bash
# macOS 네이티브 앱 (권장)
make run                   # Rust cdylib + Swift → .app 번들 실행
make run-dev               # 개발용 (번들 생략, 빠름)
make clean                 # 빌드 아티팩트 정리

# Legacy winit 바이너리
cargo run --release --bin aterm-v3
```

## Performance Constraints (MANDATORY)

### Prohibited Patterns

| Pattern | Reason |
|---------|--------|
| Full VTE replay per frame | O(total_output) x fps = catastrophic |
| New Term/Buffer allocation per frame | heap alloc cost |
| Shaping::Basic with CJK content | font fallback disabled → Korean tofu |
| Shaping::Advanced with ASCII-only | 130ms/frame, use Basic for ASCII |
| Debug build | 10-50x slower than release |

### Required Patterns

1. **Incremental VTE parsing** — persistent Term, only parse NEW bytes via `parser.advance()`
2. **Per-line Buffer caching** — reuse glyphon Buffer per line, re-shape only changed lines
3. **Adaptive shaping** — ASCII-only lines → Shaping::Basic, non-ASCII → Shaping::Advanced
4. **Input-first event loop** — consume ALL input events before draw()
5. **IME via winit Preedit/Commit** — ime_composing flag guards KeyboardInput.text
6. **set_monospace_width(cell_width)** — forces fallback fonts (Korean) to fixed-width cells

## Current Data Flow

```
reader_loop → drain_term_bytes() → signal.mark_dirty() (OnceLock)
→ main: drain new bytes → term.lock() → parser.advance(term, new_bytes)
→ render: per-line Buffer reuse + adaptive shaping + single render pass
```

## Known Issues (2026-03-29)

| Priority | Issue | Status | Detail |
|----------|-------|--------|--------|
| P1 | First consonant lost after input switch | OPEN | 한영 전환 후 첫 자음 유실 (예: '안녕' → 'ㅏㄴㄴㅕㅇ'). winit Disabled→Enabled 시퀀스가 첫 키 소비 가능성. main.rs에 Ime::Disabled/Enabled 핸들러 추가 필요 |
| P1 | Render regression 113ms | OPEN | full rebuild 후 per-line buffer 최적화 동작 확인 필요. 목표 18ms |
| P3 | CJK cell width mismatch | OPEN | proportional fallback font, set_monospace_width로 보정 중 |
| P3 | Cursor blink/shapes | OPEN | Static block only |

## Dependencies

| Crate | Source | Role |
|-------|--------|------|
| alacritty_terminal | 0.26.0-rc1 (crates.io) | VTE parser + Term state |
| winit | **path = ../winit** (local patch) | Event loop + IME |
| wgpu | 23 (crates.io) | GPU rendering |
| glyphon | 0.7 (crates.io) | Text shaping |
| portable-pty | 0.9 (crates.io) | PTY management |

## Work Principles

- Technical decisions: autonomous, based on this file. Do not ask the user.
- On error: do not stop. Fix autonomously. Report to orchestrator after 3 failures.
- After 3 failures: delegate to another LLM session.
- Report: `telepty inject --from {your-session-id} aigentry-orchestrator-claude "report"`

## Ecosystem

- Orchestrator: `aigentry-orchestrator-claude`
- Logger: `aigentry-logger-claude`
- Tester: `aigentry-tester-claude`
