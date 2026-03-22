# aterm — AI Agent Orchestration Terminal

aigentry 에코시스템의 **전용 터미널/메신저**. Tauri v2 데스크탑 앱.

## 아키텍처

```
Tauri Rust Backend (portable-pty)
  └── Tauri IPC (commands + events) ──→ Svelte Renderer (xterm.js)
```

### 디렉토리 구조

```
src/
  App.svelte        — 메인 앱 (3-패널 레이아웃)
  main.js           — Svelte 마운트
  app.css            — 전역 스타일
  lib/
    aterm-client.js  — Tauri IPC 클라이언트 (invoke/listen)
    stores.js        — Svelte 반응형 스토어
    telepty-client.js — telepty daemon 클라이언트
  components/
    SessionTree.svelte    — 좌측 세션 트리
    Terminal.svelte       — 중앙 xterm.js PTY 터미널
    Timeline.svelte       — 우측 이벤트 타임라인
    CommandPalette.svelte — Cmd+K 커맨드 팔레트
src-tauri/
  src/lib.rs         — Rust 백엔드 (portable-pty, Tauri commands)
  Cargo.toml
  tauri.conf.json
```

### Rust 백엔드 (src-tauri/src/lib.rs)

| Command | 역할 |
|---------|------|
| `list_workspaces` | 워크스페이스 목록 |
| `create_workspace` | PTY 생성 (portable-pty) |
| `close_workspace` | PTY 종료 |
| `send_to_workspace` | PTY에 텍스트 전송 |
| `send_key` | 키 매핑 후 PTY 전송 |
| `read_screen` | 버퍼 라인 반환 |
| `resize_workspace` | PTY 리사이즈 |
| `suggest_paths` | 경로 자동완성 |
| Event: `pty-output` | PTY 출력 이벤트 스트림 |

## 명령어

```bash
npm run dev      # tauri dev (핫 리로드)
npm run build    # tauri build (프로덕션)
```

## 의존성

- portable-pty 0.9: Rust PTY 관리
- @xterm/xterm 6.0: 터미널 렌더링 (WKWebView Korean IME 패치 적용)
- @tauri-apps/api: IPC 통신
- TailwindCSS: 스타일링

## Tech Stack

Tauri v2 + Rust + Svelte 5 + xterm.js + TailwindCSS

## 알려진 패치

- **xterm.js WKWebView Korean IME**: PR #5704 backport 적용.
  WKWebView는 composition 이벤트 대신 `insertReplacementText` inputType 사용.
  `node_modules/@xterm/xterm/lib/xterm.{js,mjs}` 직접 패치.
  `npm install` 시 패치 재적용 필요 (patch-package 도입 예정).

## AI 작업 원칙

### Multi-LLM 위임 원칙 (3회 실패 시 위임)

동일 문제에 **3회 이상 시도해도 해결 안 되면**, 다른 LLM에 위임한다.

| 단계 | 행동 |
|------|------|
| 1~2회 시도 | 직접 해결 시도 |
| 3회 실패 | 접근 방식 재검토, upstream 이슈 검색 |
| 4회+ 실패 | **즉시 다른 LLM(Codex 등)에 위임** |

**위임 방법:**
```bash
# telepty로 다른 LLM 세션 생성 후 위임
telepty allow --id {project}-codex codex resume
telepty inject --from {my-session} {project}-codex "문제 설명 + 컨텍스트"
```

**왜?** 같은 LLM이 같은 가설에 갇히면 해결 불가. 다른 LLM은 다른 관점(upstream 검색, 다른 분석)으로 접근하여 빠르게 해결할 수 있음.
실제 사례: Korean IME 버그 — Claude 10회+ 실패 → Codex가 upstream PR #5704 즉시 발견.
