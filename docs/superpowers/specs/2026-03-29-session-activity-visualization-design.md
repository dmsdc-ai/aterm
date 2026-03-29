# Session Activity State Visualization — Design Spec

## 1. 개요

aterm v3 사이드바에서 세션의 AI CLI 활동 상태를 실시간으로 시각화.
현재 `SessionStatus`는 프로세스 생사(Online/Dead)만 구분 — AI가 응답 생성 중인지, 대기 중인지, 에러인지 구분 불가.

### 핵심 원칙
- **즉시 인지**: 상태를 1초 내에 파악 가능해야 함
- **경량**: subscription 1개, 250ms tick, GPU 부하 무시 가능
- **공간 효율**: 기존 status dot 영역(8px) + 선택적 경과시간(우측)만 사용

## 2. 6가지 활동 상태

| 상태 | 의미 | 감지 방법 |
|------|------|----------|
| **generating** | AI CLI가 응답 생성 중 | PTY 출력 지속 + 프롬프트 미감지 |
| **idle** | 프롬프트 대기 중 | 프롬프트 감지 + 출력 정지 |
| **busy** | 사용자 입력 처리 중 (짧은 작업) | 사용자 입력 후 출력 시작, 프롬프트 미감지 |
| **error** | 에러 출력 감지 | stderr 또는 에러 패턴 감지 |
| **dead** | 프로세스 종료 | 프로세스 exit |
| **stale** | 장시간 무반응 | 출력 없음 > 5분 + 프롬프트 미감지 |

### 상태 전이

```
             사용자 입력
    idle ──────────────→ busy
     ↑                     │
     │ 프롬프트 감지        │ AI 출력 시작
     │                     ▼
     ├──────────────── generating
     │                     │
     │                     ├─ 5분 무반응 → stale
     │                     ├─ 에러 감지 → error
     │                     └─ 프로세스 종료 → dead
     │
     ├──── stale (출력 재개 시 → generating)
     ├──── error (프롬프트 복귀 시 → idle)
     └──── dead (재시작 시 → idle)
```

## 3. 시각화 명세

### 3.1 Dot 문자 + 색상 + 애니메이션

