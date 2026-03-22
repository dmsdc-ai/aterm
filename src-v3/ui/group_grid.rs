use std::borrow::Cow;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::ui::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridPhase {
    Divergence,
    Convergence,
}

impl HybridPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Divergence => "Phase 1: Divergence",
            Self::Convergence => "Phase 2: Convergence",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupGridMember<'a> {
    pub id: Cow<'a, str>,
    pub title: Cow<'a, str>,
    pub subtitle: Cow<'a, str>,
}

#[derive(Debug, Clone)]
pub struct GroupSummaryEntry<'a> {
    pub id: Cow<'a, str>,
    pub title: Cow<'a, str>,
    pub summary: Cow<'a, str>,
}

#[derive(Debug, Clone)]
pub enum GroupGridAction {
    TopicChanged(String),
    Converge,
}

#[derive(Debug, Clone, Copy)]
pub struct GroupGrid {
    palette: Palette,
}

impl GroupGrid {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }

    pub fn view<'a, Message>(
        &self,
        title: &'a str,
        topic: &'a str,
        phase: HybridPhase,
        summary: &'a [GroupSummaryEntry<'a>],
        cells: Vec<Element<'a, Message>>,
        on_action: impl Fn(GroupGridAction) -> Message + Clone + 'a,
    ) -> Element<'a, Message>
    where
        Message: Clone + 'a,
    {
        let palette = self.palette;
        let grid_rows = build_grid_rows(cells);
        let topic_input = text_input("Enter a discussion topic...", topic)
            .on_input({
                let on_action = on_action.clone();
                move |value| on_action(GroupGridAction::TopicChanged(value))
            })
            .padding(Padding::from([10, 14]))
            .style(palette.text_input_style());

        let top_bar = row![
            column![
                text(title).size(20),
                text(phase.label()).size(12).style(move |_| {
                    iced::widget::text::Style {
                        color: Some(palette.text_muted),
                    }
                }),
            ]
            .spacing(4)
            .width(Length::Shrink),
            container(topic_input)
                .width(Fill)
                .padding([0, 12]),
            button(text("Converge"))
                .padding(Padding::from([10, 16]))
                .style(palette.button_style())
                .on_press(on_action(GroupGridAction::Converge)),
        ]
        .align_y(Alignment::Center);

        let summary_panel: Element<'a, Message> = if summary.is_empty() {
            container(text("Summary panel will appear after convergence.").size(12))
                .padding(Padding::from([12, 14]))
                .style(palette.section_style())
                .width(Fill)
                .into()
        } else {
            let entries = column(
                summary.iter().map(|entry| {
                    container(
                        column![
                            text(entry.title.as_ref()).size(14),
                            text(entry.summary.as_ref()).size(12).style(move |_| {
                                iced::widget::text::Style {
                                    color: Some(palette.text_muted),
                                }
                            }),
                        ]
                        .spacing(4),
                    )
                    .padding(Padding::from([12, 14]))
                    .style(palette.panel_style())
                    .width(Fill)
                    .into()
                }),
            )
            .spacing(10);

            container(
                column![
                    text("Convergence Summary").size(14),
                    scrollable(entries).height(Length::Fixed(190.0)),
                ]
                .spacing(10),
            )
            .padding(Padding::from([12, 14]))
            .style(palette.section_style())
            .width(Fill)
            .into()
        };

        container(
            column![
                top_bar,
                column(grid_rows).spacing(12).height(Fill),
                summary_panel,
            ]
            .spacing(14),
        )
        .width(Fill)
        .height(Fill)
        .style(palette.section_style())
        .into()
    }
}

fn build_grid_rows<'a, Message>(cells: Vec<Element<'a, Message>>) -> Vec<Element<'a, Message>>
where
    Message: Clone + 'a,
{
    let count = cells.len();
    if count == 0 {
        return vec![container(text("No sessions in this group.")).into()];
    }

    let layout = row_lengths(count);
    let mut cells = cells.into_iter();
    let mut rows = Vec::new();

    for width in layout {
        let mut current = row!()
            .spacing(12)
            .width(Fill)
            .height(Length::FillPortion(1));
        for _ in 0..width {
            let Some(cell) = cells.next() else {
                break;
            };
            current = current.push(container(cell).width(Fill).height(Fill));
        }
        rows.push(current.into());
    }

    rows
}

fn row_lengths(count: usize) -> Vec<usize> {
    match count {
        0 => vec![],
        1 => vec![1],
        2 => vec![2],
        3 => vec![2, 1],
        4 => vec![2, 2],
        n => {
            let cols = (n as f32).sqrt().ceil() as usize;
            let mut rows = Vec::new();
            let mut remaining = n;
            while remaining > 0 {
                let width = remaining.min(cols);
                rows.push(width);
                remaining -= width;
            }
            rows
        }
    }
}
