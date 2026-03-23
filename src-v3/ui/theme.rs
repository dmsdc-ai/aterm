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
    pub surface_elevated: Color,
    pub surface_overlay: Color,
    pub border: Color,
    pub border_subtle: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub accent_hover: Color,
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
        surface_elevated: Color::from_rgb8(0xff, 0xff, 0xff),
        surface_overlay: Color::from_rgba8(0x00, 0x00, 0x00, 0.40),
        border: Color::from_rgba8(0xd4, 0xcf, 0xc7, 0.60),
        border_subtle: Color::from_rgba8(0xe5, 0xe0, 0xd8, 0.40),
        text: Color::from_rgb8(0x1a, 0x1a, 0x1a),
        text_secondary: Color::from_rgb8(0x3d, 0x3d, 0x3d),
        text_muted: Color::from_rgb8(0x88, 0x88, 0x88),
        text_disabled: Color::from_rgb8(0xaa, 0xaa, 0xaa),
        accent: Color::from_rgb8(0xd9, 0x77, 0x06),
        accent_hover: Color::from_rgb8(0xb4, 0x53, 0x09),
        accent_soft: Color::from_rgba8(0xd9, 0x77, 0x06, 0.08),
        success: Color::from_rgb8(0x3d, 0xa8, 0x5e),
        warning: Color::from_rgb8(0xc4, 0x95, 0x2e),
        danger: Color::from_rgb8(0xd4, 0x55, 0x55),
    }
}

pub fn dark() -> Palette {
    Palette {
        background: Color::from_rgb8(0x0d, 0x11, 0x17),
        surface: Color::from_rgb8(0x16, 0x1b, 0x22),
        surface_alt: Color::from_rgb8(0x1c, 0x21, 0x28),
        surface_elevated: Color::from_rgb8(0x2d, 0x33, 0x3b),
        surface_overlay: Color::from_rgba8(0x00, 0x00, 0x00, 0.50),
        border: Color::from_rgba8(0x3d, 0x44, 0x4d, 0.40),
        border_subtle: Color::from_rgba8(0x30, 0x36, 0x3d, 0.25),
        text: Color::from_rgb8(0xc9, 0xd1, 0xd9),
        text_secondary: Color::from_rgb8(0x8b, 0x94, 0x9e),
        text_muted: Color::from_rgb8(0x6e, 0x76, 0x81),
        text_disabled: Color::from_rgb8(0x48, 0x4f, 0x58),
        accent: Color::from_rgb8(0xd9, 0x77, 0x06),
        accent_hover: Color::from_rgb8(0xf5, 0x9e, 0x0b),
        accent_soft: Color::from_rgba8(0xd9, 0x77, 0x06, 0.12),
        success: Color::from_rgb8(0x3f, 0xb9, 0x50),
        warning: Color::from_rgb8(0xd2, 0x9a, 0x22),
        danger: Color::from_rgb8(0xf8, 0x53, 0x49),
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
                text_input::Status::Focused { .. } => self.accent_hover,
                text_input::Status::Disabled => self.border_subtle,
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
                    radius: 6.0.into(),
                    width: 1.0,
                    color: self.border,
                },
                shadow: Shadow::default(),
                snap: true,
            };

            match status {
                button::Status::Active => {}
                button::Status::Hovered => {
                    style.background = Some(Background::Color(self.surface_elevated));
                    style.border.color = self.accent_hover;
                }
                button::Status::Pressed => {
                    style.background = Some(Background::Color(self.accent_soft));
                    style.text_color = self.accent;
                    style.border.color = self.accent;
                }
                button::Status::Disabled => {
                    style.background = Some(Background::Color(self.surface));
                    style.text_color = self.text_disabled;
                    style.border.color = self.border_subtle;
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
            background: Some(Background::Color(self.surface_elevated)),
            border: Border {
                radius: 12.0.into(),
                width: 1.0,
                color: self.border,
            },
            shadow: Shadow {
                color: Color::from_rgba8(0x00, 0x00, 0x00, 0.25),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 40.0,
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

    pub fn ghost_button_style(
        self,
    ) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let (text_color, bg) = match status {
                button::Status::Hovered => (self.text, self.surface_alt),
                button::Status::Pressed => (self.accent, self.surface_elevated),
                button::Status::Disabled => (self.text_disabled, Color::TRANSPARENT),
                button::Status::Active => (self.text_muted, Color::TRANSPARENT),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    radius: 6.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: true,
            }
        }
    }

    pub fn danger_ghost_button_style(
        self,
    ) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let (text_color, bg) = match status {
                button::Status::Hovered => (self.danger, self.surface_alt),
                button::Status::Pressed => (self.danger, self.surface_elevated),
                button::Status::Disabled => (self.text_disabled, Color::TRANSPARENT),
                button::Status::Active => (self.text_muted, Color::TRANSPARENT),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    radius: 6.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: true,
            }
        }
    }

    pub fn primary_button_style(
        self,
    ) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let (bg, text_color) = match status {
                button::Status::Hovered => (self.accent_hover, Color::WHITE),
                button::Status::Pressed => (self.accent, Color::WHITE),
                button::Status::Disabled => (self.surface_elevated, self.text_disabled),
                button::Status::Active => (self.accent, Color::WHITE),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    radius: 6.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: true,
            }
        }
    }

    pub fn card_button_style(
        self,
        active: bool,
        dimmed: bool,
    ) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_, status| {
            let background = if active {
                self.accent_soft
            } else {
                match status {
                    button::Status::Hovered => self.surface_alt,
                    button::Status::Pressed => self.surface_elevated,
                    _ => Color::TRANSPARENT,
                }
            };
            let text_color = if dimmed {
                self.text_disabled
            } else {
                self.text
            };
            button::Style {
                background: Some(Background::Color(background)),
                text_color,
                border: Border {
                    radius: 4.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: true,
            }
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
            shadow: Shadow {
                color: Color { a: 0.6, ..color },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 5.0,
            },
            snap: true,
        }
    }
}
