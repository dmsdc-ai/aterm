use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::Dark
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub surface_overlay: Color,
    pub border: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

pub fn light() -> Palette {
    Palette {
        background: Color::from_rgb8(0xfa, 0xf6, 0xf0),
        surface: Color::from_rgb8(0xf0, 0xeb, 0xe3),
        surface_alt: Color::from_rgb8(0xe8, 0xe3, 0xdb),
        surface_overlay: Color::from_rgba8(0x00, 0x00, 0x00, 0.40),
        border: Color::from_rgb8(0xd4, 0xcf, 0xc7),
        text: Color::from_rgb8(0x1a, 0x1a, 0x1a),
        text_muted: Color::from_rgb8(0x88, 0x88, 0x88),
        accent: Color::from_rgb8(0xd9, 0x77, 0x06),
        accent_soft: Color::from_rgb8(0xe0, 0xdb, 0xd3),
        success: Color::from_rgb8(0x3d, 0xa8, 0x5e),
        warning: Color::from_rgb8(0xc4, 0x95, 0x2e),
        danger: Color::from_rgb8(0xd4, 0x55, 0x55),
    }
}

pub fn dark() -> Palette {
    Palette {
        background: Color::from_rgb8(0x1e, 0x18, 0x14),
        surface: Color::from_rgb8(0x15, 0x10, 0x10),
        surface_alt: Color::from_rgb8(0x25, 0x20, 0x18),
        surface_overlay: Color::from_rgba8(0x00, 0x00, 0x00, 0.72),
        border: Color::from_rgb8(0x30, 0x28, 0x20),
        text: Color::from_rgb8(0xe8, 0xe4, 0xe0),
        text_muted: Color::from_rgb8(0x6a, 0x60, 0x58),
        accent: Color::from_rgb8(0xd9, 0x77, 0x06),
        accent_soft: Color::from_rgb8(0x2a, 0x24, 0x20),
        success: Color::from_rgb8(0x5c, 0xb9, 0x7a),
        warning: Color::from_rgb8(0xd4, 0xa8, 0x53),
        danger: Color::from_rgb8(0xd9, 0x6c, 0x6c),
    }
}

impl Palette {
    pub fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => light(),
            ThemeMode::Dark => dark(),
        }
    }

    pub fn iced_theme(mode: ThemeMode) -> Theme {
        match mode {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::Dark,
        }
    }

    pub fn text_input_style(
        self,
    ) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
        move |_, status| {
            let border_color = match status {
                text_input::Status::Active => self.border,
                text_input::Status::Hovered => self.accent,
                text_input::Status::Focused { .. } => self.accent,
                text_input::Status::Disabled => self.border,
            };

            text_input::Style {
                background: Background::Color(self.surface),
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: border_color,
                },
                icon: self.text_muted,
                placeholder: self.text_muted,
                value: self.text,
                selection: self.accent_soft,
            }
        }
    }

    pub fn button_style(
        self,
    ) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let mut style = button::Style {
                background: Some(Background::Color(self.surface)),
                text_color: self.text,
                border: Border {
                    radius: 4.0.into(),
                    width: 1.0,
                    color: self.border,
                },
                shadow: Shadow::default(),
                snap: true,
            };

            match status {
                button::Status::Active => {}
                button::Status::Hovered => {
                    style.background = Some(Background::Color(self.surface_alt));
                    style.border.color = self.accent;
                }
                button::Status::Pressed => {
                    style.background = Some(Background::Color(self.accent_soft));
                    style.text_color = self.accent;
                    style.border.color = self.accent;
                }
                button::Status::Disabled => {
                    style.background = Some(Background::Color(self.surface));
                    style.text_color = self.text_muted;
                    style.border.color = self.border;
                }
            }

            style
        }
    }

    pub fn panel_style(
        self,
    ) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.surface)),
            border: Border {
                radius: 6.0.into(),
                width: 1.0,
                color: self.border,
            },
            shadow: Shadow {
                color: Color::from_rgba8(0x00, 0x00, 0x00, 0.25),
                offset: iced::Vector::new(0.0, 18.0),
                blur_radius: 28.0,
            },
            snap: true,
        }
    }

    pub fn backdrop_style(
        self,
    ) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.surface_overlay)),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        }
    }

    pub fn section_style(
        self,
    ) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.background)),
            border: Border {
                radius: 0.0.into(),
                width: 1.0,
                color: self.border,
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }

    pub fn status_dot_style(
        self,
        color: Color,
    ) -> impl Fn(&Theme) -> container::Style {
        move |_| container::Style {
            text_color: Some(color),
            background: Some(Background::Color(color)),
            border: Border {
                radius: 99.0.into(),
                width: 0.0,
                color,
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}
