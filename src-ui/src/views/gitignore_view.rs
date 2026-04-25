//! Gitignore template picker view.
//!
//! Mirrors IDEA's `addNewElementsToIgnoreFile` flow: pick a template, preview
//! the contents, then append (never overwrite) the block to repo-root
//! `.gitignore`.

use crate::i18n::I18n;
use crate::theme::{self, BadgeTone, Surface};
use crate::widgets::{self, OptionalPush, button, scrollable};
use git_core::{Repository, gitignore_add, gitignore_list_templates, gitignore_template_content};
use iced::widget::{Button, Column, Container, Row, Text};
use iced::{Alignment, Element, Length};

#[derive(Debug, Clone)]
pub enum GitignoreMessage {
    SelectTemplate(String),
    Apply(String),
    Refresh,
    Close,
}

#[derive(Debug, Clone)]
pub struct GitignoreState {
    pub selected: Option<String>,
    pub error: Option<String>,
    pub success_message: Option<String>,
    pub is_applying: bool,
}

impl GitignoreState {
    pub fn new() -> Self {
        let selected = gitignore_list_templates().first().map(|s| s.to_string());
        Self {
            selected,
            error: None,
            success_message: None,
            is_applying: false,
        }
    }

    pub fn select(&mut self, name: String) {
        self.selected = Some(name);
        self.error = None;
        self.success_message = None;
    }

    pub fn apply(&mut self, repo: &Repository, name: String) {
        self.is_applying = true;
        self.error = None;
        self.success_message = None;

        match gitignore_add(repo, &name) {
            Ok(()) => {
                self.success_message = Some(name);
                self.is_applying = false;
            }
            Err(error) => {
                self.error = Some(format!("{error}"));
                self.is_applying = false;
            }
        }
    }
}

impl Default for GitignoreState {
    fn default() -> Self {
        Self::new()
    }
}

fn build_template_row<'a>(name: &str, is_selected: bool) -> Element<'a, GitignoreMessage> {
    let row = Container::new(Text::new(name.to_string()).size(13))
        .padding([8, 12])
        .width(Length::Fill)
        .style(theme::panel_style(if is_selected {
            Surface::Selection
        } else {
            Surface::Raised
        }));

    Button::new(row)
        .style(theme::button_style(theme::ButtonTone::Ghost))
        .on_press(GitignoreMessage::SelectTemplate(name.to_string()))
        .width(Length::Fill)
        .into()
}

fn build_template_list(state: &GitignoreState) -> Element<'_, GitignoreMessage> {
    let templates = gitignore_list_templates();
    let list = templates
        .iter()
        .fold(Column::new().spacing(theme::spacing::XS), |column, name| {
            let selected = state
                .selected
                .as_deref()
                .map(|sel| sel == *name)
                .unwrap_or(false);
            column.push(build_template_row(name, selected))
        });

    Container::new(scrollable::styled(list).height(Length::Fixed(360.0)))
        .padding([10, 12])
        .width(Length::Fixed(180.0))
        .style(theme::panel_style(Surface::Panel))
        .into()
}

fn build_preview<'a>(state: &'a GitignoreState, i18n: &'a I18n) -> Element<'a, GitignoreMessage> {
    let preview_body: Element<'_, GitignoreMessage> = match state
        .selected
        .as_deref()
        .and_then(gitignore_template_content)
    {
        Some(content) => scrollable::styled(
            Container::new(Text::new(content.to_string()).size(12))
                .padding([10, 12])
                .width(Length::Fill),
        )
        .height(Length::Fixed(360.0))
        .into(),
        None => Container::new(
            Text::new(i18n.gi_preview_empty)
                .size(12)
                .color(theme::darcula::TEXT_SECONDARY),
        )
        .padding([16, 16])
        .into(),
    };

    Container::new(
        Column::new()
            .spacing(theme::spacing::SM)
            .push(
                Text::new(i18n.gi_preview_title)
                    .size(13)
                    .color(theme::darcula::TEXT_SECONDARY),
            )
            .push(preview_body),
    )
    .padding([10, 12])
    .width(Length::Fill)
    .style(theme::panel_style(Surface::Panel))
    .into()
}

fn build_action_buttons<'a>(
    state: &'a GitignoreState,
    i18n: &'a I18n,
) -> Element<'a, GitignoreMessage> {
    let can_apply = !state.is_applying && state.selected.is_some();
    let apply_msg = state
        .selected
        .clone()
        .filter(|_| can_apply)
        .map(GitignoreMessage::Apply);

    Row::new()
        .spacing(theme::spacing::XS)
        .push(button::primary(i18n.gi_apply_btn, apply_msg))
        .push(button::ghost(i18n.refresh, Some(GitignoreMessage::Refresh)))
        .push(button::ghost(i18n.close, Some(GitignoreMessage::Close)))
        .into()
}

pub fn view<'a>(state: &'a GitignoreState, i18n: &'a I18n) -> Element<'a, GitignoreMessage> {
    let status: Option<Element<'_, GitignoreMessage>> = if let Some(error) = state.error.as_ref() {
        Some(widgets::status_banner(
            i18n.gi_status_failed,
            error.as_str(),
            BadgeTone::Danger,
        ))
    } else {
        state.success_message.as_ref().map(|applied| {
            widgets::status_banner(
                i18n.gi_status_done,
                i18n.gi_status_done_detail_fmt
                    .replace("{}", applied.as_str()),
                BadgeTone::Success,
            )
        })
    };

    let toolbar = Container::new(
        Row::new()
            .spacing(theme::spacing::XS)
            .align_y(Alignment::Center)
            .push(Text::new(i18n.gi_title).size(16))
            .push(widgets::info_chip::<GitignoreMessage>(
                gitignore_list_templates().len().to_string(),
                BadgeTone::Neutral,
            ))
            .push_maybe(state.selected.as_ref().map(|name| {
                widgets::info_chip::<GitignoreMessage>(
                    i18n.gi_selected_fmt.replace("{}", name),
                    BadgeTone::Accent,
                )
            })),
    )
    .padding([10, 12])
    .style(theme::panel_style(Surface::Panel));

    let body = Row::new()
        .spacing(theme::spacing::SM)
        .push(build_template_list(state))
        .push(build_preview(state, i18n));

    let content = Column::new()
        .spacing(theme::spacing::MD)
        .push(toolbar)
        .push(widgets::section_header(
            i18n.gi_eyebrow,
            i18n.gi_subtitle,
            i18n.gi_detail,
        ))
        .push_maybe(status)
        .push(body)
        .push(build_action_buttons(state, i18n));

    Container::new(scrollable::styled(content).height(Length::Fill))
        .padding([10, 12])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel_style(Surface::Panel))
        .into()
}
