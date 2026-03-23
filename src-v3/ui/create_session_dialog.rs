use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::ui::theme::Palette;
use super::cli_presets::PRESETS;

#[derive(Debug, Clone, Default)]
pub struct CreateSessionState {
    pub open: bool,
    pub cwd: String,
    pub selected_preset: usize,
    pub custom_command: String,
    pub custom_args: String,
}

#[derive(Debug, Clone)]
pub enum CreateSessionAction {
    Close,
    SelectPreset(usize),
    Create,
    CustomCommandChanged(String),
    CustomArgsChanged(String),
}

#[derive(Debug, Clone, Copy)]
pub struct CreateSessionDialog {
    palette: Palette,
}

impl CreateSessionDialog {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }

    pub fn view<'a>(&self, state: &'a CreateSessionState) -> Element<'a, CreateSessionAction> {
        if !state.open {
            return Space::new().width(0).height(0).into();
        }

        let palette = self.palette;
        let folder_name = state
            .cwd
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("workspace");

        // Preset buttons grid
        let preset_buttons: Vec<Element<'a, CreateSessionAction>> = PRESETS
            .iter()
            .enumerate()
            .map(|(i, preset)| {
                let is_selected = i == state.selected_preset;
                button(
                    column![
                        text(preset.icon).size(20),
                        text(preset.label).size(12),
                    ]
                    .spacing(4)
                    .align_x(Alignment::Center)
                    .width(Fill),
                )
                .width(Fill)
                .padding(Padding::from([12, 8]))
                .style(move |_, status| {
                    let bg = if is_selected {
                        palette.accent_soft
                    } else {
                        match status {
                            iced::widget::button::Status::Hovered => palette.surface_alt,
                            iced::widget::button::Status::Pressed => palette.surface_elevated,
                            _ => palette.surface,
                        }
                    };
                    iced::widget::button::Style {
                        background: Some(iced::Background::Color(bg)),
                        text_color: if is_selected { palette.accent } else { palette.text },
                        border: iced::Border {
                            radius: 8.0.into(),
                            width: if is_selected { 1.0 } else { 0.0 },
                            color: if is_selected { palette.accent } else { iced::Color::TRANSPARENT },
                        },
                        shadow: iced::Shadow::default(),
                        snap: true,
                    }
                })
                .on_press(CreateSessionAction::SelectPreset(i))
                .into()
            })
            .collect();

        let preset_row = row(preset_buttons).spacing(8);

        // Preview command or custom inputs
        let selected_index = state.selected_preset.min(PRESETS.len().saturating_sub(1));
        let preset = &PRESETS[selected_index];
        let is_custom = preset.id == "custom";

        let detail_section: Element<'a, CreateSessionAction> = if is_custom {
            column![
                text_input("Command (e.g. python3)", &state.custom_command)
                    .on_input(CreateSessionAction::CustomCommandChanged)
                    .padding(Padding::from([10, 14]))
                    .size(14)
                    .style(palette.text_input_style()),
                text_input("Arguments (e.g. --verbose --port 8080)", &state.custom_args)
                    .on_input(CreateSessionAction::CustomArgsChanged)
                    .padding(Padding::from([10, 14]))
                    .size(14)
                    .style(palette.text_input_style()),
            ]
            .spacing(8)
            .into()
        } else {
            let preview = format!("{} {}", preset.command, preset.args.join(" "));
            container(
                text(preview)
                    .size(11)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text_muted),
                    }),
            )
            .padding([8, 12])
            .width(Fill)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(palette.background)),
                border: iced::Border {
                    radius: 4.0.into(),
                    width: 0.0,
                    color: palette.border,
                },
                ..Default::default()
            })
            .into()
        };

        // Dialog content
        let dialog_content = column![
            // Header
            row![
                text("New Session")
                    .size(16)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text),
                    }),
                Space::new().width(Fill),
                text(folder_name)
                    .size(12)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text_muted),
                    }),
            ]
            .align_y(Alignment::Center),
            // Label
            text("CLI")
                .size(11)
                .style(move |_| iced::widget::text::Style {
                    color: Some(palette.text_muted),
                }),
            // Presets
            preset_row,
            // Detail: custom inputs or preview
            detail_section,
            // Buttons row
            row![
                button(text("Cancel").size(13))
                    .padding(Padding::from([8, 20]))
                    .style(palette.button_style())
                    .on_press(CreateSessionAction::Close),
                Space::new().width(Fill),
                button(text("Create").size(13))
                    .padding(Padding::from([8, 20]))
                    .style(palette.primary_button_style())
                    .on_press(CreateSessionAction::Create),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(24)
        .width(Length::Fixed(400.0));

        // Modal with backdrop
        let panel = container(dialog_content).style(palette.panel_style());

        // Center the panel
        container(
            container(panel)
                .width(Fill)
                .height(Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .style(palette.backdrop_style())
        .into()
    }
}
