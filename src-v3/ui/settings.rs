use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Fill, Length, Padding};
use crate::ui::theme::{Palette, ThemeMode};

#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub open: bool,
}

#[derive(Debug, Clone)]
pub enum SettingsAction {
    Close,
    SetTheme(ThemeMode),
}

pub struct SettingsPanel {
    palette: Palette,
}

impl SettingsPanel {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }

    pub fn view<'a>(&self, state: &'a SettingsState, current_theme: ThemeMode) -> Element<'a, SettingsAction> {
        if !state.open {
            return Space::new().width(0).height(0).into();
        }

        let palette = self.palette;

        let theme_button = |label: &'static str, mode: ThemeMode| -> Element<'a, SettingsAction> {
            let is_active = current_theme == mode;
            button(text(label).size(12))
                .padding(Padding::from([8, 16]))
                .width(Fill)
                .style(move |_, status| {
                    let bg = if is_active {
                        palette.accent
                    } else {
                        match status {
                            iced::widget::button::Status::Hovered => palette.surface_alt,
                            iced::widget::button::Status::Pressed => palette.surface_elevated,
                            _ => palette.surface,
                        }
                    };
                    iced::widget::button::Style {
                        background: Some(iced::Background::Color(bg)),
                        text_color: if is_active { iced::Color::WHITE } else { palette.text },
                        border: iced::Border {
                            radius: 6.0.into(),
                            width: 0.0,
                            color: iced::Color::TRANSPARENT,
                        },
                        shadow: iced::Shadow::default(),
                        snap: true,
                    }
                })
                .on_press(SettingsAction::SetTheme(mode))
                .into()
        };

        let content = column![
            text("Settings").size(16).style(move |_| iced::widget::text::Style {
                color: Some(palette.text),
            }),
            text("THEME").size(11).style(move |_| iced::widget::text::Style {
                color: Some(palette.text_muted),
            }),
            row![
                theme_button("Dark", ThemeMode::Dark),
                theme_button("Light", ThemeMode::Light),
            ].spacing(8),
            Space::new().height(8),
            row![
                Space::new().width(Fill),
                button(text("Close").size(13))
                    .padding(Padding::from([8, 20]))
                    .style(palette.button_style())
                    .on_press(SettingsAction::Close),
            ],
        ]
        .spacing(16)
        .padding(24)
        .width(Length::Fixed(320.0));

        let panel = container(content)
            .style(palette.panel_style());

        container(
            container(panel)
                .width(Fill)
                .height(Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
        .width(Fill)
        .height(Fill)
        .style(palette.backdrop_style())
        .into()
    }
}
