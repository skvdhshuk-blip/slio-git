//! Welcome screen — IDEA FlatWelcomeFrame.kt:111 parity.
//!
//! Layout: left recent list / right actions (Open + Clone disabled + version).
//! Ref: RecentProjectsManagerBase.kt:551+:350 / NewRecentProjectPanel.java /
//!      RecentProjectListActionProvider.kt

use crate::i18n::I18n;
use crate::state::ProjectEntry;
use crate::theme::{self, Surface};
use crate::widgets::button;
use iced::widget::{Column, Container, Row, Space, Text};
use iced::{Alignment, Color, Element, Length, Padding};
use std::time::{SystemTime, UNIX_EPOCH};

const ROW_HEIGHT: f32 = 42.0;
const LEFT_PANEL_WIDTH: f32 = 340.0;
const PATH_MAX_DISPLAY_LEN: usize = 48;

/// Messages emitted by the welcome view.
#[derive(Debug, Clone)]
pub enum WelcomeMessage {
    OpenProject(std::path::PathBuf),
}

/// Humanize a unix-seconds timestamp relative to now.
/// Returns `—` for None (AC-15 legacy entries).
pub fn humanize_time(last_opened: Option<u64>, i18n: &I18n) -> String {
    let Some(ts) = last_opened else {
        return "\u{2014}".to_string(); // —
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now.saturating_sub(ts);
    if secs < 60 {
        return i18n.welcome_just_now.to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return i18n
            .welcome_minutes_ago_fmt
            .replace("{}", &mins.to_string());
    }
    let hours = mins / 60;
    if hours < 24 {
        return i18n.welcome_hours_ago_fmt.replace("{}", &hours.to_string());
    }
    let days = hours / 24;
    i18n.welcome_days_ago_fmt.replace("{}", &days.to_string())
}

/// Left-truncate path to at most `max_chars` keeping the last segment intact.
/// e.g. `/very/long/path/to/repo` → `…/path/to/repo`
pub fn truncate_path_left(path: &std::path::Path, max_chars: usize) -> String {
    let s = path.display().to_string();
    if s.len() <= max_chars {
        return s;
    }
    let trimmed = &s[s.len().saturating_sub(max_chars)..];
    // advance to the next path separator so we don't cut mid-component
    if let Some(pos) = trimmed.find('/').or_else(|| trimmed.find('\\')) {
        format!("\u{2026}{}", &trimmed[pos..])
    } else {
        format!("\u{2026}{}", trimmed)
    }
}

/// Render the full welcome body.
/// `selected_idx` — None or index of highlighted row for keyboard navigation (AC-8).
pub fn view<'a, Message>(
    recent: &'a [ProjectEntry],
    selected_idx: Option<usize>,
    i18n: &'a I18n,
    on_open: impl Fn(std::path::PathBuf) -> Message + 'a,
    on_open_folder: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let left = build_recent_panel(recent, selected_idx, i18n, on_open);
    let right = build_actions_panel(i18n, on_open_folder);

    let content = Row::new()
        .spacing(0)
        .height(Length::Fill)
        .push(left)
        .push(right);

    Container::new(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::frame_style(Surface::Root))
        .into()
}

fn build_recent_panel<'a, Message>(
    recent: &'a [ProjectEntry],
    selected_idx: Option<usize>,
    i18n: &'a I18n,
    on_open: impl Fn(std::path::PathBuf) -> Message + 'a,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let title = Text::new(i18n.welcome_recent_list_title)
        .size(13)
        .color(theme::darcula::TEXT_SECONDARY);

    let list: Element<'a, Message> = if recent.is_empty() {
        Text::new(i18n.welcome_no_recent)
            .size(12)
            .color(theme::darcula::TEXT_DISABLED)
            .into()
    } else {
        let rows: Vec<Element<'a, Message>> = recent
            .iter()
            .enumerate()
            .map(|(i, entry)| build_recent_row(entry, i, selected_idx, i18n, &on_open))
            .collect();
        Column::with_children(rows).spacing(0).into()
    };

    Container::new(
        Column::new()
            .spacing(theme::spacing::SM)
            .push(
                Container::new(title)
                    .padding([theme::spacing::MD as u16, theme::spacing::LG as u16]),
            )
            .push(list),
    )
    .width(Length::Fixed(LEFT_PANEL_WIDTH))
    .height(Length::Fill)
    .style(theme::panel_style(Surface::Panel))
    .into()
}

