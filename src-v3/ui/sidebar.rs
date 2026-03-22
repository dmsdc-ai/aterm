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
        .spacing(8)
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
        button(text("⚙").size(13))
            .padding(Padding::from([6, 16]))
            .style(move |_, status| {
                let text_color = match status {
                    button::Status::Hovered | button::Status::Pressed => palette.accent,
                    button::Status::Disabled => palette.text_muted,
                    button::Status::Active => palette.text_muted,
                };

                button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color,
                    border: Border {
                        radius: 0.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: Shadow::default(),
                    snap: true,
                }
            })
            .on_press(SidebarAction::OpenSettings),
    )
    .width(Fill)
    .into()
}

fn sessions_header<'a>(palette: Palette) -> Element<'a, SidebarAction> {
    row![
        section_label("Sessions", palette),
        Space::new().width(Fill),
        button(text("+").size(14))
            .padding(Padding::from([6, 16]))
            .style(move |_, status| {
                let text_color = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        palette.accent
                    }
                    button::Status::Disabled => palette.text_muted,
                    button::Status::Active => palette.text_muted,
                };

                button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color,
                    border: Border {
                        radius: 0.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: Shadow::default(),
                    snap: true,
                }
            })
            .on_press(SidebarAction::CreateSession),
    ]
    .align_y(Alignment::Center)
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
        column(cards).spacing(2),
    ]
    .spacing(10)
    .into()
}

fn group_section<'a>(items: &'a [GroupEntry<'a>], palette: Palette) -> Element<'a, SidebarAction> {
    let cards = items.iter().map(|item| group_card(item, palette));
    column![
        section_label("Groups", palette),
        column(cards).spacing(2),
    ]
    .spacing(10)
    .into()
}

fn section_label<'a>(title: &'a str, palette: Palette) -> Element<'a, SidebarAction> {
    container(text(title.to_uppercase()).size(11)).style(move |_| iced::widget::container::Style {
        text_color: Some(palette.text_muted),
        background: None,
        border: Border {
            radius: 3.0.into(),
            width: 0.0,
            color: palette.border,
        },
        shadow: Shadow::default(),
        snap: true,
    })
    .padding([0, 16])
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
    let accent = item.active.then_some(palette.accent);

    button(
        row![
            selection_strip(accent),
            row![
                status_dot(dot_color, palette),
                column![
                    text(item.title.as_ref()).size(12),
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
            .padding(Padding::from([6, 16]))
            .width(Fill),
        ]
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Fill)
    )
    .width(Fill)
    .padding(0)
    .style(move |_, status| card_style(palette, item.active, status))
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
            .padding(Padding::from([6, 16]))
            .width(Fill),
        ]
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Fill)
    )
    .width(Fill)
    .padding(0)
    .style(move |_, status| card_style(palette, item.active, status))
    .on_press(SidebarAction::SelectGroup(item.id.to_string()))
    .into()
}

fn selection_strip<'a>(accent: Option<Color>) -> Element<'a, SidebarAction> {
    let color = accent.unwrap_or(Color::TRANSPARENT);
    container(Space::new().width(2).height(Fill))
        .width(2)
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

fn card_style(
    palette: Palette,
    active: bool,
    status: button::Status,
) -> button::Style {
    let background = if active {
        palette.surface_alt
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => palette.surface_alt,
            button::Status::Disabled | button::Status::Active => palette.surface,
        }
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: palette.text,
        border: Border {
            radius: 0.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn status_dot<'a>(color: Color, _palette: Palette) -> Element<'a, SidebarAction> {
    container(Space::new().width(7).height(7))
        .width(7)
        .height(7)
        .style(move |_| iced::widget::container::Style {
            text_color: None,
            background: Some(Background::Color(color)),
            border: Border {
                radius: 3.5.into(),
                width: 0.0,
                color,
            },
            shadow: Shadow {
                color: Color { a: 0.5, ..color },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 4.0,
            },
            snap: true,
        })
        .into()
}
