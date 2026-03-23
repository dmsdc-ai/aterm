use std::borrow::Cow;

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Fill, Length, Padding,
    Shadow,
};

use crate::ui::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Local,
    Telepty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Online,
    Busy,
    Offline,
    Stale,
    Running,
    Dead,
    Unknown,
}

impl SessionStatus {
    pub fn color(self, palette: Palette) -> Color {
        match self {
            Self::Online | Self::Running => palette.success,
            Self::Busy => palette.warning,
            Self::Offline => palette.text_muted,
            Self::Dead => palette.danger,
            Self::Stale => palette.warning,
            Self::Unknown => palette.border,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionEntry<'a> {
    pub id: Cow<'a, str>,
    pub title: Cow<'a, str>,
    pub subtitle: Cow<'a, str>,
    pub status: SessionStatus,
    pub kind: SessionKind,
    pub pending_injects: usize,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct GroupEntry<'a> {
    pub id: Cow<'a, str>,
    pub title: Cow<'a, str>,
    pub subtitle: Cow<'a, str>,
    pub members: usize,
    pub active: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SidebarModel<'a> {
    pub local_sessions: &'a [SessionEntry<'a>],
    pub telepty_sessions: &'a [SessionEntry<'a>],
    pub groups: &'a [GroupEntry<'a>],
}

impl<'a> SidebarModel<'a> {
    pub fn empty() -> Self {
        Self {
            local_sessions: &[],
            telepty_sessions: &[],
            groups: &[],
        }
    }
}

#[derive(Debug, Clone)]
pub enum SidebarAction {
    SelectSession(String),
    SelectGroup(String),
    CreateSession,
    DeleteSession(String),
    OpenSettings,
}

#[derive(Debug, Clone, Copy)]
pub struct Sidebar {
    palette: Palette,
}

impl Sidebar {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }

    pub fn view<'a>(&self, model: SidebarModel<'a>) -> Element<'a, SidebarAction> {
        let palette = self.palette;
        let content = column![
            sessions_header(palette),
            section("Local", model.local_sessions, palette, |item| {
                SidebarAction::SelectSession(item.id.to_string())
            }),
            section("Remote", model.telepty_sessions, palette, |item| {
                SidebarAction::SelectSession(item.id.to_string())
            }),
            group_section(model.groups, palette),
        ]
        .spacing(16)
        .padding(Padding::default().top(16))
        .width(Fill);

        let footer = settings_button(palette);

        container(
            column![scrollable(content).height(Fill).width(Fill), footer]
                .width(Fill)
                .height(Fill),
        )
            .width(Length::Fixed(250.0))
            .height(Fill)
            .style(move |_| iced::widget::container::Style {
                text_color: Some(palette.text),
                background: Some(Background::Color(palette.surface)),
                border: Border {
                    radius: 0.0.into(),
                    width: 0.0,
                    color: palette.border,
                },
                shadow: Shadow::default(),
                snap: true,
            })
            .into()
    }
}

fn settings_button<'a>(palette: Palette) -> Element<'a, SidebarAction> {
    container(
        button(
            container(text("\u{2699}").size(13))
                .width(28)
                .height(28)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
            .padding(0)
            .style(palette.ghost_button_style())
            .on_press(SidebarAction::OpenSettings),
    )
    .padding(Padding::from([8, 16]))
    .width(Fill)
    .into()
}

fn sessions_header<'a>(palette: Palette) -> Element<'a, SidebarAction> {
    row![
        section_label("Sessions", palette),
        Space::new().width(Fill),
        button(
            container(text("+").size(14))
                .width(28)
                .height(28)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
            .padding(0)
            .style(palette.ghost_button_style())
            .on_press(SidebarAction::CreateSession),
    ]
    .align_y(Alignment::Center)
    .padding(Padding::default().right(8))
    .width(Fill)
    .into()
}

fn section<'a, F>(
    title: &'a str,
    items: &'a [SessionEntry<'a>],
    palette: Palette,
    map: F,
) -> Element<'a, SidebarAction>
where
    F: Fn(&SessionEntry<'a>) -> SidebarAction + Copy + 'a,
{
    let cards = items.iter().map(|item| session_card(item, palette, map));
    column![
        section_label(title, palette),
        column(cards).spacing(4),
    ]
    .spacing(8)
    .into()
}

fn group_section<'a>(items: &'a [GroupEntry<'a>], palette: Palette) -> Element<'a, SidebarAction> {
    let cards = items.iter().map(|item| group_card(item, palette));
    column![
        section_label("Groups", palette),
        column(cards).spacing(4),
    ]
    .spacing(8)
    .into()
}

fn section_label<'a>(title: &'a str, palette: Palette) -> Element<'a, SidebarAction> {
    container(text(title.to_uppercase()).size(9).style(move |_| iced::widget::text::Style {
        color: Some(palette.text_muted),
    }))
    .padding(Padding { top: 12.0, right: 16.0, bottom: 4.0, left: 16.0 })
    .into()
}