| 상태 | Dot 문자 | 색상 | 애니메이션 | 경과시간 |
|------|---------|------|-----------|---------|
| **generating** | `◐◓◑◒` (4프레임) | `accent` (#d97706) | 250ms 사이클 | `{N}s` 표시 |
| **idle** | `●` | `success` (#3fb950) | 없음 (정적) | 없음 |
| **busy** | `●` ↔ `○` | `success` (#3fb950) | 500ms 토글 | `{N}s` 표시 |
| **error** | `✦` | `danger` (#f85349) | 없음 | 없음 |
| **dead** | `✕` | `text_disabled` (#484f58) | 없음, 행 전체 dimmed | 없음 |
| **stale** | `◌` (빈 원) | `text_muted` (#6e7681) | 없음 | 없음 |

### 3.2 사이드바 행 레이아웃

```
│ SESSIONS                        + │
│ ▍◐ orchestrator          cl  12s  │  ← generating
│   ● brain                co       │  ← idle
│ ▍◐ design                cl   3s  │  ← generating
│   ○ forum                ge       │  ← busy (○ 프레임)
│   ● dustcraw                      │  ← idle
│   ◌ linux                         │  ← stale
│   ✕ old-test                      │  ← dead (dimmed)
│   ✦ broken               cl       │  ← error
```

행 구조:
```
[strip 3px][dot 8px][gap 8px][name Fill][badge 18px][gap 4px][elapsed 24px]
```

- 경과시간: 9px mono, `text_muted`, 우측 정렬
- `{N}s` 형식 (초), 60초 초과 시 `{N}m` (분)
- idle/dead/stale/error는 경과시간 미표시 (공간 절약)

### 3.3 CLI 뱃지

| CLI | 뱃지 | 비고 |
|-----|------|------|
| claude | `cl` | 9px mono, `text_muted` |
| codex | `co` | |
| gemini | `ge` | |
| 없음 | — | 뱃지 영역 비움 |

## 4. iced 구현

### 4.1 Subscription 타이머

```rust
fn subscription(&self) -> Subscription<Message> {
    // 기존 subscription에 추가
    time::every(Duration::from_millis(250)).map(Message::AnimTick)
}
```

`AnimTick`마다:
- `frame_counter += 1`
- generating dot = `['◐','◓','◑','◒'][frame_counter % 4]`
- busy dot = `if frame_counter % 2 == 0 { '●' } else { '○' }`
- 경과시간 갱신 (활성 상태만)

### 4.2 ActivityState 타입

```rust
// core/activity.rs (신규)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Generating { since: Instant },
    Idle,
    Busy { since: Instant },
    Error,
    Dead,
    Stale,
}

impl ActivityState {
    pub fn dot_char(&self, frame: u32) -> char {
        match self {
            Self::Generating { .. } => ['◐','◓','◑','◒'][(frame % 4) as usize],
            Self::Idle => '●',
            Self::Busy { .. } => if frame % 2 == 0 { '●' } else { '○' },
            Self::Error => '✦',
            Self::Dead => '✕',
            Self::Stale => '◌',
        }
    }

    pub fn dot_color(&self, palette: &Palette) -> Color {
        match self {
            Self::Generating { .. } => palette.accent,
            Self::Idle => palette.success,
            Self::Busy { .. } => palette.success,
            Self::Error => palette.danger,
            Self::Dead => palette.text_disabled,
            Self::Stale => palette.text_muted,
        }
    }

    pub fn elapsed_text(&self) -> Option<String> {
        match self {
            Self::Generating { since } | Self::Busy { since } => {
                let secs = since.elapsed().as_secs();
                if secs >= 60 {
                    Some(format!("{}m", secs / 60))
                } else {
                    Some(format!("{}s", secs))
                }
            }
            _ => None,
        }
    }

    pub fn is_dimmed(&self) -> bool {
        matches!(self, Self::Dead)
    }
}
```

### 4.3 활동 감지 로직

```rust
// core/activity.rs

pub fn compute_activity(
    status: &SessionStatus,    // Online/Dead
    pty_output_age: Duration,  // 마지막 PTY 출력 이후 경과
    has_prompt: bool,          // 프롬프트 감지 여부
    user_input_age: Duration,  // 마지막 사용자 입력 이후 경과
    has_error: bool,           // 에러 패턴 감지
) -> ActivityState {
    if matches!(status, SessionStatus::Dead) {
        return ActivityState::Dead;
    }
    if has_error {
        return ActivityState::Error;
    }
    if has_prompt && pty_output_age > Duration::from_secs(1) {
        return ActivityState::Idle;
    }
    if pty_output_age > Duration::from_secs(300) && !has_prompt {
        return ActivityState::Stale;
    }
    if pty_output_age < Duration::from_secs(2) && !has_prompt {
        return ActivityState::Generating { since: Instant::now() };
    }
    if user_input_age < Duration::from_secs(3) && pty_output_age < Duration::from_secs(2) {
        return ActivityState::Busy { since: Instant::now() };
    }
    ActivityState::Idle
}
```

**주의**: `since: Instant::now()`는 매 호출마다 리셋됨. 실제 구현에서는 이전 상태의 `since`를 유지해야 함 (상태 전이 시에만 갱신).

### 4.4 sidebar.rs 변경

`status_dot` 함수를 `activity_dot`으로 확장:

```rust
fn activity_dot<'a>(
    activity: ActivityState,
    frame: u32,
    palette: Palette,
) -> Element<'a, SidebarAction> {
    let ch = activity.dot_char(frame);
    let color = activity.dot_color(&palette);

    text(ch.to_string())
        .size(10)
        .style(move |_| iced::widget::text::Style {
            color: Some(color),
        })
        .into()
}
```

경과시간 표시:

```rust
fn elapsed_label<'a>(
    activity: &ActivityState,
    palette: Palette,
) -> Option<Element<'a, SidebarAction>> {
    activity.elapsed_text().map(|t| {
        text(t)
            .size(9)
            .style(move |_| iced::widget::text::Style {
                color: Some(palette.text_muted),
            })
            .into()
    })
}
```

### 4.5 SessionEntry 확장

```rust
pub struct SessionEntry {
    pub id: String,
    pub name: String,
    pub status: SessionStatus,
    pub active: bool,
    pub pending_injects: usize,
    pub cli_type: Option<CliType>,       // 추가
    pub activity: ActivityState,          // 추가
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliType {
    Claude,
    Codex,
    Gemini,
}

impl CliType {
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Claude => "cl",
            Self::Codex => "co",
            Self::Gemini => "ge",
        }
    }
}
```

## 5. 성능 분석

| 항목 | 부하 |
|------|------|
| Subscription | 250ms 간격 1개 (기존 1초 tick와 공존) |
| AnimTick 처리 | `frame_counter += 1` (O(1)) |
| 사이드바 렌더링 | 20개 세션 × text widget 1개 = 무시 가능 |
| compute_activity | 세션당 비교 5회 (O(1)) |
| 총 영향 | CPU 0.1% 미만, GPU 프레임 변화 없음 |

## 6. 수정 파일 목록

| 파일 | 변경 |
|------|------|
| `core/activity.rs` | **신규** — ActivityState, CliType, compute_activity |
| `core/mod.rs` | activity 모듈 추가 |
| `ui/sidebar.rs` | activity_dot, elapsed_label, CLI badge, SessionEntry 확장 |
| `main.rs` | AnimTick 메시지 추가, subscription 확장, frame_counter 상태 |

## 7. 구현 우선순위

| 순서 | 항목 | 의존성 |
|------|------|--------|
| 1 | ActivityState + CliType 타입 정의 | 없음 |
| 2 | compute_activity 감지 로직 | ActivityState |
| 3 | sidebar activity_dot + elapsed_label | ActivityState |
| 4 | CLI badge 표시 | CliType |
| 5 | AnimTick subscription + frame_counter | sidebar |
| 6 | main.rs 통합 | 전체 |
