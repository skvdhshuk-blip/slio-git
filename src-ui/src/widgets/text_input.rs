//! Styled text input helpers.

use crate::theme;
use crate::widgets::{OptionalPush, button::compact_ghost};
use iced::widget::{Container, Row, Text, TextInput, text};
use iced::{Element, Length};

pub fn styled<'a, Message: Clone + 'a>(
    placeholder: &'a str,
    value: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
) -> TextInput<'a, Message> {
    TextInput::new(placeholder, value)
        .padding([4, 8])
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .width(Length::Fill)
        .style(theme::text_input_style())
        .on_input(on_change)
}

pub fn styled_password<'a, Message: Clone + 'a>(
    placeholder: &'a str,
    value: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
) -> TextInput<'a, Message> {
    TextInput::new(placeholder, value)
        .padding([4, 8])
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .width(Length::Fill)
        .secure(true)
        .style(theme::text_input_style())
        .on_input(on_change)
}

/// IDEA-style: search input with clear button extension
/// When text is non-empty, shows a clear (×) button on the right
/// Shows a search icon (🔎) on the left inside the input area
pub fn search_with_clear<'a, Message: Clone + 'a>(
    placeholder: &'a str,
    value: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
    on_clear: Message,
) -> Element<'a, Message> {
    let input = TextInput::new(placeholder, value)
        .padding([4, 8])
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .width(Length::Fill)
        .style(theme::text_input_style())
        .on_input(on_change);

    let clear_button = compact_ghost("×", Some(on_clear));

    // IDEA-style: search icon prefix inside the input row
    let search_icon = Text::new("🔎")
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .color(theme::darcula::TEXT_SECONDARY);

    Container::new(
        Row::new()
            .spacing(theme::spacing::XS)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .push(search_icon)
            .push(input)
            .push_maybe((!value.is_empty()).then_some(clear_button)),
    )
    .center_y(Length::Fixed(theme::density::STANDARD_CONTROL_HEIGHT))
    .into()
}
