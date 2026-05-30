//! Clone dialog view.
//!
//! Provides a dialog for cloning a remote repository.

use crate::i18n::I18n;
use crate::theme::{self, BadgeTone, Surface};
use crate::widgets::{self, OptionalPush, button, scrollable, text_input};
use git_core::clone::CloneOptions;
use iced::widget::{Column, Container, Row, Space, Text};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;

/// Message types for the clone dialog.
#[derive(Debug, Clone)]
pub enum CloneMessage {
    SetUrl(String),
    SetDirectory(String),
    BrowseParent,
    ToggleShallow,
    SetDepth(String),
    Execute,
    Cancel,
}

/// State for the clone dialog.
#[derive(Debug, Clone)]
pub struct CloneDialogState {
    pub url: String,
    pub directory: String,
    pub parent_dir: String,
    pub shallow: bool,
    pub depth: String,
    pub url_validation: Option<String>,
    pub is_cloning: bool,
    pub progress: Option<f64>,
    pub error: Option<String>,
    /// Whether the dialog is open.
    pub open: bool,
}

impl CloneDialogState {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            directory: String::new(),
            parent_dir: home_dir_string(),
            shallow: false,
            depth: "1".to_string(),
            url_validation: None,
            is_cloning: false,
            progress: None,
            error: None,
            open: false,
        }
    }

    /// Reset the dialog to its initial state (preserves open flag).
    pub fn reset(&mut self) {
        self.url.clear();
        self.directory.clear();
        self.parent_dir = home_dir_string();
        self.shallow = false;
        self.depth = "1".to_string();
        self.url_validation = None;
        self.is_cloning = false;
        self.progress = None;
        self.error = None;
    }

    /// Open the dialog.
    pub fn open(&mut self) {
        self.reset();
        self.open = true;
    }

    /// Close the dialog.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Validate the URL and return an error message if invalid.
    pub fn validate_url(&mut self) {
        if self.url.trim().is_empty() {
            self.url_validation = None;
            return;
        }
        match git_core::clone::validate_clone_url(&self.url) {
            Ok(()) => self.url_validation = None,
            Err(e) => self.url_validation = Some(e.to_string()),
        }
    }

    /// Extract the repo name from the URL and update the directory field.
    pub fn auto_fill_directory(&mut self) {
        if let Some(name) = extract_repo_name(&self.url) {
            self.directory = name;
        }
    }

    /// Build the CloneOptions from current state.
    pub fn build_options(&self) -> Option<CloneOptions> {
        if self.url.trim().is_empty() || self.directory.trim().is_empty() {
            return None;
        }
        let depth = if self.shallow {
            self.depth.trim().parse::<u32>().ok().filter(|&d| d > 0)
        } else {
            None
        };
        Some(CloneOptions {
            url: self.url.trim().to_string(),
            parent_dir: PathBuf::from(&self.parent_dir),
            directory_name: self.directory.trim().to_string(),
            depth,
            branch: None,
        })
    }

    /// Returns the full destination path.
    pub fn destination_path(&self) -> PathBuf {
        PathBuf::from(&self.parent_dir).join(self.directory.trim())
    }
}

