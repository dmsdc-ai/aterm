# aterm Context (렌더링 교훈 + 세션 통신)

aterm 전용 문서. 위임 시 해당 프로젝트 교훈을 **반드시** inject에 포함 (Rule 7-1).

## 검증된 렌더링 교훈 (에코시스템 영속)

### 반드시 지킬 것

| 규칙 | 이유 | 검증일 |
|------|------|--------|
| wgpu `Bgra8UnormSrgb` → `srgb_to_linear()` 변환 필수 | raw sRGB → double-gamma → 어두운 색이 밝게 렌더링 | 2026-04-03 |
| Block Drawing U+2580-U+2595 → glyphon 우회, rect_renderer pixel-perfect | font glyph → line-height 불일치 → 셀 틈새 | 2026-04-03 |
| PTY reader NFC 정규화 필수 (`unicode-normalization`) | macOS NFD + cosmic-text 미지원 → 한글 자모 분리 | 2026-04-03 |
| 렌더링 이슈 → 증거 기반 디버깅만 (PTY 로그 캡처 → diff) | 추측 수정 3회 실패. `script`로 raw escape seq 비교 필수 | 2026-04-03 |
| ANSI INVERSE/BCE 전체 라인 bg는 정상 동작 — 억제 금지 | Gemini CLI 등이 사용하는 정당한 ANSI 렌더링 | 2026-04-03 |

### 실패한 접근 (반복 금지)

| 접근 | 실패 이유 |
|------|----------|
| Rust grid_padding + scissor로 하단 잘림 수정 | Swift MTKView frame 문제. 잘못된 레이어 |
| Swift isFlipped + masksToBounds로 하단 잘림 | 증거 없이 수정. frame 치수 불일치 미파악 |
| row_last_content_col trailing blank bg 억제 | 정상 ANSI 동작을 잘못 억제 |
| Dark 팔레트만 변경으로 색상 차이 해결 | sRGB↔linear 버그가 근본 원인 |
| 추측 기반 렌더링 수정 (증거 없이) | 3회 연속 실패. 반드시 로그 캡처 후 수정 |

## aterm 세션 통신

### 내부 세션 (같은 aterm 앱)

| 자연어 | 명령 |
|--------|------|
| 세션 목록 보여줘 | `aterm list` |
| ghostty에 빌드 실행해줘 | `aterm inject ghostty 'make build'` |
| 세션 상태 확인 | `aterm status <workspace>` |
| 태스크 목록 | `aterm tasks` |
| 태스크 추가 | `aterm tasks add '<설명>'` |
| 태스크 완료 | `aterm tasks done <id>` |
| 레슨 보여줘 | `aterm lessons` |
| 레슨 추가 | `aterm lessons add '<내용>'` |
| 사용법 | `aterm help` |

### 외부 세션 (다른 터미널/머신)

| 자연어 | 명령 |
|--------|------|
| 외부 세션 목록 | `telepty list` |
| 외부 세션에 메시지 | `telepty inject <session-id> '<message>'` |

### 자동 위임 (dispatch)

복잡한 태스크를 자동 분해 → 서브세션 생성 → 실행 → 수집 → 정리:

| 자연어 | 명령 |
|--------|------|
| 태스크 자동 분배 | `aterm dispatch <task-id>` |
| 자유 텍스트로 분배 | `aterm dispatch --plan '설명'` |

**dispatch 흐름**: 태스크 분해 → CLI 자동 선택 (implement→codex, architect→claude, research→gemini) → 서브세션 생성 → inject → 완료 대기 → 결과 수집 → 정리

복잡한 태스크(3+ 파일, 멀티 컴포넌트)는 `aterm dispatch`. 단일 파일 태스크는 subagent 추천.

### 감지 규칙

- `$ATERM_IPC_SOCKET` 존재 → aterm 내부 → `aterm` 명령
- `$ATERM_IPC_SOCKET` 없음 → 외부 → `telepty` 명령
