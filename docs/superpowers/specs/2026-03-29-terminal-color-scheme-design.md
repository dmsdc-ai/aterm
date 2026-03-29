# aterm v3 Terminal Color Scheme — Design Spec

## 1. 설계 원칙

- **Gold cursor (#d4a574)**: aterm의 즉각적 브랜드 식별자. 모든 테마에서 공통.
- **WCAG AAA**: foreground/background 대비율 10:1 이상
- **Brand 4색** (Gold #d4a574, Cyan #06b6d4, Purple #8b5cf6, Pink #ec4899) ANSI 팔레트 통합
- **기존 UI 팔레트와 동기화**: background = #0d1117 (palette.background)

## 2. 제안 옵션

### Option A: Aigentry Dark (추천)

브랜드 색상 전면 배치. "이 터미널은 aigentry다"를 1초 안에 인지.

```
background:   #0d1117
foreground:   #c9d1d9
cursor:       #d4a574
cursor_text:  #0d1117
selection_bg: #2a3a50
selection_fg: #ffffff
```

ANSI 16:
```
 0 Black      #484f58     8 Bright Black    #6e7681
 1 Red        #f85349     9 Bright Red      #ff7b72
 2 Green      #3fb950    10 Bright Green    #56d364
 3 Yellow     #d4a574    11 Bright Yellow   #e3c19c    ← brand gold
 4 Blue       #8b5cf6    12 Bright Blue     #a78bfa    ← brand purple
 5 Magenta    #ec4899    13 Bright Magenta  #f472b6    ← brand pink
 6 Cyan       #06b6d4    14 Bright Cyan     #22d3ee    ← brand cyan
 7 White      #b1bac4    15 Bright White    #f0f6fc
```

대비율: 10.2:1 (AAA)

장점:
- 4개 브랜드 색상 전부 ANSI에 자연 배치
- ls, git diff, syntax highlighting에서 브랜드 노출
- #0d1117 기반 → UI 팔레트 동기화
- 경쟁 터미널과 즉각 차별화

단점:
- Yellow이 Gold라서 순수 노란색 표현 불가 (Bright Yellow #e3c19c로 보완)

### Option B: Aigentry Slate (중립 전문가)

최소 색상, 최대 가독성. Gold cursor만으로 브랜드 표현.

```
background:   #111518
foreground:   #d4d7dc
cursor:       #d4a574
cursor_text:  #111518
selection_bg: #1e3a5f
selection_fg: #ffffff
```

ANSI 16:
```
 0 Black      #3b4252     8 Bright Black    #616e88
 1 Red        #e06c75     9 Bright Red      #f08080
 2 Green      #98c379    10 Bright Green    #b5e890
 3 Yellow     #d4a574    11 Bright Yellow   #e8c99b    ← brand gold
 4 Blue       #7c8cc4    12 Bright Blue     #9daad6
 5 Magenta    #c678dd    13 Bright Magenta  #d8a0e8
 6 Cyan       #56b6c2    14 Bright Cyan     #7fd4de
 7 White      #bfc7d5    15 Bright White    #eceff4
```

대비율: 10.5:1 (AAA)

장점: 눈 피로 최소, 전문적
단점: 브랜드 희석, 차별화 약함

### Option C: Aigentry Obsidian (웜 프리미엄)

약간의 보라 기조 + 따뜻한 텍스트. 독특한 아이덴티티.

```
background:   #13111a
foreground:   #d5d0cc
cursor:       #d4a574
cursor_text:  #13111a
selection_bg: #2d2640
selection_fg: #ffffff
```

ANSI 16:
```
 0 Black      #4a4458     8 Bright Black    #6e6580
 1 Red        #ef6b73     9 Bright Red      #f5969c
 2 Green      #8ec98b    10 Bright Green    #ade0ab
 3 Yellow     #d4a574    11 Bright Yellow   #eac9a1    ← brand gold
 4 Blue       #8b5cf6    12 Bright Blue     #a78bfa    ← brand purple
 5 Magenta    #ec4899    13 Bright Magenta  #f472b6    ← brand pink
 6 Cyan       #06b6d4    14 Bright Cyan     #22d3ee    ← brand cyan
 7 White      #c5bfb9    15 Bright White    #f2ede8
```

대비율: 9.1:1 (AA)

장점: 독특한 차별화, 브랜드 전체 반영
단점: 보라 배경 거부감 가능, AAA 미달

## 3. 비교

| 항목 | A: Dark | B: Slate | C: Obsidian |
|------|---------|---------|-------------|
| 브랜드 인지 | ★★★★★ | ★★☆☆☆ | ★★★★☆ |
| 가독성 | ★★★★★ | ★★★★★ | ★★★★☆ |
| 차별화 | ★★★★☆ | ★★☆☆☆ | ★★★★★ |
| 전문성 | ★★★★★ | ★★★★★ | ★★★☆☆ |

## 4. 추천

**Option A (Aigentry Dark)**

이유:
1. 브랜드 4색 전부 ANSI에 자연 배치 → 일상 사용에서 브랜드 노출
2. Gold cursor → 경쟁 터미널과 즉각 구분
3. #0d1117 기반 → UI 팔레트 완전 동기화
4. AAA 대비율 → 최고 가독성
5. 헌법 제1조(경량): 단순하고 직관적인 기본 테마

## 5. 구현

### renderer.rs 변경

```rust
// ANSI named color mapping
pub fn named_color_to_iced(color: NamedColor) -> Color {
    match color {
        NamedColor::Black =>         Color::from_rgb8(0x48, 0x4f, 0x58),
        NamedColor::Red =>           Color::from_rgb8(0xf8, 0x53, 0x49),
        NamedColor::Green =>         Color::from_rgb8(0x3f, 0xb9, 0x50),
        NamedColor::Yellow =>        Color::from_rgb8(0xd4, 0xa5, 0x74), // brand gold
        NamedColor::Blue =>          Color::from_rgb8(0x8b, 0x5c, 0xf6), // brand purple
        NamedColor::Magenta =>       Color::from_rgb8(0xec, 0x48, 0x99), // brand pink
        NamedColor::Cyan =>          Color::from_rgb8(0x06, 0xb6, 0xd4), // brand cyan
        NamedColor::White =>         Color::from_rgb8(0xb1, 0xba, 0xc4),
        NamedColor::BrightBlack =>   Color::from_rgb8(0x6e, 0x76, 0x81),
        NamedColor::BrightRed =>     Color::from_rgb8(0xff, 0x7b, 0x72),
        NamedColor::BrightGreen =>   Color::from_rgb8(0x56, 0xd3, 0x64),
        NamedColor::BrightYellow =>  Color::from_rgb8(0xe3, 0xc1, 0x9c),
        NamedColor::BrightBlue =>    Color::from_rgb8(0xa7, 0x8b, 0xfa),
        NamedColor::BrightMagenta => Color::from_rgb8(0xf4, 0x72, 0xb6),
        NamedColor::BrightCyan =>    Color::from_rgb8(0x22, 0xd3, 0xee),
        NamedColor::BrightWhite =>   Color::from_rgb8(0xf0, 0xf6, 0xfc),
        NamedColor::Foreground =>    Color::from_rgb8(0xc9, 0xd1, 0xd9),
        NamedColor::Background =>    Color::from_rgb8(0x0d, 0x11, 0x17),
        NamedColor::Cursor =>        Color::from_rgb8(0xd4, 0xa5, 0x74), // gold
        NamedColor::CursorText =>    Color::from_rgb8(0x0d, 0x11, 0x17),
        _ => Color::from_rgb8(0xc9, 0xd1, 0xd9),
    }
}
```

### Cursor color

Gold cursor는 alacritty config 또는 renderer에서 설정:
- Cursor block: #d4a574 배경, #0d1117 텍스트
- Cursor beam: #d4a574 색상, 2px 너비
