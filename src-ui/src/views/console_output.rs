//! T2 — Git Console Output: bottom collapsible panel showing git command stdout/stderr.

use crate::theme::{self, darcula, ButtonTone, Surface};
use iced::widget::scrollable as iced_scrollable;
use iced::widget::{Button, Column, Container, Row, Space, Text, rule, text};
use iced::{Alignment, Element, Length};

const RING_BUFFER_MAX: usize = 500;

/// Ring buffer for console output lines, capped at 500 entries.
#[derive(Debug, Clone, Default)]
pub struct ConsoleRingBuffer {
    lines: Vec<String>,
}

impl ConsoleRingBuffer {
    pub fn push(&mut self, line: String) {
        let filtered = credential_filter(&line);
        self.lines.push(filtered);
        if self.lines.len() > RING_BUFFER_MAX {
            let excess = self.lines.len() - RING_BUFFER_MAX;
            self.lines.drain(0..excess);
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Console output panel state.
#[derive(Debug, Clone, Default)]
pub struct ConsoleOutputState {
    pub visible: bool,
    pub buffer: ConsoleRingBuffer,
    pub scroll_offset: f32,
}

impl ConsoleOutputState {
    pub fn append_line(&mut self, line: String) {
        self.buffer.push(line);
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}

/// Messages produced by the console output panel.
#[derive(Debug, Clone)]
pub enum ConsoleOutputMessage {
    Toggle,
    Clear,
    AppendLine(String),
    ScrollChanged(iced::widget::scrollable::Viewport),
}

/// Render the console output bottom panel.
pub fn view<'a>(
    state: &'a ConsoleOutputState,
    title: &'a str,
    clear_label: &'a str,
    close_label: &'a str,
    empty_label: &'a str,
) -> Element<'a, ConsoleOutputMessage> {
    let header = Row::new()
        .spacing(theme::spacing::XS)
        .align_y(Alignment::Center)
        .push(
            Text::new("Console")
                .size(12)
                .color(darcula::TEXT_PRIMARY),
        )
        .push(Space::new().width(Length::Fixed(8.0)))
        .push(
            Text::new(title)
                .size(10)
                .color(darcula::TEXT_DISABLED),
        )
        .push(Space::new().width(Length::Fill))
        .push(Button::new(
            Text::new(clear_label)
                .size(11)
                .color(darcula::TEXT_SECONDARY),
        )
        .style(theme::button_style(ButtonTone::Ghost))
        .padding([2, 6])
        .on_press_maybe((!state.buffer.is_empty()).then_some(ConsoleOutputMessage::Clear)))
        .push(Button::new(
            Text::new(close_label)
                .size(11)
                .color(darcula::TEXT_SECONDARY),
        )
        .style(theme::button_style(ButtonTone::Ghost))
        .padding([2, 6])
        .on_press(ConsoleOutputMessage::Toggle));

    let body: Element<'_, ConsoleOutputMessage> = if state.buffer.is_empty() {
        Container::new(
            Text::new(empty_label)
                .size(12)
                .color(darcula::TEXT_DISABLED),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    } else {
        let output_text = state.buffer.lines().join("\n");
        let scroll = iced_scrollable::Scrollable::new(
            Text::new(output_text)
                .size(11)
                .font(iced::Font::MONOSPACE)
                .color(darcula::TEXT_PRIMARY)
                .line_height(text::LineHeight::Relative(1.4)),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(console_scrollable_style)
        .on_scroll(ConsoleOutputMessage::ScrollChanged);

        Container::new(scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([4, 8])
            .into()
    };

    Container::new(
        Column::new()
            .spacing(0)
            .push(
                Container::new(header)
                    .padding(theme::density::SECONDARY_BAR_PADDING)
                    .style(theme::frame_style(Surface::Nav)),
            )
            .push(rule::horizontal(1).style(theme::separator_rule_style()))
            .push(
                Container::new(body)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(theme::frame_style(Surface::Root)),
            ),
    )
    .height(Length::Fixed(theme::layout::TOOL_WINDOW_HEIGHT))
    .width(Length::Fill)
    .style(theme::panel_style(Surface::Panel))
    .into()
}

fn console_scrollable_style(
    _theme: &iced::Theme,
    _status: iced_scrollable::Status,
) -> iced_scrollable::Style {
    iced_scrollable::Style {
        container: iced::widget::container::Style {
            background: Some(iced::Background::Color(darcula::BG_EDITOR)),
            ..Default::default()
        },
        vertical_rail: iced::widget::scrollable::Rail {
            background: Some(iced::Background::Color(darcula::BG_CARD)),
            border: iced::Border {
                radius: 3.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
            scroller: iced::widget::scrollable::Scroller {
                background: darcula::TEXT_DISABLED.into(),
                border: iced::Border {
                    radius: 3.0.into(),
                    width: 0.0,
                    color: iced::Color::TRANSPARENT,
                },
            },
        },
        horizontal_rail: iced::widget::scrollable::Rail {
            background: None,
            border: iced::Border::default(),
            scroller: iced::widget::scrollable::Scroller {
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border::default(),
            },
        },
        gap: None,
        auto_scroll: iced::widget::scrollable::AutoScroll {
            background: iced::Color::TRANSPARENT.into(),
            border: iced::Border::default(),
            shadow: iced::Shadow::default(),
            icon: iced::Color::TRANSPARENT,
        },
    }
}

/// Filter credential-like patterns from a console output line.
fn credential_filter(line: &str) -> String {
    let lower = line.to_lowercase();

    let has_credential = lower.contains("token=")
        || lower.contains("password=")
        || lower.contains("passwd=")
        || lower.contains("secret=")
        || lower.contains("authorization:")
        || lower.contains("-password")
        || lower.contains("access_key")
        || lower.contains("secret_key")
        || lower.contains("private_key")
        || lower.contains("credential")
        || lower.contains("ghp_")
        || lower.contains("github_pat_")
        || (lower.contains("https://") || lower.contains("http://"))
            && lower.contains('@')
            && lower.contains(':');

    if !has_credential {
        return line.to_string();
    }

    mask_credential_values(line)
}

fn mask_credential_values(line: &str) -> String {
    let lower = line.to_lowercase();
    let mut result = line.to_string();

    // Build a list of (start, end) replace ranges, working on the original line
    // so we can process all matches without recursing.
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    // Mask key=value and key:value credential patterns
    let cred_keys = [
        "token",
        "password",
        "passwd",
        "secret",
        "access_key",
        "secret_key",
        "private_key",
        "credential",
    ];

    for key in cred_keys {
        let prefix_eq = format!("{key}=");
        let prefix_colon = format!("{key}:");

        for prefix in [&prefix_eq, &prefix_colon] {
            let mut search_start = 0;
            while let Some(pos) = lower[search_start..].find(prefix.as_str()) {
                let val_start = search_start + pos + prefix.len();
                if val_start < lower.len() {
                    let rest = &lower[val_start..];
                    let val_end = val_start
                        + rest
                            .find(|c: char| c.is_whitespace() || c == ',' || c == ';')
                            .unwrap_or(rest.len());
                    if val_end > val_start {
                        ranges.push((val_start, val_end));
                    }
                }
                search_start = val_start + 1;
            }
        }
    }

    // Mask Bearer token: "Bearer <token>"
    for (start, _) in lower.match_indices("bearer ") {
        let val_start = start + 7;
        let rest = &lower[val_start..];
        let val_end = val_start
            + rest
                .find(|c: char| c.is_whitespace() || c == ',' || c == ';')
                .unwrap_or(rest.len());
        if val_end > val_start {
            ranges.push((val_start, val_end));
        }
    }

    // Mask --password flag values
    for (flag_start, _) in lower.match_indices("--password") {
        let after_flag = flag_start + 10;
        if after_flag >= lower.len() {
            continue;
        }
        let val_text = lower[after_flag..].trim_start();
        let offset = after_flag + (lower[after_flag..].len() - val_text.len());
        if val_text.is_empty() {
            continue;
        }
        if val_text.starts_with('=') {
            let val_start = offset + 1;
            let rest = &lower[val_start..];
            let val_end = rest
                .find(|c: char| c.is_whitespace())
                .map(|p| val_start + p)
                .unwrap_or(lower.len());
            if val_end > val_start {
                ranges.push((val_start, val_end));
            }
        } else {
            let val_end = val_text
                .find(|c: char| c.is_whitespace())
                .map(|p| offset + p)
                .unwrap_or(lower.len());
            if val_end > offset {
                ranges.push((offset, val_end));
            }
        }
    }

    // Mask URL credentials: https://user:pass@
    for scheme in ["https://", "http://"] {
        let mut search_start = 0;
        while let Some(pos) = lower[search_start..].find(scheme) {
            let abs_pos = search_start + pos;
            let after_scheme = &lower[abs_pos + scheme.len()..];
            if let Some(at_pos) = after_scheme.find('@') {
                let creds = &after_scheme[..at_pos];
                if creds.contains(':') {
                    let start = abs_pos + scheme.len();
                    let end = abs_pos + scheme.len() + at_pos;
                    ranges.push((start, end));
                }
            }
            search_start = abs_pos + 1;
        }
    }

    // Apply ranges in reverse order so positions stay valid
    ranges.sort_by_key(|(s, _)| *s);
    ranges.reverse();
    let mut seen = std::collections::BTreeSet::new();
    for (start, end) in ranges {
        // Skip overlapping ranges (only keep the first for each start)
        if seen.contains(&start) {
            continue;
        }
        // Only apply if range hasn't been replaced yet
        if start < result.len() && end <= result.len() {
            result.replace_range(start..end, "***");
            seen.insert(start);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_enforces_500_limit() {
        let mut buf = ConsoleRingBuffer::default();
        for i in 0..520 {
            buf.push(format!("line {i}"));
        }
        assert_eq!(buf.lines().len(), 500);
        assert_eq!(buf.lines().first().unwrap(), "line 20");
        assert_eq!(buf.lines().last().unwrap(), "line 519");
    }

    #[test]
    fn ring_buffer_clear() {
        let mut buf = ConsoleRingBuffer::default();
        buf.push("hello".into());
        buf.clear();
        assert!(buf.is_empty());
    }

    #[test]
    fn credential_filter_masks_token_assignment() {
        let masked = credential_filter("token=ghp_abc123secret");
        assert!(!masked.contains("ghp_abc123secret"));
        assert!(masked.contains("***"));
    }

    #[test]
    fn credential_filter_masks_password_flag() {
        let masked = credential_filter("git clone --password mypass123 repo");
        assert!(!masked.contains("mypass123"));
        assert!(masked.contains("***"));
    }

    #[test]
    fn credential_filter_masks_bearer_token() {
        let masked = credential_filter("Authorization: Bearer xyz789token");
        assert!(!masked.contains("xyz789token"));
        assert!(masked.contains("***"));
    }

    #[test]
    fn credential_filter_masks_url_credentials() {
        let masked = credential_filter("Fetching https://user:pass123@example.com/repo.git");
        assert!(!masked.contains("user:pass123"));
        assert!(masked.contains("***"));
    }

    #[test]
    fn credential_filter_passes_safe_lines() {
        let safe = credential_filter("Already up to date.");
        assert_eq!(safe, "Already up to date.");
    }

    #[test]
    fn console_state_toggle_and_append() {
        let mut state = ConsoleOutputState::default();
        assert!(!state.visible);
        state.toggle();
        assert!(state.visible);
        state.append_line("test".into());
        assert_eq!(state.buffer.lines().len(), 1);
    }
}
