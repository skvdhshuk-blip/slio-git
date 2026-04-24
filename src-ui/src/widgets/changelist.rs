//! Styled change list for the redesigned repository shell.
//!
//! Supports two display modes: flat list and directory tree,
//! with collapsible "Staged" and "Unstaged Changes" groups and
//! per-file stage/unstage icon buttons.

use crate::components::status_icons::FileStatus;
use crate::i18n::I18n;
use crate::state::{ChangeSectionKind, DragState, FileDisplayMode};
use crate::theme::{self, BadgeTone, Surface};
use crate::widgets::{self, scrollable};
use git_core::index::Change;
use iced::widget::{Button, Column, Container, Row, Space, Stack, Text, mouse_area, text};
use iced::{Alignment, Background, Color, Element, Length, Point, mouse};
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

/// Local display kind — includes Untracked which the public enum omits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SectionKind {
    // IDEA-style sort order: STAGED > UNSTAGED > UNTRACKED
    Staged,
    Unstaged,
    #[allow(dead_code)]
    Untracked,
}

impl SectionKind {
    #[allow(dead_code)]
    fn context_label(self) -> &'static str {
        match self {
            SectionKind::Staged => "Staged",
            SectionKind::Unstaged => "Changes",
            SectionKind::Untracked => "New Files",
        }
    }

    fn to_pub(self) -> Option<ChangeSectionKind> {
        match self {
            SectionKind::Staged => Some(ChangeSectionKind::Staged),
            SectionKind::Unstaged => Some(ChangeSectionKind::Unstaged),
            SectionKind::Untracked => None,
        }
    }
}

pub struct ChangesList<'a, Message> {
    i18n: &'a I18n,
    staged: &'a [Change],
    unstaged: &'a [Change],
    untracked: &'a [Change],
    selected_path: Option<&'a str>,
    display_mode: FileDisplayMode,
    staged_collapsed: bool,
    unstaged_collapsed: bool,
    drag: Option<&'a DragState>,
    on_select: Option<Rc<dyn Fn(String) -> Message + 'a>>,
    on_stage: Option<Rc<dyn Fn(String) -> Message + 'a>>,
    on_unstage: Option<Rc<dyn Fn(String) -> Message + 'a>>,
    on_context_menu: Option<Rc<dyn Fn(String) -> Message + 'a>>,
    on_track_cursor: Option<Rc<dyn Fn(Point) -> Message + 'a>>,
    on_toggle_display_mode: Option<Message>,
    on_toggle_staged: Option<Message>,
    on_toggle_unstaged: Option<Message>,
    on_drag_start: Option<Rc<dyn Fn(String, ChangeSectionKind) -> Message + 'a>>,
    on_drag_hover: Option<Rc<dyn Fn(ChangeSectionKind) -> Message + 'a>>,
    on_drag_release: Option<Message>,
    on_drag_cancel: Option<Message>,
}