impl Default for CloneDialogState {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the repository name from a URL.
///
/// Handles HTTPS, SSH, and SCP-style URLs.
/// e.g. `https://github.com/user/repo.git` -> `repo`
///      `git@github.com:user/repo.git` -> `repo`
fn extract_repo_name(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Get the last path segment
    let path_part = if let Some(pos) = trimmed.find("://") {
        // scheme://host/path
        &trimmed[pos + 3..]
    } else if let Some(pos) = trimmed.find(':') {
        // SCP-style: user@host:path
        &trimmed[pos + 1..]
    } else {
        trimmed
    };

    // Split by '/' and take the last non-empty segment
    let segment = path_part
        .split('/')
        .filter(|s| !s.is_empty())
        .next_back()?;

    // Strip .git suffix
    let name = segment.strip_suffix(".git").unwrap_or(segment);
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Get the home directory as a string, falling back to "~".
fn home_dir_string() -> String {
    dirs::home_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~".to_string())
}

/// Build the clone dialog view.
pub fn view<'a>(state: &'a CloneDialogState, i18n: &'a I18n) -> Element<'a, CloneMessage> {
    // ── Header ──
    let header = Container::new(
        Row::new()
            .align_y(Alignment::Center)
            .push(
                Text::new(i18n.clone_title)
                    .size(14)
                    .color(theme::darcula::TEXT_PRIMARY),
            )
            .push(Space::new().width(Length::Fill))
            .push(button::compact_ghost(
                i18n.close,
                Some(CloneMessage::Cancel),
            )),
    )
    .padding([6, 14])
    .width(Length::Fill)
    .style(theme::frame_style(Surface::Toolbar));

    // ── URL input with validation ──
    let url_section = Container::new(
        Column::new()
            .spacing(theme::spacing::XS)
            .push(
                Text::new(i18n.clone_url_label)
                    .size(11)
                    .color(theme::darcula::TEXT_SECONDARY),
            )
            .push(text_input::styled(
                "https://github.com/user/repo.git",
                &state.url,
                CloneMessage::SetUrl,
            ))
            .push_maybe(state.url_validation.as_ref().map(|msg| {
                Text::new(msg)
                    .size(11)
                    .color(theme::darcula::DANGER)
            })),
    )
    .padding([8, 14]);

    // ── Directory input + Browse ──
    let dir_section = Container::new(
        Column::new()
            .spacing(theme::spacing::XS)
            .push(
                Text::new(i18n.clone_directory_label)
                    .size(11)
                    .color(theme::darcula::TEXT_SECONDARY),
            )
            .push(
                Row::new()
                    .spacing(theme::spacing::XS)
                    .align_y(Alignment::Center)
                    .push(
                        Container::new(text_input::styled(
                            i18n.clone_directory_label,
                            &state.directory,
                            CloneMessage::SetDirectory,
                        ))
                        .width(Length::Fill),
                    )
                    .push(button::secondary(
                        i18n.clone_browse_btn,
                        Some(CloneMessage::BrowseParent),
                    )),
            )
            .push(
                Text::new(format!("{}/{}", state.parent_dir, state.directory))
                    .size(11)
                    .color(theme::darcula::TEXT_DISABLED)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
            ),
    )
    .padding([8, 14]);

    // ── Shallow clone options ──
    let shallow_section = Container::new(
        Column::new()
            .spacing(theme::spacing::XS)
            .push(widgets::compact_checkbox(
                state.shallow,
                i18n.clone_shallow_label,
                |_| CloneMessage::ToggleShallow,
            ))
            .push_maybe(state.shallow.then(|| -> Element<'_, CloneMessage> {
                Row::new()
                    .spacing(theme::spacing::XS)
                    .align_y(Alignment::Center)
                    .push(
                        Text::new(i18n.clone_depth_label)
                            .size(11)
                            .color(theme::darcula::TEXT_SECONDARY),
                    )
                    .push(
                        Container::new(text_input::styled(
                            "1",
                            &state.depth,
                            CloneMessage::SetDepth,
                        ))
                        .width(Length::Fixed(80.0)),
                    )
                    .into()
            })),
    )
    .padding([8, 14]);

    // ── Progress bar ──
    let progress_section: Option<Element<'_, CloneMessage>> = if state.is_cloning {
        Some(
            widgets::progress_bar::progress_bar_view(
                i18n.clone_progress_fmt,
                state.progress.map(|p| p as f32),
                None,
                |_| CloneMessage::Cancel,
            ),
        )
    } else {
        None
    };

