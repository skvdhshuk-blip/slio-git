//! N1 — Log filter bar (text + date from/to + Clear + count label).

use crate::i18n::I18n;
use crate::log_filter::DateValidation;
use crate::theme::{self, darcula};
use iced::widget::text_input as iced_text_input;
use iced::widget::{Button, Row, Space, Text, TextInput, text};
use iced::{Alignment, Background, Border, Element, Length, Theme};

/// Messages produced by the filter bar.
#[derive(Debug, Clone)]
pub enum FilterBarMessage {
    TextChanged(String),
    TextSubmit,
    DateFromChanged(String),
    DateToChanged(String),
    ClearFilter,
}

/// Render the filter bar row.
pub fn view<'a>(
    text_value: &'a str,
    date_from_text: &'a str,
    date_to_text: &'a str,
    validation: DateValidation,
    shown: usize,
    total: usize,
    history_commit_limit: u32,
    i18n: &'a I18n,
) -> Element<'a, FilterBarMessage> {
    let from_err = validation.from_invalid || validation.range_inverted;
    let to_err = validation.to_invalid || validation.range_inverted;

    let text_input_widget = TextInput::new(i18n.lf_text_placeholder, text_value)
        .padding([4, 8])
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .width(Length::FillPortion(3))
        .style(theme::text_input_style())
        .on_input(FilterBarMessage::TextChanged)
        .on_submit(FilterBarMessage::TextSubmit);

    let date_from_widget = TextInput::new(i18n.lf_date_placeholder, date_from_text)
        .padding([4, 8])
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .width(Length::FillPortion(2))
        .style(date_input_style(from_err))
        .on_input(FilterBarMessage::DateFromChanged);

    let date_to_widget = TextInput::new(i18n.lf_date_placeholder, date_to_text)
        .padding([4, 8])
        .size(theme::typography::CAPTION_SIZE)
        .font(theme::app_font())
        .line_height(text::LineHeight::Relative(1.0))
        .width(Length::FillPortion(2))
        .style(date_input_style(to_err))
        .on_input(FilterBarMessage::DateToChanged);

    let has_filter =
        !text_value.is_empty() || !date_from_text.is_empty() || !date_to_text.is_empty();
    let clear_btn = Button::new(
        Text::new(i18n.lf_clear)
            .size(theme::typography::CAPTION_SIZE)
            .color(darcula::TEXT_SECONDARY),
    )
    .style(theme::button_style(theme::ButtonTone::Ghost))
    .padding([4, 8])
    .on_press_maybe(has_filter.then_some(FilterBarMessage::ClearFilter));

    let is_filtered = shown != total;
    let at_limit = shown >= history_commit_limit as usize;
    let count_label: Element<'a, FilterBarMessage> = if shown == 0 && is_filtered {
        let mut col = iced::widget::Column::new().spacing(2).push(
            Text::new(i18n.lf_no_match)
                .size(theme::typography::CAPTION_SIZE)
                .color(darcula::WARNING),
        );
        if at_limit {
            col = col.push(
                Text::new(i18n.lf_limit_outside_range)
                    .size(theme::typography::CAPTION_SIZE.saturating_sub(1))
                    .color(darcula::TEXT_SECONDARY),
            );
        }
        col.into()
    } else {
        let count_text = i18n
            .lf_count_fmt
            .replace("{shown}", &shown.to_string())
            .replace("{total}", &total.to_string());
        let mut col = iced::widget::Column::new().spacing(2).push(
            Text::new(count_text)
                .size(theme::typography::CAPTION_SIZE)
                .color(darcula::TEXT_SECONDARY),
        );
        if is_filtered && at_limit {
            let hint = i18n
                .lf_limit_hint_fmt
                .replace("{n}", &history_commit_limit.to_string());
            col = col.push(
                Text::new(hint)
                    .size(theme::typography::CAPTION_SIZE.saturating_sub(1))
                    .color(darcula::TEXT_DISABLED)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            );
        }
        col.into()
    };

    Row::new()
        .spacing(theme::spacing::XS)
        .align_y(Alignment::Center)
        .padding([4, 8])
        .push(text_input_widget)
        .push(
            Text::new(i18n.lf_date_from_label)
                .size(theme::typography::CAPTION_SIZE)
                .color(darcula::TEXT_SECONDARY),
        )
        .push(date_from_widget)
        .push(
            Text::new(i18n.lf_date_to_label)
                .size(theme::typography::CAPTION_SIZE)
                .color(darcula::TEXT_SECONDARY),
        )
        .push(date_to_widget)
        .push(clear_btn)
        .push(Space::new().width(Length::Fixed(4.0)))
        .push(count_label)
        .into()
}

fn date_input_style(
    is_error: bool,
) -> impl Fn(&Theme, iced_text_input::Status) -> iced_text_input::Style {
    move |_theme, status| {
        let field_bg = darcula::BG_CARD_2;
        let (background, border_color, border_width) = if is_error {
            (field_bg, darcula::DANGER, 1.5)
        } else {
            match status {
                iced_text_input::Status::Focused { .. } => (field_bg, darcula::ACCENT, 1.5),
                iced_text_input::Status::Hovered => {
                    (field_bg, darcula::ACCENT.scale_alpha(0.45), 1.0)
                }
                _ => (field_bg, darcula::BORDER, 1.0),
            }
        };
        iced_text_input::Style {
            background: Background::Color(background),
            border: Border {
                width: border_width,
                color: border_color,
                radius: crate::theme::radius::MD.into(),
            },
            icon: darcula::TEXT_SECONDARY,
            placeholder: darcula::TEXT_DISABLED,
            value: darcula::TEXT_PRIMARY,
            selection: darcula::SELECTION_BG,
        }
    }
}