impl<'a, Message: Clone + 'a> ChangesList<'a, Message> {
    pub fn new(
        i18n: &'a I18n,
        staged: &'a [Change],
        unstaged: &'a [Change],
        untracked: &'a [Change],
    ) -> Self {
        Self {
            i18n,
            staged,
            unstaged,
            untracked,
            selected_path: None,
            display_mode: FileDisplayMode::Flat,
            staged_collapsed: false,
            unstaged_collapsed: false,
            drag: None,
            on_select: None,
            on_stage: None,
            on_unstage: None,
            on_context_menu: None,
            on_track_cursor: None,
            on_toggle_display_mode: None,
            on_toggle_staged: None,
            on_toggle_unstaged: None,
            on_drag_start: None,
            on_drag_hover: None,
            on_drag_release: None,
            on_drag_cancel: None,
        }
    }

    pub fn with_selected_path(mut self, selected_path: Option<&'a str>) -> Self {
        self.selected_path = selected_path;
        self
    }

    pub fn with_display_mode(mut self, mode: FileDisplayMode) -> Self {
        self.display_mode = mode;
        self
    }

    pub fn with_collapsed_state(
        mut self,
        staged_collapsed: bool,
        unstaged_collapsed: bool,
    ) -> Self {
        self.staged_collapsed = staged_collapsed;
        self.unstaged_collapsed = unstaged_collapsed;
        self
    }

    pub fn with_select_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(String) -> Message + 'a,
    {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn with_stage_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(String) -> Message + 'a,
    {
        self.on_stage = Some(Rc::new(handler));
        self
    }

    pub fn with_unstage_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(String) -> Message + 'a,
    {
        self.on_unstage = Some(Rc::new(handler));
        self
    }

    pub fn with_context_menu_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(String) -> Message + 'a,
    {
        self.on_context_menu = Some(Rc::new(handler));
        self
    }

    pub fn with_track_cursor_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(Point) -> Message + 'a,
    {
        self.on_track_cursor = Some(Rc::new(handler));
        self
    }

    pub fn with_toggle_display_mode(mut self, msg: Message) -> Self {
        self.on_toggle_display_mode = Some(msg);
        self
    }

    pub fn with_toggle_staged(mut self, msg: Message) -> Self {
        self.on_toggle_staged = Some(msg);
        self
    }

    pub fn with_toggle_unstaged(mut self, msg: Message) -> Self {
        self.on_toggle_unstaged = Some(msg);
        self
    }

    pub fn with_drag_state(mut self, drag: Option<&'a DragState>) -> Self {
        self.drag = drag;
        self
    }

    pub fn with_drag_start_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(String, ChangeSectionKind) -> Message + 'a,
    {
        self.on_drag_start = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_hover_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(ChangeSectionKind) -> Message + 'a,
    {
        self.on_drag_hover = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_release(mut self, msg: Message) -> Self {
        self.on_drag_release = Some(msg);
        self
    }

    pub fn with_drag_cancel(mut self, msg: Message) -> Self {
        self.on_drag_cancel = Some(msg);
        self
    }

    pub fn view(&self) -> Element<'a, Message> {
        let total_changes = self.staged.len() + self.unstaged.len() + self.untracked.len();
        if total_changes == 0 {
            return widgets::panel_empty_state(
                self.i18n.changes,
                self.i18n.clean_workspace,
                self.i18n.clean_workspace_detail,
                None,
            );
        }

        let mut sections = Column::new().spacing(2);

        // Staged group
        if !self.staged.is_empty() {
            sections = sections.push(self.build_collapsible_section(
                self.i18n.staged_changes,
                self.staged,
                SectionKind::Staged,
                self.staged_collapsed,
                self.on_toggle_staged.clone(),
            ));
        }

        // Unstaged + Untracked group (combined under "Unstaged Changes")
        let unstaged_combined: Vec<&Change> =
            self.unstaged.iter().chain(self.untracked.iter()).collect();
        if !unstaged_combined.is_empty() {
            sections = sections.push(self.build_collapsible_section_refs(
                self.i18n.unstaged_changes,
                &unstaged_combined,
                SectionKind::Unstaged,
                self.unstaged_collapsed,
                self.on_toggle_unstaged.clone(),
            ));
        }

        let scrollable = scrollable::styled(sections).height(Length::Fill);

        let base: Element<'a, Message> = if let Some(handler) = self.on_track_cursor.as_ref() {
            let handle = handler.clone();
            mouse_area(
                Container::new(scrollable)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_move(move |point| handle(point))
            .interaction(mouse::Interaction::Pointer)
            .into()
        } else {
            scrollable.into()
        };

        // Ghost overlay: follows cursor when dragging, no handlers (avoids eating on_enter)
        if let Some(drag) = self.drag.filter(|d| d.started) {
            let ghost_label = split_path(&drag.path).0;
            let ghost = Container::new(
                Text::new(ghost_label)
                    .size(theme::typography::CAPTION_SIZE)
                    .color(Color::WHITE),
            )
            .padding([2, 6])
            .style(theme::panel_style(Surface::ListSelection))
            .width(Length::Shrink);

            let cx = drag.cursor.x.max(0.0) as u16;
            let cy = drag.cursor.y.max(0.0) as u16;
            let ghost_row = Row::new()
                .push(Space::new().width(Length::Fixed(cx as f32)))
                .push(ghost);
            let ghost_col = Column::new()
                .push(Space::new().height(Length::Fixed(cy as f32)))
                .push(ghost_row);

            Stack::new().push(base).push(ghost_col).into()
        } else {
            base
        }
    }

    /// Build toolbar with display mode toggle
    pub fn toolbar(&self) -> Element<'a, Message> {
        let mode_icon = match self.display_mode {
            FileDisplayMode::Flat => "≡",
            FileDisplayMode::Tree => "▤",
        };
        let _mode_tooltip = match self.display_mode {
            FileDisplayMode::Flat => self.i18n.tree_view,
            FileDisplayMode::Tree => self.i18n.flat_view,
        };

        Row::new()
            .spacing(theme::spacing::XS)
            .align_y(Alignment::Center)
            .push(Text::new(self.i18n.changes).size(theme::typography::BODY_SIZE))
            .push(widgets::info_chip::<Message>(
                (self.staged.len() + self.unstaged.len() + self.untracked.len()).to_string(),
                BadgeTone::Neutral,
            ))
            .push(Space::new().width(Length::Fill))
            .push(crate::widgets::button::toolbar_icon(
                mode_icon,
                self.on_toggle_display_mode.clone(),
            ))
            .into()
    }

    fn build_collapsible_section(
        &self,
        title: &'a str,
        changes: &'a [Change],
        kind: SectionKind,
        collapsed: bool,
        on_toggle: Option<Message>,
    ) -> Element<'a, Message> {
        let refs: Vec<&Change> = changes.iter().collect();
        self.build_collapsible_section_refs(title, &refs, kind, collapsed, on_toggle)
    }

    fn build_collapsible_section_refs(
        &self,
        title: &'a str,
        changes: &[&'a Change],
        kind: SectionKind,
        collapsed: bool,
        on_toggle: Option<Message>,
    ) -> Element<'a, Message> {
        let expand_icon = if collapsed { "▶" } else { "▼" };

        // Highlight section header when it is the active drop target
        let is_drop_target = self
            .drag
            .as_ref()
            .and_then(|d| d.hover_kind)
            .and_then(|hk| kind.to_pub().map(|pk| pk == hk))
            .unwrap_or(false);
        let header_surface = if is_drop_target {
            Surface::ListSelection
        } else {
            Surface::ListRow
        };

        let header_row = Row::new()
            .spacing(theme::spacing::XS)
            .align_y(Alignment::Center)
            .push(
                Text::new(expand_icon)
                    .size(theme::typography::MICRO_SIZE)
                    .color(theme::darcula::TEXT_SECONDARY),
            )
            .push(
                Text::new(title)
                    .size(theme::typography::CAPTION_SIZE)
                    .color(theme::darcula::TEXT_SECONDARY),
            )
            .push(widgets::info_chip::<Message>(
                changes.len().to_string(),
                Self::section_badge_tone(kind),
            ));

        let header_container = Container::new(header_row)
            .padding([2, 4])
            .width(Length::Fill)
            .style(theme::panel_style(header_surface));

        let header: Element<'a, Message> = if let Some(pub_kind) = kind.to_pub() {
            if let Some(hover_handler) = self.on_drag_hover.as_ref() {
                let hover_msg = hover_handler(pub_kind);
                let mut area = mouse_area(if let Some(msg) = on_toggle {
                    Button::new(header_container)
                        .style(theme::button_style(theme::ButtonTone::Ghost))
                        .padding(0)
                        .on_press(msg)
                        .width(Length::Fill)
                        .into()
                } else {
                    Element::from(header_container)
                });
                area = area.on_enter(hover_msg);
                area.into()
            } else if let Some(msg) = on_toggle {
                Button::new(header_container)
                    .style(theme::button_style(theme::ButtonTone::Ghost))
                    .padding(0)
                    .on_press(msg)
                    .width(Length::Fill)
                    .into()
            } else {
                Element::from(header_container)
            }
        } else if let Some(msg) = on_toggle {
            Button::new(header_container)
                .style(theme::button_style(theme::ButtonTone::Ghost))
                .padding(0)
                .on_press(msg)
                .width(Length::Fill)
                .into()
        } else {
            Element::from(header_container)
        };

        let mut section = Column::new().spacing(0).push(header);

        if !collapsed {
            match self.display_mode {
                FileDisplayMode::Flat => {
                    for change in changes {
                        section = section.push(self.build_change_row(change, kind));
                    }
                }
                FileDisplayMode::Tree => {
                    section = self.build_tree_rows(section, changes, kind);
                }
            }
        }

        section.into()
    }

    fn build_tree_rows(
        &self,
        mut section: Column<'a, Message>,
        changes: &[&'a Change],
        kind: SectionKind,
    ) -> Column<'a, Message> {
        // Group files by directory, collecting indices
        let mut dir_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, change) in changes.iter().enumerate() {
            let dir = Path::new(&change.path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
            dir_groups.entry(dir).or_default().push(i);
        }

        for (dir, indices) in &dir_groups {
            if !dir.is_empty() {
                // Directory header
                section = section.push(
                    Container::new(
                        Row::new()
                            .spacing(4)
                            .align_y(Alignment::Center)
                            .push(Space::new().width(Length::Fixed(16.0)))
                            .push(
                                Text::new("📁")
                                    .size(theme::typography::MICRO_SIZE)
                                    .color(theme::darcula::TEXT_DISABLED),
                            )
                            .push(
                                Text::new(dir.clone())
                                    .size(theme::typography::MICRO_SIZE)
                                    .color(theme::darcula::TEXT_DISABLED),
                            ),
                    )
                    .padding([1, 4]),
                );
            }

            for &idx in indices {
                section = section.push(self.build_change_row(changes[idx], kind));
            }
        }

        section
    }

    fn build_change_row(&self, change: &'a Change, kind: SectionKind) -> Element<'a, Message> {
        let status = FileStatus::from(&change.status);
        let staged = matches!(kind, SectionKind::Staged);
        let is_selected = self.selected_path == Some(change.path.as_str());
        let (file_name, parent_path) = split_path(&change.path);

        // Build the file info row
        let mut info_row = Row::new()
            .spacing(theme::spacing::XS)
            .align_y(Alignment::Center)
            .push(
                Container::new(
                    Text::new(status.symbol())
                        .size(theme::typography::CAPTION_SIZE)
                        .color(status.color()),
                )
                .width(Length::Fixed(14.0)),
            );

        match self.display_mode {
            FileDisplayMode::Flat => {
                // Show filename + parent path
                let mut name_col = Column::new().spacing(1).width(Length::Fill).push(
                    Text::new(file_name)
                        .size(theme::typography::CAPTION_SIZE)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
                if !parent_path.is_empty() {
                    name_col = name_col.push(
                        Text::new(parent_path)
                            .size(theme::typography::MICRO_SIZE)
                            .width(Length::Fill)
                            .wrapping(text::Wrapping::WordOrGlyph)
                            .color(theme::darcula::TEXT_SECONDARY),
                    );
                }
                info_row = info_row.push(name_col);
            }
            FileDisplayMode::Tree => {
                // Show just filename (directory is shown as group header)
                info_row = info_row.push(
                    Text::new(file_name)
                        .size(theme::typography::CAPTION_SIZE)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
        }

        // Submodule indicator
        if change.is_submodule {
            if let Some(summary) = &change.submodule_summary {
                info_row = info_row.push(
                    Text::new(format!("⊞ {}", summary))
                        .size(theme::typography::MICRO_SIZE)
                        .color(theme::darcula::TEXT_DISABLED),
                );
            }
        }

        // Stage/unstage action button ("+" or "−")
        let action_button: Element<'a, Message> = if staged {
            if let Some(handler) = &self.on_unstage {
                let msg = handler(change.path.clone());
                Button::new(
                    Text::new("−")
                        .size(theme::typography::CAPTION_SIZE)
                        .color(theme::darcula::STATUS_DELETED),
                )
                .style(theme::button_style(theme::ButtonTone::Ghost))
                .padding([0, 4])
                .on_press(msg)
                .into()
            } else {
                Space::new().width(Length::Fixed(18.0)).into()
            }
        } else if let Some(handler) = &self.on_stage {
            let msg = handler(change.path.clone());
            Button::new(
                Text::new("+")
                    .size(theme::typography::CAPTION_SIZE)
                    .color(theme::darcula::STATUS_ADDED),
            )
            .style(theme::button_style(theme::ButtonTone::Ghost))
            .padding([0, 4])
            .on_press(msg)
            .into()
        } else {
            Space::new().width(Length::Fixed(18.0)).into()
        };

        info_row = info_row.push(action_button);

        // Dim source row while dragging it
        let is_dragged_source = self
            .drag
            .as_ref()
            .map(|d| d.started && d.path == change.path)
            .unwrap_or(false);

        let row_surface = if is_selected {
            Surface::ListSelection
        } else {
            Surface::ListRow
        };

        let item_panel = if is_dragged_source {
            Container::new(
                iced::widget::container(info_row)
                    .padding([2, 4])
                    .width(Length::Fill)
                    .style(move |theme| {
                        let mut style = theme::panel_style(row_surface)(theme);
                        if let Some(bg) = style.background {
                            style.background = Some(match bg {
                                Background::Color(c) => {
                                    Background::Color(Color { a: c.a * 0.4, ..c })
                                }
                                other => other,
                            });
                        }
                        style
                    }),
            )
            .width(Length::Fill)
        } else {
            Container::new(info_row)
                .padding([2, 4])
                .width(Length::Fill)
                .style(theme::panel_style(row_surface))
        };

        let selection: Element<'a, Message> = if let Some(select_message) = self
            .on_select
            .as_ref()
            .map(|handler| handler(change.path.clone()))
        {
            let btn = Button::new(item_panel)
                .width(Length::Fill)
                .style(theme::button_style(theme::ButtonTone::Ghost))
                .on_press(select_message.clone());

            let mut area = mouse_area(Container::new(btn).width(Length::Fill))
                .on_double_click(select_message)
                .interaction(mouse::Interaction::Pointer);

            if let Some(context_message) = self
                .on_context_menu
                .as_ref()
                .map(|handler| handler(change.path.clone()))
            {
                area = area.on_right_press(context_message);
            }

            // Outer mouse_area captures drag-start (fires before Button's on_press per iced
            // outer→inner dispatch; Button on_press still fires for click selection)
            if let Some(pub_kind) = kind.to_pub() {
                if let Some(drag_handler) = self.on_drag_start.as_ref() {
                    let drag_msg = drag_handler(change.path.clone(), pub_kind);
                    let inner: Element<'a, Message> = area.into();
                    let outer = mouse_area(inner).on_press(drag_msg);
                    return Container::new(outer).width(Length::Fill).into();
                }
            }

            area.into()
        } else {
            item_panel.into()
        };

        Container::new(selection).width(Length::Fill).into()
    }

    fn section_badge_tone(kind: SectionKind) -> BadgeTone {
        match kind {
            SectionKind::Staged => BadgeTone::Success,
            SectionKind::Unstaged => BadgeTone::Accent,
            SectionKind::Untracked => BadgeTone::Neutral,
        }
    }
}

fn split_path(path: &str) -> (String, String) {
    let parsed = Path::new(path);
    let file_name = parsed
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string();
    let parent = parsed
        .parent()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_default();

    (file_name, parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ChangeSectionKind;

    #[test]
    fn section_kind_exposes_compact_context_label() {
        assert_eq!(SectionKind::Staged.context_label(), "Staged");
        assert_eq!(SectionKind::Unstaged.context_label(), "Changes");
        assert_eq!(SectionKind::Untracked.context_label(), "New Files");
    }

    #[test]
    fn split_path_returns_file_name_and_parent_directory() {
        assert_eq!(
            split_path("src/ui/main.rs"),
            ("main.rs".to_string(), "src/ui".to_string())
        );
        assert_eq!(
            split_path("Cargo.toml"),
            ("Cargo.toml".to_string(), String::new())
        );
    }

    #[test]
    fn section_header_drop_target_wiring() {
        // SectionKind::Staged maps to ChangeSectionKind::Staged
        assert_eq!(
            SectionKind::Staged.to_pub(),
            Some(ChangeSectionKind::Staged)
        );
        assert_eq!(
            SectionKind::Unstaged.to_pub(),
            Some(ChangeSectionKind::Unstaged)
        );
        assert_eq!(SectionKind::Untracked.to_pub(), None);
    }
}
