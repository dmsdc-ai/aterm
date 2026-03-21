# aterm — AI Agent Orchestration Messenger

aigentry 에코시스템의 **전용 터미널/메신저**. Electron 데스크탑 앱 + CLI.

## 아키텍처

```
Electron Main Process (node-pty + Unix Socket)
  ├── IPC ──→ Svelte Renderer (xterm.js)
  └── Unix Socket ──→ CLI (bin/aterm.js)

Renderer ──→ telepty daemon (localhost:3848) [optional]
```

### 디렉토리 구조

```
src/
  main/          — Electron main process (Node.js)
    index.js     — BrowserWindow + IPC + PTY + socket server
  preload/       — preload scripts
    index.js     — contextBridge (atermAPI)
  renderer/      — Svelte frontend
    index.html
    src/
      App.svelte, main.js, app.css
      lib/       — aterm-client.js, stores.js, telepty-client.js
      components/ — SessionTree, Terminal, Timeline, CommandPalette
      design/    — 디자인 토큰 (colors, spacing, typography, animations)
  server/        — 공유 서버 코드 (main process + standalone 모드 공용)
    index.js     — standalone HTTP + WS 서버
    pty-manager.js — PTY 프로세스 관리
    socket-server.js — Unix domain socket 서버
bin/
  aterm.js       — CLI tool (Unix socket 직접 사용)
```

### 핵심 모듈

| 파일 | 역할 |
|------|------|
| `src/main/index.js` | Electron main: BrowserWindow, IPC, PTY, socket |
| `src/preload/index.js` | contextBridge: atermAPI 노출 |
| `src/renderer/src/lib/aterm-client.js` | IPC/WS 듀얼 클라이언트 (자동 감지) |
| `src/renderer/src/lib/telepty-client.js` | telepty daemon WS/HTTP 클라이언트 |
| `src/renderer/src/lib/stores.js` | Svelte 반응형 스토어 |
| `src/renderer/src/components/Terminal.svelte` | 중앙 xterm.js PTY 터미널 |
| `src/renderer/src/components/SessionTree.svelte` | 좌측 세션 트리 |
| `src/renderer/src/components/Timeline.svelte` | 우측 이벤트 타임라인 |
| `src/renderer/src/components/CommandPalette.svelte` | Cmd+K 커맨드 팔레트 |
| `src/server/pty-manager.js` | PTY 프로세스 관리 (공유) |
| `src/server/socket-server.js` | Unix domain socket 서버 (공유) |

## 명령어

```bash
# Electron 데스크탑 앱
npm run dev      # electron-vite dev (핫 리로드)
npm run build    # electron-vite build (프로덕션)
npm run preview  # electron-vite preview

# Standalone 웹 모드 (Electron 없이)
npm run server   # node src/server/index.js (HTTP + WS 서버)
npm run web:dev  # vite dev (브라우저 프론트엔드)
npm run web:build # vite build (dist/)

# CLI
aterm status     # 서버 상태 확인
aterm ls         # 워크스페이스 목록
```

## 통신 모드

| 모드 | 경로 | 용도 |
|------|------|------|
| Electron IPC | main ↔ renderer (contextBridge) | 데스크탑 앱 |
| WebSocket | ws://localhost:3849 | 브라우저 standalone |
| Unix Socket | ~/.aterm/aterm.sock | CLI 도구 |

aterm-client.js는 `window.atermAPI` 존재 여부로 IPC/WS 모드를 자동 감지.

## 의존성

- node-pty: 네이티브 모듈 (빌드 시 external)
- telepty daemon (localhost:3848): 선택적 — 세션 관리용

## Tech Stack

Electron + electron-vite + Svelte 5 + xterm.js + TailwindCSS
