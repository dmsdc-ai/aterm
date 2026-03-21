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

### aigentry Mascot
- ASCII art 네트워크/노드 그래프
- 색상: 오렌지/앰버 (#fbbf24, #f59e0b, #d97706)
- 사용 위치: 빈 화면 중앙, 로딩, 앱 아이콘
- SVG 파일: `~/projects/aigentry-design/mascot.svg`

### Ecosystem Tone
- 따뜻한 다크 브라운 (차가운 네이비/차콜 아님)
- 오렌지/앰버 액센트 (파란색 아님)
- 미니멀, 여백 중심
- Lucide 라인 아이콘
- registry (aigentry.dev)와 동일한 톤
