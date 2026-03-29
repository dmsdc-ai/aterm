# AGENTS.md — aterm v3

## Overview

aterm v3 is a GPU-accelerated terminal emulator. Pure Rust, no framework.

## Architecture

```
winit (event loop + window)
  ├── wgpu (GPU rendering)
  │     └── glyphon (text shaping/rasterizing)
  ├── portable-pty (PTY process management)
  ├── alacritty_terminal (VTE parser + Term state)
  └── macOS NSTextInputClient (native IME)
```

## Directory Structure

```
src-v3/
  main.rs              — winit ApplicationHandler, event loop, input, draw
  renderer.rs          — wgpu + glyphon terminal grid renderer
  terminal/mod.rs      — TerminalState: VTE parsing, Term management
  core/
    pty.rs             — PtyManager, OutputBuffer, reader_loop, PtyOutputSignal
    inject.rs          — InjectQueue, injector polling loop
    session.rs         — Session persistence
    telepty.rs         — telepty CLI integration
    cli_presets.rs     — claude/codex/gemini CLI presets
  ime/
    macos.rs           — Native macOS NSTextInputClient
Cargo.toml
```

## Build / Run

```bash
cargo run --release --bin aterm-v3    # MUST use --release (debug is 10-50x slower)
cargo test --bin aterm-v3
cargo check --bin aterm-v3
```

## Performance Constraints (MANDATORY)

### Prohibited Patterns

| Pattern | Reason |
|---------|--------|
| Full VTE replay per frame | O(total_output) x fps = catastrophic. Gets worse as buffer grows |
| New Term allocation per frame | 10,000-line scrollback Term + parser init cost |
| New glyphon Buffer per frame | Full grid re-layout + re-shaping unnecessary |
| Snapshot string comparison for change detection | 256KB byte compare is wrong approach |
| Debug build | 10-50x slower than release |

### Required Patterns

1. **Incremental VTE parsing** — persistent Term across frames, only parse NEW bytes via `parser.advance()`
2. **Change-based rendering** — cache glyphon Buffer, update only changed cells
3. **Input-first event loop** — consume ALL pending input events before draw()
4. **60fps (16ms) is sufficient** for a terminal. No need for 120fps.
5. **Shaping::Advanced required** — Shaping::Basic disables font fallback entirely, breaking CJK rendering (Korean shows as tofu). Always use Shaping::Advanced for glyphon.

### Reference: alacritty

alacritty uses the same `alacritty_terminal` crate correctly:
1. Persistent `Term` — one instance for app lifetime
2. PTY reader → new bytes → `term.lock()` → `parser.advance(term, new_bytes)` → unlock
3. Renderer reads `term.renderable_content()` only
4. No full replay, no snapshot comparison

## Current Data Flow (fixed 2026-03-29)

```
reader_loop → drain_term_bytes() byte queue → signal.mark_dirty() (OnceLock lock-free)
→ main: drain new bytes only → term.lock() → parser.advance(term, new_bytes) → unlock
→ render: term.renderable_content() → persistent Buffer reuse + Shaping::Advanced + single render pass
```

## Known Issues (2026-03-29)

| Priority | Issue | Status | Detail |
|----------|-------|--------|--------|
| P1 | Korean jamo separation | OPEN | 한영 전환 후 자모 분리 지속 (예: '안녕' → 'ㅇㅏㄴㄴㅕㅇ'). main.rs에 native_ime_active 가드 추가했으나 동작하지 않음. ime/macos.rs NSTextInputClient 근본 재검토 필요 |
| P1 | Typing delay persists | OPEN | 증분 VTE 파싱 적용했으나 여전히 실시간 느낌 아님. 추가 병목 조사 필요 |
| P2 | Font size still small | OPEN | 14→16px 변경했으나 사용자 체감 여전히 작음. 18-20px 또는 설정 가능하게 |
| P3 | CJK proportional fallback font | OPEN | Korean falls back to proportional font, cell width mismatch possible |
| P3 | Cursor blink animation | OPEN | Static block cursor, no blinking |
| P3 | Underline/beam cursor shapes | OPEN | Only block cursor supported |

## Remaining P3 Items (optional)

| File | Item | Impact |
|------|------|--------|
| `core/inject.rs` | 500ms polling → condvar/notify | inject only, not typing |
| `ime/macos.rs` | key_down Mutex x4 → RefCell | minimal |
| `main.rs` | PowerPreference::LowPower → None | trivial |

## Dependencies

| Crate | Version | Role |
|-------|---------|------|
| alacritty_terminal | 0.26.0-rc1 | VTE parser + Term state |
| winit | 0.30 | Event loop + window |
| wgpu | 23 | GPU rendering |
| glyphon | 0.7 | Text shaping/rasterizing |
| portable-pty | 0.9 | PTY process management |
| tokio | 1 | Async runtime (sync features only) |

## Work Principles

- Technical decisions: autonomous, based on this file. Do not ask the user.
- On error: do not stop. Fix autonomously. Report to orchestrator after 3 failures.
- After 3 failures: delegate to another LLM session.
- Report: `telepty inject --from {your-session-id} aigentry-orchestrator-claude "report"`

## Ecosystem

- Orchestrator: `aigentry-orchestrator-claude`
- Logger: `aigentry-logger-claude`
- Tester: `aigentry-tester-claude`
- Design: `aigentry-design-claude`