fn build_recent_row<'a, Message>(
    entry: &'a ProjectEntry,
    idx: usize,
    selected_idx: Option<usize>,
    i18n: &'a I18n,
    on_open: &impl Fn(std::path::PathBuf) -> Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let path_exists = entry.path.exists();
    let is_selected = selected_idx == Some(idx);

    let name_color = if path_exists {
        theme::darcula::ACCENT
    } else {
        theme::darcula::TEXT_DISABLED
    };
    let path_color = if path_exists {
        theme::darcula::TEXT_SECONDARY
    } else {
        theme::darcula::TEXT_DISABLED
    };

    let path_display = truncate_path_left(&entry.path, PATH_MAX_DISPLAY_LEN);
    let time_str = humanize_time(entry.last_opened, i18n);

    let main_row = Row::new()
        .spacing(theme::spacing::SM)
        .align_y(Alignment::Center)
        .push(
            Text::new(&entry.name)
                .size(13)
                .color(name_color)
                .width(Length::Fill),
        )
        .push(
            Text::new(time_str)
                .size(11)
                .color(theme::darcula::TEXT_DISABLED),
        );

    let path_row = Row::new().push(
        Text::new(path_display)
            .size(11)
            .color(path_color)
            .width(Length::Fill),
    );

    let row_content = Column::new().spacing(2).push(main_row).push(path_row);

    let surface = if is_selected {
        Surface::ListSelection
    } else {
        Surface::ListRow
    };

    let padded = Container::new(row_content)
        .padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: theme::spacing::LG,
            right: theme::spacing::LG,
        })
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(theme::panel_style(surface));

    if path_exists {
        let msg = on_open(entry.path.clone());
        // Row is a button-like click area; wrap in mouse_area via iced button pattern
        iced::widget::button(padded)
            .style(|_theme, _status| iced::widget::button::Style {
                background: None,
                ..Default::default()
            })
            .width(Length::Fill)
            .on_press(msg)
            .into()
    } else {
        // Greyed out, not clickable — AC-9
        // The tooltip would require iced::widget::tooltip; we embed the path-missing text
        // as secondary text (tooltip widget support deferred if unavailable)
        let missing_hint = Text::new(i18n.welcome_recent_path_missing_tooltip)
            .size(10)
            .color(Color {
                a: 0.5,
                ..theme::darcula::WARNING
            });
        iced::widget::button(
            Container::new(
                Column::new()
                    .spacing(2)
                    .push(
                        Row::new()
                            .spacing(theme::spacing::SM)
                            .align_y(Alignment::Center)
                            .push(
                                Text::new(&entry.name)
                                    .size(13)
                                    .color(name_color)
                                    .width(Length::Fill),
                            ),
                    )
                    .push(
                        Text::new(truncate_path_left(&entry.path, PATH_MAX_DISPLAY_LEN))
                            .size(11)
                            .color(path_color)
                            .width(Length::Fill),
                    )
                    .push(missing_hint),
            )
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: theme::spacing::LG,
                right: theme::spacing::LG,
            })
            .width(Length::Fill)
            .style(theme::panel_style(Surface::ListRow)),
        )
        .style(|_theme, _status| iced::widget::button::Style {
            background: None,
            ..Default::default()
        })
        .width(Length::Fill)
        .into()
    }
}

// Version string is 'static because env!() is a compile-time string literal
static VERSION_STR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn version_display() -> &'static str {
    VERSION_STR.get_or_init(|| format!("v{}", env!("CARGO_PKG_VERSION")))
}

fn build_actions_panel<'a, Message>(i18n: &'a I18n, on_open_folder: Message) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let open_btn = button::primary(i18n.welcome_open_folder_btn, Some(on_open_folder));

    // Clone disabled placeholder — AC-12
    let clone_btn = button::secondary(i18n.welcome_clone_btn, None::<Message>);

    let clone_hint = Text::new(i18n.welcome_clone_tooltip)
        .size(11)
        .color(theme::darcula::TEXT_DISABLED);

    let version_label = Text::new(version_display())
        .size(11)
        .color(theme::darcula::TEXT_DISABLED);

    let actions = Column::new()
        .spacing(theme::spacing::SM)
        .push(open_btn)
        .push(clone_btn)
        .push(clone_hint);

    let right_content = Column::new()
        .spacing(theme::spacing::LG)
        .height(Length::Fill)
        .push(
            Container::new(actions).padding([theme::spacing::LG as u16, theme::spacing::LG as u16]),
        )
        .push(Space::new().height(Length::Fill))
        .push(
            Container::new(version_label)
                .padding([theme::spacing::MD as u16, theme::spacing::LG as u16])
                .width(Length::Fill),
        );

    Container::new(right_content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::frame_style(Surface::Root))
        .into()
}