    // ── Error display ──
    let error_section: Option<Element<'_, CloneMessage>> = state.error.as_ref().map(|err| {
        widgets::status_banner(
            i18n.clone_error_invalid_url,
            err,
            BadgeTone::Danger,
        )
    });

    // ── Footer buttons ──
    let can_clone = !state.is_cloning
        && !state.url.trim().is_empty()
        && !state.directory.trim().is_empty()
        && state.url_validation.is_none();

    let footer = Container::new(
        Row::new()
            .spacing(theme::spacing::SM)
            .align_y(Alignment::Center)
            .push(Space::new().width(Length::Fill))
            .push(button::ghost(i18n.clone_cancel, Some(CloneMessage::Cancel)))
            .push(button::primary(
                i18n.clone_btn,
                can_clone.then_some(CloneMessage::Execute),
            )),
    )
    .padding([8, 14])
    .width(Length::Fill)
    .style(theme::frame_style(Surface::Toolbar));

    // ── Assembly ──
    let mut body = Column::new().spacing(0).width(Length::Fill);
    body = body.push(header);
    body = body.push(iced::widget::rule::horizontal(1));
    body = body.push(url_section);
    body = body.push(dir_section);
    body = body.push(shallow_section);
    if let Some(p) = progress_section {
        body = body.push(p);
    }
    if let Some(e) = error_section {
        body = body.push(e);
    }
    body = body.push(iced::widget::rule::horizontal(1));
    body = body.push(footer);

    Container::new(scrollable::styled(body).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel_style(Surface::Panel))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_repo_name_https() {
        assert_eq!(
            extract_repo_name("https://github.com/user/repo.git"),
            Some("repo".to_string())
        );
    }

    #[test]
    fn extract_repo_name_ssh() {
        assert_eq!(
            extract_repo_name("git@github.com:user/repo.git"),
            Some("repo".to_string())
        );
    }

    #[test]
    fn extract_repo_name_no_dotgit() {
        assert_eq!(
            extract_repo_name("https://github.com/user/repo"),
            Some("repo".to_string())
        );
    }

    #[test]
    fn extract_repo_name_empty() {
        assert_eq!(extract_repo_name(""), None);
    }

    #[test]
    fn default_state_is_closed() {
        let state = CloneDialogState::new();
        assert!(!state.open);
        assert_eq!(state.depth, "1");
        assert!(!state.shallow);
    }

    #[test]
    fn open_sets_flag() {
        let mut state = CloneDialogState::new();
        state.open();
        assert!(state.open);
    }

    #[test]
    fn close_clears_flag() {
        let mut state = CloneDialogState::new();
        state.open();
        state.close();
        assert!(!state.open);
    }

    #[test]
    fn build_options_empty_url_returns_none() {
        let state = CloneDialogState::new();
        assert!(state.build_options().is_none());
    }

    #[test]
    fn build_options_valid() {
        let mut state = CloneDialogState::new();
        state.url = "https://github.com/user/repo.git".to_string();
        state.directory = "repo".to_string();
        let opts = state.build_options().unwrap();
        assert_eq!(opts.url, "https://github.com/user/repo.git");
        assert_eq!(opts.directory_name, "repo");
        assert!(opts.depth.is_none());
    }

    #[test]
    fn build_options_shallow_with_depth() {
        let mut state = CloneDialogState::new();
        state.url = "https://github.com/user/repo.git".to_string();
        state.directory = "repo".to_string();
        state.shallow = true;
        state.depth = "5".to_string();
        let opts = state.build_options().unwrap();
        assert_eq!(opts.depth, Some(5));
    }

    #[test]
    fn build_options_shallow_zero_depth_is_none() {
        let mut state = CloneDialogState::new();
        state.url = "https://github.com/user/repo.git".to_string();
        state.directory = "repo".to_string();
        state.shallow = true;
        state.depth = "0".to_string();
        let opts = state.build_options().unwrap();
        assert!(opts.depth.is_none());
    }
}
