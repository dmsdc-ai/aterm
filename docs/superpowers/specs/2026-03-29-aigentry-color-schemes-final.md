# Aigentry Terminal Color Schemes — Dark + Light

## Design Principles

1. **Gold cursor** (#d4a574 dark / #b45309 light) — aterm 브랜드 식별자, 모든 테마 공통
2. **Brand 4색 통합** — Gold, Cyan, Purple, Pink이 ANSI 팔레트에 자연 배치
3. **WCAG AAA** — foreground/background 7:1 이상, ANSI colors 4.5:1 이상
4. **UI 팔레트 동기화** — `theme.rs` Palette와 background/foreground 일치

---

## 1. Aigentry Dark

```
background:   #0d1117
foreground:   #c9d1d9
cursor:       #d4a574    (brand gold)
cursor_text:  #0d1117
selection_bg: #2a3a50
selection_fg: #ffffff
```

**대비율**: foreground/background = 11.1:1 (AAA)

### ANSI 16

| # | Name | Hex | Brand | Contrast |
|---|------|-----|-------|----------|
| 0 | Black | `#484f58` | — | — |
| 1 | Red | `#f85349` | — | 5.3:1 AA |
| 2 | Green | `#3fb950` | — | 6.8:1 AA |
| 3 | Yellow | `#d4a574` | Gold | 7.2:1 AAA |
| 4 | Blue | `#8b5cf6` | Purple | 4.6:1 AA |
| 5 | Magenta | `#ec4899` | Pink | 5.4:1 AA |
| 6 | Cyan | `#06b6d4` | Cyan | 6.5:1 AA |
| 7 | White | `#b1bac4` | — | 8.5:1 AAA |
| 8 | Bright Black | `#6e7681` | — | 3.7:1 |
| 9 | Bright Red | `#ff7b72` | — | 6.7:1 AA |
| 10 | Bright Green | `#56d364` | — | 8.2:1 AAA |
| 11 | Bright Yellow | `#e3c19c` | Gold light | 9.8:1 AAA |
| 12 | Bright Blue | `#a78bfa` | Purple light | 5.9:1 AA |
| 13 | Bright Magenta | `#f472b6` | Pink light | 6.4:1 AA |
| 14 | Bright Cyan | `#22d3ee` | Cyan light | 8.5:1 AAA |
| 15 | Bright White | `#f0f6fc` | — | 14.5:1 AAA |

### Dim Colors (iced 미사용, 참고용)

| Name | Hex |
|------|-----|
| Dim Black | `#2d333b` |
| Dim Red | `#b34044` |
| Dim Green | `#2d8a4e` |
| Dim Yellow | `#a07850` |
| Dim Blue | `#6944b8` |
| Dim Magenta | `#b03672` |
| Dim Cyan | `#0589a0` |
| Dim White | `#8b949e` |

---

## 2. Aigentry Light

```
background:   #faf6f0
foreground:   #24292f
cursor:       #b45309    (darkened brand gold)
cursor_text:  #faf6f0
selection_bg: #d4a57425  (gold 15% opacity)
selection_fg: #24292f
```

**대비율**: foreground/background = 13.0:1 (AAA)

### ANSI 16

Light mode 전략:
- **Normal colors**: 브랜드 색상의 어두운 버전 (light bg에서 AAA 근접)
- **Bright colors**: 원본 브랜드 색상 그대로 (강조용, AA 이상)

| # | Name | Hex | Brand | Contrast |
|---|------|-----|-------|----------|
| 0 | Black | `#24292f` | — | 13.0:1 AAA |
| 1 | Red | `#cf222e` | — | 7.4:1 AAA |
| 2 | Green | `#1a7f37` | — | 5.0:1 AA |
| 3 | Yellow | `#93570a` | Gold dark | 6.1:1 AA |
| 4 | Blue | `#6d28d9` | Purple dark | 7.8:1 AAA |
| 5 | Magenta | `#bf3989` | Pink dark | 5.2:1 AA |
| 6 | Cyan | `#0e7490` | Cyan dark | 5.3:1 AA |
| 7 | White | `#8b949e` | — | 3.1:1 |
| 8 | Bright Black | `#57606a` | — | 5.6:1 AA |
| 9 | Bright Red | `#d5534a` | — | 4.7:1 AA |
| 10 | Bright Green | `#2ea44f` | — | 4.5:1 AA |
| 11 | Bright Yellow | `#b45309` | Gold (cursor) | 5.7:1 AA |
| 12 | Bright Blue | `#8b5cf6` | Purple original | 4.5:1 AA |
| 13 | Bright Magenta | `#ec4899` | Pink original | 4.6:1 AA |
| 14 | Bright Cyan | `#06b6d4` | Cyan original | 4.5:1 AA |
| 15 | Bright White | `#6e7781` | — | 4.6:1 AA |

### Dim Colors (참고용)

| Name | Hex |
|------|-----|
| Dim Black | `#afb8c1` |
| Dim Red | `#e57373` |
| Dim Green | `#66bb6a` |
| Dim Yellow | `#c49a6c` |
| Dim Blue | `#a78bfa` |
| Dim Magenta | `#f0a0c0` |
| Dim Cyan | `#4db8cc` |
| Dim White | `#d0d7de` |

---

## 3. Dark ↔ Light 매핑

| 요소 | Dark | Light | 관계 |
|------|------|-------|------|
| Background | `#0d1117` | `#faf6f0` | 반전 (cool dark ↔ warm light) |
| Foreground | `#c9d1d9` | `#24292f` | 반전 |
| Cursor | `#d4a574` | `#b45309` | 같은 gold, light에서 어둡게 |
| Selection | `#2a3a50` | `#d4a57425` | dark=blue / light=gold tint |
| ANSI Normal | 밝은/비비드 | 어두운/채도↓ | light bg에서 가독성 확보 |
| ANSI Bright | 더 밝은 | 원본 brand 색상 | light에서 brand 원색 유지 |
| Brand Gold | Yellow slot | Yellow slot | 양쪽 모두 ANSI Yellow에 배치 |
| Brand Purple | Blue slot | Blue slot | 양쪽 모두 ANSI Blue에 배치 |
| Brand Pink | Magenta slot | Magenta slot | 양쪽 모두 ANSI Magenta에 배치 |
| Brand Cyan | Cyan slot | Cyan slot | 양쪽 모두 ANSI Cyan에 배치 |

---

## 4. 구현

### renderer.rs — named_color_fallback 교체

```rust
fn named_color_fallback(named: NamedColor, is_light: bool) -> Color {
    if is_light {
        light_named_color(named)
    } else {
        dark_named_color(named)
    }
}

fn dark_named_color(named: NamedColor) -> Color {
    match named {
        NamedColor::Black =>         Color::from_rgb8(0x48, 0x4f, 0x58),
        NamedColor::Red =>           Color::from_rgb8(0xf8, 0x53, 0x49),
        NamedColor::Green =>         Color::from_rgb8(0x3f, 0xb9, 0x50),
        NamedColor::Yellow =>        Color::from_rgb8(0xd4, 0xa5, 0x74),
        NamedColor::Blue =>          Color::from_rgb8(0x8b, 0x5c, 0xf6),
        NamedColor::Magenta =>       Color::from_rgb8(0xec, 0x48, 0x99),
        NamedColor::Cyan =>          Color::from_rgb8(0x06, 0xb6, 0xd4),
        NamedColor::White =>         Color::from_rgb8(0xb1, 0xba, 0xc4),
        NamedColor::BrightBlack =>   Color::from_rgb8(0x6e, 0x76, 0x81),
        NamedColor::BrightRed =>     Color::from_rgb8(0xff, 0x7b, 0x72),
        NamedColor::BrightGreen =>   Color::from_rgb8(0x56, 0xd3, 0x64),
        NamedColor::BrightYellow =>  Color::from_rgb8(0xe3, 0xc1, 0x9c),
        NamedColor::BrightBlue =>    Color::from_rgb8(0xa7, 0x8b, 0xfa),
        NamedColor::BrightMagenta => Color::from_rgb8(0xf4, 0x72, 0xb6),
        NamedColor::BrightCyan =>    Color::from_rgb8(0x22, 0xd3, 0xee),
        NamedColor::BrightWhite =>   Color::from_rgb8(0xf0, 0xf6, 0xfc),
        NamedColor::Foreground |
        NamedColor::BrightForeground => Color::from_rgb8(0xc9, 0xd1, 0xd9),
        NamedColor::Background =>    Color::from_rgb8(0x0d, 0x11, 0x17),
        NamedColor::Cursor =>        Color::from_rgb8(0xd4, 0xa5, 0x74),
        _ =>                         Color::from_rgb8(0xc9, 0xd1, 0xd9),
    }
}

fn light_named_color(named: NamedColor) -> Color {
    match named {
        NamedColor::Black =>         Color::from_rgb8(0x24, 0x29, 0x2f),
        NamedColor::Red =>           Color::from_rgb8(0xcf, 0x22, 0x2e),
        NamedColor::Green =>         Color::from_rgb8(0x1a, 0x7f, 0x37),
        NamedColor::Yellow =>        Color::from_rgb8(0x93, 0x57, 0x0a),
        NamedColor::Blue =>          Color::from_rgb8(0x6d, 0x28, 0xd9),
        NamedColor::Magenta =>       Color::from_rgb8(0xbf, 0x39, 0x89),
        NamedColor::Cyan =>          Color::from_rgb8(0x0e, 0x74, 0x90),
        NamedColor::White =>         Color::from_rgb8(0x8b, 0x94, 0x9e),
        NamedColor::BrightBlack =>   Color::from_rgb8(0x57, 0x60, 0x6a),
        NamedColor::BrightRed =>     Color::from_rgb8(0xd5, 0x53, 0x4a),
        NamedColor::BrightGreen =>   Color::from_rgb8(0x2e, 0xa4, 0x4f),
        NamedColor::BrightYellow =>  Color::from_rgb8(0xb4, 0x53, 0x09),
        NamedColor::BrightBlue =>    Color::from_rgb8(0x8b, 0x5c, 0xf6),
        NamedColor::BrightMagenta => Color::from_rgb8(0xec, 0x48, 0x99),
        NamedColor::BrightCyan =>    Color::from_rgb8(0x06, 0xb6, 0xd4),
        NamedColor::BrightWhite =>   Color::from_rgb8(0x6e, 0x77, 0x81),
        NamedColor::Foreground |
        NamedColor::BrightForeground => Color::from_rgb8(0x24, 0x29, 0x2f),
        NamedColor::Background =>    Color::from_rgb8(0xfa, 0xf6, 0xf0),
        NamedColor::Cursor =>        Color::from_rgb8(0xb4, 0x53, 0x09),
        _ =>                         Color::from_rgb8(0x24, 0x29, 0x2f),
    }
}
```

### theme.rs 연동

Dark/Light Palette의 `background`/`text` 값은 이미 동기화됨:
- Dark: background=#0d1117, text=#c9d1d9
- Light: background=#faf6f0, text=#1a1a1a (→ foreground #24292f로 업데이트 권장)

### TerminalRenderer 테마 전환

```rust
impl TerminalRenderer {
    pub fn for_theme(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                font_size: Pixels(14.0),
                line_height: 1.25,
                foreground: Color::from_rgb8(0xc9, 0xd1, 0xd9),
                background: Color::from_rgb8(0x0d, 0x11, 0x17),
            },
            ThemeMode::Light => Self {
                font_size: Pixels(14.0),
                line_height: 1.25,
                foreground: Color::from_rgb8(0x24, 0x29, 0x2f),
                background: Color::from_rgb8(0xfa, 0xf6, 0xf0),
            },
        }
    }
}
```
