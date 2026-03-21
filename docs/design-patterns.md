# aterm Design Patterns

aigentry 에코시스템 공식 디자인 시스템. 모든 UI 컴포넌트는 이 패턴을 따릅니다.

## Color Palette (aigentry 공식)

### Dark Mode (기본)
| 역할 | 색상 | 비고 |
|------|------|------|
| Background Base | `#1a1210` ~ `#201815` | 따뜻한 다크 브라운 |
| Sidebar | `#151010` ~ `#1a1210` | 더 진한 브라운 |
| Elevated | `#252018` ~ `#2a2420` | 카드, 호버 |
| Border | `#302820` ~ `#352d25` | 미세한 구분선 |
| Text Primary | `#e8e4e0` | 따뜻한 화이트 |
| Text Secondary | `#b0a898` | 본문 |
| Text Muted | `#7a7068` | 라벨, 힌트 |
| Text Disabled | `#504840` | 비활성 |
| Accent | `#d97706` / `#f59e0b` | 오렌지/앰버 |
| Accent Hover | `#f59e0b` | 밝은 앰버 |
| Success | `#5cb97a` | 활성 상태 |
| Danger | `#d96c6c` | 에러 |
| Warning | `#d4a853` | 경고 |

### Light Mode
| 역할 | 색상 |
|------|------|
| Background Base | `#faf6f0` |
| Sidebar | `#f0ebe3` |
| Elevated | `#ffffff` |
| Border | `#d4cfc7` |
| Text Primary | `#1a1a1a` |
| Text Muted | `#888888` |
| Accent | `#d97706` |

## Icon System

