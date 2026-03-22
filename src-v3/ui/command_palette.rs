use std::borrow::Cow;

use iced::widget::{button, column, container, row, stack, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::ui::theme::{Palette, ThemeMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    Deliberate,
    Group,
    Broadcast,
    Theme(ThemeMode),
}

#[derive(Debug, Clone)]
pub struct CommandEntry<'a> {
    pub command: PaletteCommand,
    pub title: Cow<'a, str>,
    pub description: Cow<'a, str>,
    pub shortcut: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub enum CommandPaletteAction {
    Close,
    QueryChanged(String),
    Execute(PaletteCommand),
    Submit,
    Select(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct CommandPalette {
    palette: Palette,
}

impl CommandPalette {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }

    pub fn view<'a>(
        &self,
        state: &CommandPaletteState,
        commands: &'a [CommandEntry<'a>],
    ) -> Element<'a, CommandPaletteAction> {
        if !state.open {
            return Space::new().into();
        }

        let filtered = self.filtered_commands(state, commands);
        let selected_index = state.selected.min(filtered.len().saturating_sub(1));

        let input = text_input("Type a command...", &state.query)
            .on_input(CommandPaletteAction::QueryChanged)
            .on_submit(CommandPaletteAction::Submit)
            .padding(Padding::from([14, 16]))
            .style(self.palette.text_input_style());

        let results = if filtered.is_empty() {
            column![empty_state(self.palette)].spacing(8)
        } else {
            column(
                filtered
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| command_row(*entry, index == selected_index, self.palette)),
            )
            .spacing(8)
        };

        let panel = container(
            column![
                row![
                    text("Command Palette").size(20),
                    Space::new().width(Fill),
                    button(text("Esc")).style(self.palette.button_style()).on_press(CommandPaletteAction::Close),
                ]
                .align_y(Alignment::Center),
                text("deliberate, group, broadcast, theme").size(12),
                input,
                container(results).width(Fill),
            ]
            .spacing(14),
        )
        .padding(24)
        .width(Length::Fixed(620.0))
        .style(self.palette.panel_style());

        let centered_panel = container(panel)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill);

        stack([
            container(Space::new().width(Fill).height(Fill))
                .width(Fill)
                .height(Fill)
                .style(self.palette.backdrop_style())
                .into(),
            centered_panel.into(),
        ])
        .into()
    }

    pub fn filtered_commands<'a>(
        &self,
        state: &CommandPaletteState,
        commands: &'a [CommandEntry<'a>],
    ) -> Vec<&'a CommandEntry<'a>> {
        let query = state.query.trim().to_lowercase();
        if query.is_empty() {
            return commands.iter().collect();
        }

        commands
            .iter()
            .filter(|entry| {
                entry.title.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn selected_command<'a>(
        &self,
        state: &CommandPaletteState,
        commands: &'a [CommandEntry<'a>],
    ) -> Option<&'a CommandEntry<'a>> {
        let filtered = self.filtered_commands(state, commands);
        filtered.get(state.selected.min(filtered.len().saturating_sub(1))).copied()
    }
}

fn empty_state<'a>(palette: Palette) -> Element<'a, CommandPaletteAction> {
    container(
        column![
            text("No matching command").size(14),
            text("Try deliberate, group, broadcast, or theme").size(12),
        ]
        .spacing(4),
    )
    .padding(Padding::from([14, 16]))
    .style(move |_| iced::widget::container::Style {
        text_color: Some(palette.text_muted),
        background: Some(iced::Background::Color(palette.surface_alt)),
        border: iced::Border {
            radius: 16.0.into(),
            width: 1.0,
            color: palette.border,
        },
        shadow: iced::Shadow::default(),
        snap: true,
    })
    .into()
}

fn command_row<'a>(
    entry: &'a CommandEntry<'a>,
    selected: bool,
    palette: Palette,
) -> Element<'a, CommandPaletteAction> {
    let shortcut = entry
        .shortcut
        .as_ref()
        .map(|value| value.as_ref())
        .unwrap_or("");
    let shortcut_widget: Element<'a, CommandPaletteAction> = if shortcut.is_empty() {
        Space::new().into()
    } else {
        container(text(shortcut).size(11))
            .padding(Padding::from([4, 8]))
            .style(move |_| iced::widget::container::Style {
                text_color: Some(palette.text),
                background: Some(iced::Background::Color(palette.accent_soft)),
                border: iced::Border {
                    radius: 999.0.into(),
                    width: 1.0,
                    color: palette.border,
                },
                shadow: iced::Shadow::default(),
                snap: true,
            })
            .into()
    };

    button(
        row![
            column![
                text(entry.title.as_ref()).size(14),
                text(entry.description.as_ref())
                    .size(12)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text_muted),
                    }),
            ]
            .spacing(3)
            .width(Fill),
            shortcut_widget,
        ]
        .spacing(12)
        .align_y(Alignment::Center)
    )
    .width(Fill)
    .padding(Padding::from([14, 16]))
    .style(move |_, status| {
        let mut style = palette.button_style()(&iced::Theme::Dark, status);
        if selected {
            style.background = Some(iced::Background::Color(palette.accent_soft));
            style.border.color = palette.accent;
        }
        style
    })
    .on_press(CommandPaletteAction::Execute(entry.command))
    .into()
}
