# aterm v3 — iced 제거 + winit/wgpu 직접 구현

## 배경
iced 프레임워크가 매 프레임 IME를 리셋하여 한글 자모 분리 버그를 유발. 18회+ 패치 시도 실패. iced를 제거하고 winit+wgpu로 직접 구현.

## 아키텍처

### 의존성
| 크레이트 | 역할 |
|---------|------|
| winit | 윈도우 관리 + 이벤트 루프 |
| wgpu | GPU 렌더링 |
| glyphon | wgpu 텍스트 렌더링 (cosmic-text 기반) |
| alacritty_terminal | 터미널 에뮬레이션 (VT 파싱 + 그리드) |
| portable-pty | PTY 관리 |

### 재사용 코드 (2348줄)
- `core/pty.rs` (928줄) — PTY 관리
- `core/inject.rs` (205줄) — inject 큐, idle state
- `core/session.rs` (174줄) — 세션 영속성
- `core/telepty.rs` (184줄) — telepty 데몬 클라이언트
- `terminal/mod.rs` (144줄) — TerminalState (alacritty_terminal 래핑)
- `ime/macos.rs` (445줄) — macOS IME ClassBuilder
- `ui/cli_presets.rs` (42줄) — CLI 프리셋

### 재작성 코드 (4080줄 → ~2000줄 목표)
- `main.rs` — winit EventLoop + wgpu 초기화 + 앱 상태
- `renderer.rs` — wgpu + glyphon 터미널 그리드 렌더러

## Phase 1: 최소 터미널

### 데이터 흐름
```
키보드 → winit KeyboardInput → PTY write
PTY read → alacritty_terminal Term::advance → 그리드
렌더 → Term 그리드 → glyphon → wgpu 서피스
```

### main.rs 구조
```rust
fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop);
    let (device, queue, surface) = setup_wgpu(&window);
    let renderer = TerminalRenderer::new(&device, &queue);
    let terminal = TerminalState::new(cols, rows);
    let pty = PtyManager::shared();

    event_loop.run(|event, target| {
        match event {
            Event::WindowEvent { event: WindowEvent::KeyboardInput { .. }, .. } => {
                // → PTY write
            }
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                // drain PTY → terminal.feed_output
                // render terminal grid via glyphon + wgpu
            }
            _ => {}
        }
    });
}
```

### renderer.rs
- `TerminalRenderer::new(device, queue)` — glyphon FontSystem + SwashCache 초기화
- `TerminalRenderer::render(terminal, surface, device, queue)` — 그리드 → 텍스트 버퍼 → GPU

### IME
- winit의 `set_ime_allowed(true)` + `set_ime_purpose(Terminal)` at startup (alacritty 패턴)
- iced 없으므로 매 프레임 리셋 문제 없음
- WindowEvent::Ime(Commit/Preedit) 직접 처리

## Phase 2-5 (후속)
- Phase 2: 사이드바 (세션/태스크 보드)
- Phase 3: 커맨드 팔레트 (Cmd+K)
- Phase 4: 상태바
- Phase 5: IME 통합 (ime/macos.rs 재연결)

## 검증 기준 (Phase 1)
- [ ] 윈도우 생성 + wgpu 서피스 렌더
- [ ] PTY 생성 + 셸 프롬프트 표시
- [ ] 키보드 입력 → PTY → 출력 표시
- [ ] 한글 IME 조합 (set_ime_allowed 패턴)
- [ ] 리사이즈 정상
- [ ] cargo check + cargo run 성공
