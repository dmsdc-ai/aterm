# aterm — AI Agent Orchestration Messenger

aigentry 에코시스템의 **전용 터미널/메신저**. 메신저 껍데기 + CLI 속살.

## 아키텍처

```
Svelte Frontend (xterm.js + WS) ──→ telepty daemon (localhost:3848)
```

### 핵심 모듈

| 파일 | 역할 |
|------|------|
| `src/lib/telepty-client.js` | telepty daemon WS/HTTP 클라이언트 |
| `src/lib/stores.js` | Svelte 반응형 스토어 (sessions, events) |
| `src/components/SessionTree.svelte` | 좌측 세션 트리 (project/session 2단계) |
| `src/components/Timeline.svelte` | 우측 이벤트 타임라인 + 세션 정보 |
| `src/components/Terminal.svelte` | 중앙 xterm.js PTY 터미널 |
| `src/components/CommandPalette.svelte` | Cmd+K 커맨드 팔레트 |

## 명령어

```bash
npm run dev    # 개발 서버 (Vite)
npm run build  # 프로덕션 빌드
```

## 의존성

- telepty daemon이 localhost:3848에서 실행 중이어야 함
- Phase 2: Tauri 래핑 (Rust 필요)

## Tech Stack

Svelte + Vite + xterm.js + TailwindCSS. Tauri는 Phase 2.