### Library: Lucide
- 공식 라이브러리: [lucide.dev](https://lucide.dev)
- 크기: **24px**
- Stroke width: **1.5px**
- Color: `currentColor` (상태에 따라 변경)
- React 의존 없음 — SVG 직접 사용

### Session Icon Mapping

| 세션 | Lucide 아이콘 | 용도 |
|------|--------------|------|
| orchestrator | `Target` | 오케스트레이션/지휘 |
| brain | `Brain` | 기억/지식관리 |
| dustcraw | `Bug` | 크롤링/스크래핑 |
| amplify | `Megaphone` | 증폭/배포 |
| telepty | `Zap` | 통신/세션관리 |
| deliberation | `MessageSquare` | 토론/합의 |
| registry | `FlaskConical` | 등록/탐색 |
| ssot | `ClipboardCheck` | 스펙/단일 소스 |
| devkit | `Wrench` | 개발도구 |
| aterm | `Terminal` | 터미널/메신저 |
| design | `Palette` | 디자인/UI |

### Dynamic Icon Assignment
새 세션 생성 시:
1. 프로젝트명에서 키워드 매칭 (위 테이블 참조)
2. CLAUDE.md 파일 맥락 분석
3. 매칭 실패 시 기본 아이콘: `Circle`

### State Styling

| 상태 | 아이콘 색상 | 애니메이션 | 추가 표시 |
|------|-----------|----------|----------|
| idle | `#666666` (회색) | 없음 | opacity 0.5 |
| running | `#5cb97a` (밝은 초록) | 미세 펄스 (2s, scale 1→1.05) | — |
| error | `#d96c6c` (빨강) | 깜빡임 (1s) | 빨간 dot |
| done | `#5cb97a` (초록) | 없음 | opacity 0.7 |

## Layout Patterns

### Sidebar
- 섹션 구분: **대문자 레이블** (SESSIONS / WORKSPACES)
- 레이블 스타일: 11px, uppercase, letter-spacing 0.08em, muted color
- 선택 항목: **좌측 오렌지 바** (2-3px, accent color)
- 호버: 미세한 배경 변화 (150ms ease)
- 구분선 없음 — 여백으로 구분

### Header
- 높이: 48px
- 배경: sidebar와 동일 (seamless)
- 로고: accent 색상 dot + "aterm" 텍스트
- 연결 상태: 초록 dot + "Connected"

### Typography
- UI 텍스트: 시스템 산세리프 (-apple-system, system-ui)
- 코드/세션명: 모노스페이스 (SF Mono, JetBrains Mono)
- 제목: weight 600
- 본문: weight 400
- 라벨: 11px uppercase, wide letter-spacing

### Border Radius
- Small (buttons, inputs): 6-8px
- Large (cards, panels): 12px
- Pill (badges): 20px

### Animation Rules
- 최소한의 애니메이션만 사용
- 트랜지션: 150ms ease
- 상태 펄스: 2s ease-in-out infinite
- 페이드인: 200ms ease

## Brand

### aigentry Mascot (Pixel Art SVG)
- **형태:** 사각형 픽셀 아트 (SVG inline, 동그라미 없음)
- **구성:** 청록 사각 눈 2개 + 금색 2x4 사각형 그리드 코어
- **눈 색상:** 다크 #06b6d4 / 라이트 #0891b2
- **코어 색상:** 다크 #d4a574 / 라이트 #996515
- **SVG viewBox:** `0 0 24 28` — 눈 (4x4 rect x2) + 코어 (4x4 rect x8)
- **animate:** 코어 pulse (opacity 1↔0.6, 2s)
- **원본 참고:** `~/projects/aigentry-registry/frontend/src/components/mascot/AigentryMascot.tsx`
- **프로필 참고:** `~/Downloads/aigentry-profile-v3.png`

**배치 위치:**
| 위치 | 크기 | animate |
|------|------|---------|
| 헤더 로고 옆 | 12px (sm) | false |
| 빈 화면 중앙 | 32px (lg) | true (pulse) |
| 사이드바 하단 | 12px (sm) | true (pulse) |

**Favicon:** 🔶 (`getMascotEmoji()` → `'🔶'`)

## UI/UX Principles Checklist

모든 aterm UI 컴포넌트는 아래 원칙을 준수합니다.

| # | 원칙 | 상태 | 적용 방법 |
|---|------|------|----------|
| 1 | **Signifiers** | ✅ | hover bg 변화, active scale(0.995), selected 좌측 오렌지 바, cursor:pointer |
| 2 | **Visual Hierarchy** | ✅ | 마스코트 56px > 타이틀 18px/600 > 본문 13px/400 > 라벨 11px/uppercase |
| 3 | **Grouping/Proximity** | ✅ | 섹션 간 20px spacer, 관련 요소 gap 8px, 비관련 시각적 분리 |
| 4 | **Color Theory** | ✅ | 시맨틱: success #5cb97a, danger #d96c6c, warning #d4a853, accent #d97706 |
| 5 | **Typography** | ✅ | 2폰트 (sans=UI, mono=코드), 5단계 텍스트 위계 (primary→disabled) |
| 6 | **Spacing** | ✅ | padding 일관 (6px rows, 16px sections, 20px section gaps) |
| 7 | **Consistency** | ✅ | 모든 session/workspace row 동일 패턴, 모든 label 동일 스타일 |
| 8 | **Affordance** | ✅ | 버튼 cursor:pointer, + 버튼 명확, 토글 아이콘 직관적 (☀/🌙) |
| 9 | **Feedback** | ✅ | hover→bg변화, active→scale축소, focus-visible→오렌지 outline, pulse→상태표시 |

### 필수 인터랙션 패턴

| 요소 | hover | active/pressed | selected | focus-visible |
|------|-------|---------------|----------|---------------|
| Session row | bg → sidebar-hover | scale(0.995) + bg → selected | 좌측 2px 오렌지 바 | 2px accent outline |
| Workspace row | bg → sidebar-hover | scale(0.995) + bg → selected | — | 2px accent outline |
| + 버튼 | bg → button-hover | scale(0.9) + accent color | — | 2px accent outline |
| 테마 토글 | bg → border-default | scale(0.92) | — | 2px accent outline |

### Brand Rule: 동그라미 금지, 사각형만

**절대 규칙: aigentry 에코시스템에서 동그라미(circle) 패턴 사용 금지.**

| 요소 | ❌ 금지 | ✅ 사용 |
|------|--------|--------|
| 마스코트 | 동그란 dot | 사각형 픽셀 |
| 상태 표시 | ● (circle) | ■ (square, border-radius: 1px) |
| 아이콘 형태 | 원형 기반 | 사각형 기반 |
| 레전드 dot | 둥근 점 | 사각 점 |
| 버튼 | — | border-radius OK (도형 자체는 사각형) |

**이유:** aigentry 브랜드 아이덴티티 = 픽셀 아트 = 사각형

## Messenger UX (채택됨)

### Core Layout: 듀얼 뷰
```
┌────────────┬──────────────────────┬──────────┐
│ 사이드바    │ Chat View (70%)      │ Terminal │
│ 친구 목록   │ 채팅 버블            │ (30%)    │
│            │                      │ 토글     │
└────────────┴──────────────────────┴──────────┘
```
- 기본: 채팅 뷰만 (터미널 숨김)
- 토글: 우측 터미널 패널 슬라이드
- 퍼블릭 사용자: 터미널 불필요 / 파워유저: 직접 입력

### 대화 모드
| 모드 | 트리거 | UI |
|------|--------|-----|
| 1:1 채팅 | 세션 클릭 | 채팅 버블 + 터미널 패널(옵션) |
| 그룹 채팅 | Cmd+K → `group orc brain design` | 멀티 에이전트 버블 (발산→수렴) |

### 사이드바
- 친구 목록 패턴: 아이콘 + 이름 + 마지막 메시지 미리보기
- 마지막 메시지: brain 구조화 이벤트에서 가져옴
- 상태: running(초록), idle(회색), error(빨강)

### 그룹채팅 UI
- 각 에이전트: 다른 색상/아이콘 버블
- 발산 단계: 병렬 메시지 타임스탬프 순
- 수렴 단계: synthesis 특별 카드
- PTY 출력: 터미널 패널에서만 표시 (버블 변환 없음)

### Ecosystem Tone
- 따뜻한 다크 브라운 (차가운 네이비/차콜 아님)
- 오렌지/앰버 액센트 (파란색 아님)
- 미니멀, 여백 중심
- Lucide 라인 아이콘
- registry (aigentry.dev)와 동일한 톤
