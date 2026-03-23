use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length, Padding};
use crate::ui::theme::Palette;

#[derive(Debug, Clone, Default)]
pub struct DeliberateDialogState {
    pub open: bool,
    pub topic: String,
}

#[derive(Debug, Clone)]
pub enum DeliberateAction {
    Close,
    TopicChanged(String),
    Submit,
}

pub struct DeliberateDialog {
    palette: Palette,
}

impl DeliberateDialog {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }

    pub fn view<'a>(&self, state: &'a DeliberateDialogState) -> Element<'a, DeliberateAction> {
        if !state.open {
            return Space::new().width(0).height(0).into();
        }

        let palette = self.palette;

        let content = column![
            text("Deliberate").size(16).style(move |_| iced::widget::text::Style {
                color: Some(palette.text),
            }),
            text("Enter a topic for multi-agent deliberation").size(12).style(move |_| iced::widget::text::Style {
                color: Some(palette.text_muted),
            }),
            text_input("e.g. Design the auth system...", &state.topic)
                .on_input(DeliberateAction::TopicChanged)
                .on_submit(DeliberateAction::Submit)
                .padding(Padding::from([10, 14]))
                .size(14)
                .style(palette.text_input_style()),
            row![
                button(text("Cancel").size(13))
                    .padding(Padding::from([8, 20]))
                    .style(palette.button_style())
                    .on_press(DeliberateAction::Close),
                Space::new().width(Fill),
                button(text("Start Deliberation").size(13))
                    .padding(Padding::from([8, 20]))
                    .style(palette.primary_button_style())
                    .on_press(DeliberateAction::Submit),
            ].align_y(Alignment::Center),
        ]
        .spacing(16)
        .padding(24)
        .width(Length::Fixed(480.0));

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