fn session_card<'a, F>(
    item: &'a SessionEntry<'a>,
    palette: Palette,
    map: F,
) -> Element<'a, SidebarAction>
where
    F: Fn(&SessionEntry<'a>) -> SidebarAction + Copy + 'a,
{
    let dot_color = item.status.color(palette);
    let is_dead = matches!(item.status, SessionStatus::Dead);
    let item_id = item.id.to_string();

    let delete_btn = button(
        container(text("\u{00d7}").size(14))
            .width(22)
            .height(22)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
        .padding(0)
        .style(palette.danger_ghost_button_style())
        .on_press(SidebarAction::DeleteSession(item_id));

    let inject_badge: Element<'a, SidebarAction> = if item.pending_injects > 0 {
        container(
            text(format!("{}", item.pending_injects)).size(9)
                .style(move |_| iced::widget::text::Style {
                    color: Some(Color::WHITE),
                })
        )
        .padding(Padding::from([1, 4]))
        .style(move |_| iced::widget::container::Style {
            background: Some(Background::Color(palette.accent)),
            border: Border { radius: 8.0.into(), width: 0.0, color: palette.accent },
            ..Default::default()
        })
        .into()
    } else {
        Space::new().width(0).height(0).into()
    };

    button(
        row![
            selection_strip(item.active.then_some(palette.accent)),
            row![
                status_dot(dot_color, palette),
                column![
                    row![
                        text(item.title.as_ref()).size(12),
                        inject_badge,
                    ].spacing(6).align_y(Alignment::Center),
                    text(item.subtitle.as_ref()).size(10).style(move |_| {
                        iced::widget::text::Style {
                            color: Some(palette.text_muted),
                        }
                    }),
                ]
                .spacing(2)
                .width(Fill),
                delete_btn,
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding(Padding::from([10, 16]))
            .width(Fill),
        ]
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Fill)
    )
    .width(Fill)
    .padding(0)
    .style(palette.card_button_style(item.active, is_dead))
    .on_press(map(item))
    .into()
}

fn group_card<'a>(item: &'a GroupEntry<'a>, palette: Palette) -> Element<'a, SidebarAction> {
    let accent = item.active.then_some(palette.accent);

    button(
        row![
            selection_strip(accent),
            row![
                status_dot(
                    if item.active { palette.accent } else { palette.border },
                    palette,
                ),
                column![
                    row![
                        text(item.title.as_ref()).size(12),
                        Space::new().width(Fill),
                        text(format!("{} sessions", item.members))
                            .size(10)
                            .style(move |_| iced::widget::text::Style {
                                color: Some(palette.text_muted),
                            }),
                    ]
                    .align_y(Alignment::Center),
                    text(item.subtitle.as_ref()).size(10).style(move |_| {
                        iced::widget::text::Style {
                            color: Some(palette.text_muted),
                        }
                    }),
                ]
                .spacing(2)
                .width(Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding(Padding::from([8, 16]))
            .width(Fill),
        ]
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Fill)
    )
    .width(Fill)
    .padding(0)
    .style(palette.card_button_style(item.active, false))
    .on_press(SidebarAction::SelectGroup(item.id.to_string()))
    .into()
}

fn selection_strip<'a>(accent: Option<Color>) -> Element<'a, SidebarAction> {
    let color = accent.unwrap_or(Color::TRANSPARENT);
    container(Space::new().width(3).height(Fill))
        .width(3)
        .height(Fill)
        .style(move |_| iced::widget::container::Style {
            text_color: None,
            background: Some(Background::Color(color)),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        })
        .into()
}

fn status_dot<'a>(color: Color, palette: Palette) -> Element<'a, SidebarAction> {
    container(Space::new().width(8).height(8))
        .width(8)
        .height(8)
        .style(palette.status_dot_style(color))
        .into()
}
