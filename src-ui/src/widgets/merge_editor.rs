//! Meld-style 3-column merge editor.
//!
//! Three CodeEditor panes: left (ours) | center (result) | right (theirs)
//! with two link maps and synchronized scrolling.

use crate::theme::{self, BadgeTone, Surface};
use crate::widgets::diff_core;
use crate::widgets::{self, OptionalPush, button};
use git_core::diff::{MergeChunkType, MergeEditorModel, join_lines_preserving_trailing_newline};
use iced::widget::canvas::{self, Canvas};
use iced::widget::{Column, Container, Row, Space, Stack, Text};
use iced::{Alignment, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};
use iced_code_editor::{CodeEditor, Message as EditorMessage};
use std::cell::Cell;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

const ACTION_GUTTER_WIDTH: f32 = 42.0;
const OVERVIEW_WIDTH: f32 = 18.0;
const OVERVIEW_PADDING_Y: f32 = 6.0;
const MIN_EMPTY_BLOCK_HEIGHT: f32 = 6.0;
const MIN_OVERVIEW_BLOCK_HEIGHT: f32 = 2.0;
const ACTION_ROW_HEIGHT: f32 = 20.0;
const ACTION_ROW_PADDING: f32 = 4.0;
const ACTION_BUTTON_GAP: f32 = 4.0;
const HUNK_NAV_SYNC_POINT: f32 = 0.2;
const OVERVIEW_SYNC_POINT: f32 = 0.5;

// ═══════════════════════════════════════
// Public types
// ═══════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePane {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkResolution {
    Ours,
    Theirs,
    Base,
}

#[derive(Debug, Clone)]
pub enum MergeEditorEvent {
    Editor {
        pane: MergePane,
        message: EditorMessage,
    },
    AcceptOurs(usize),
    AcceptTheirs(usize),
    AcceptBase(usize),
    AcceptAllOurs,
    AcceptAllTheirs,
    AutoMerge,
    JumpToOverviewFraction(f32),
    JumpToChunk(usize),
    PrevChunk,
    NextChunk,
    BackToList,
    Apply,
}

// ═══════════════════════════════════════
// Internal data
// ═══════════════════════════════════════

#[derive(Debug, Clone)]
struct PaneDecorations {
    lines: Vec<Option<MergeDecoratedLine>>,
}

#[derive(Debug, Clone)]
struct MergeDecoratedLine {
    chunk_type: MergeChunkType,
    resolved: bool,
}

#[derive(Debug, Clone)]
struct LinkMapBlock {
    chunk_id: usize,
    chunk_type: MergeChunkType,
    resolved: bool,
    left_range: Range<usize>,
    right_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GutterButtonKind {
    Base,
    Ours,
    Theirs,
}

#[derive(Debug, Clone)]
struct GutterActionButton {
    kind: GutterButtonKind,
    event: MergeEditorEvent,
    active: bool,
}

#[derive(Debug, Clone)]
struct OverviewBlock {
    chunk_id: usize,
    chunk_type: MergeChunkType,
    resolved: bool,
    range: Range<usize>,
}

#[derive(Debug, Clone)]
struct ChunkLayout {
    chunk_id: usize,
    chunk_type: MergeChunkType,
    resolved: bool,
    left_range: Range<usize>,
    center_range: Range<usize>,
    right_range: Range<usize>,
}

// ═══════════════════════════════════════
// MergeEditorState
// ═══════════════════════════════════════

pub struct MergeEditorState {
    model: MergeEditorModel,
    resolutions: Vec<Option<ChunkResolution>>,

    left: CodeEditor,
    center: CodeEditor,
    right: CodeEditor,

    left_decorations: Arc<PaneDecorations>,
    center_decorations: Arc<PaneDecorations>,
    right_decorations: Arc<PaneDecorations>,
    left_links: Arc<[LinkMapBlock]>,
    right_links: Arc<[LinkMapBlock]>,
    overview_blocks: Arc<[OverviewBlock]>,
    chunk_layouts: Arc<[ChunkLayout]>,

    left_line_count: usize,
    center_line_count: usize,
    right_line_count: usize,

    current_chunk: Option<usize>,
    suppress_sync: [bool; 3],
}

impl std::fmt::Debug for MergeEditorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeEditorState")
            .field("path", &self.model.path)
            .field("chunks", &self.model.chunks.len())
            .finish()
    }
}

impl Clone for MergeEditorState {
    fn clone(&self) -> Self {
        Self::new(self.model.clone())
    }
}

impl MergeEditorState {
    pub fn new(model: MergeEditorModel) -> Self {
        let chunk_count = model.chunks.len();
        let mut resolutions: Vec<Option<ChunkResolution>> = vec![None; chunk_count];

        // Auto-merge non-conflicting chunks
        for (i, chunk) in model.chunks.iter().enumerate() {
            resolutions[i] = match chunk.chunk_type {
                MergeChunkType::Equal => Some(ChunkResolution::Base),
                MergeChunkType::BothChanged => Some(ChunkResolution::Ours),
                MergeChunkType::OursOnly => Some(ChunkResolution::Ours),
                MergeChunkType::TheirsOnly => Some(ChunkResolution::Theirs),
                MergeChunkType::Conflict => None,
            };
        }

        let (center_text, chunk_layouts) = build_chunk_layouts(&model, &resolutions);
        let path_hint = Some(model.path.as_str());

        let left = build_editor(&model.ours_text, path_hint);
        let center = build_editor(&center_text, path_hint);
        let right = build_editor(&model.theirs_text, path_hint);

        let left_line_count = line_count(&model.ours_text);
        let center_line_count = line_count(&center_text);
        let right_line_count = line_count(&model.theirs_text);

        let (left_decos, center_decos, right_decos) =
            build_all_decorations(&model, &chunk_layouts, center_line_count);
        let (left_links, right_links) = build_all_link_blocks(&chunk_layouts);
        let overview_blocks = build_overview(&chunk_layouts);

        let first_conflict = model
            .chunks
            .iter()
            .position(|c| c.chunk_type == MergeChunkType::Conflict);

        Self {
            model,
            resolutions,
            left,
            center,
            right,
            left_decorations: Arc::new(left_decos),
            center_decorations: Arc::new(center_decos),
            right_decorations: Arc::new(right_decos),
            left_links: Arc::from(left_links),
            right_links: Arc::from(right_links),
            overview_blocks: Arc::from(overview_blocks),
            chunk_layouts: Arc::from(chunk_layouts),
            left_line_count,
            center_line_count,
            right_line_count,
            current_chunk: first_conflict.or(Some(0)),
            suppress_sync: [false; 3],
        }
    }

    pub fn resolved_text(&self) -> String {
        build_chunk_layouts(&self.model, &self.resolutions).0
    }

    pub fn all_resolved(&self) -> bool {
        self.resolutions.iter().all(|r| r.is_some())
    }

    pub fn unresolved_count(&self) -> usize {
        self.resolutions.iter().filter(|r| r.is_none()).count()
    }

    pub fn total_chunks(&self) -> usize {
        self.model.chunks.len()
    }

    pub fn conflict_count(&self) -> usize {
        self.model
            .chunks
            .iter()
            .filter(|c| c.chunk_type == MergeChunkType::Conflict)
            .count()
    }

    pub fn update(&mut self, event: MergeEditorEvent) -> iced::Task<MergeEditorEvent> {
        match event {
            MergeEditorEvent::AcceptOurs(id) => {
                self.set_chunk_resolution(id, ChunkResolution::Ours);
                self.scroll_to_chunk(id, HUNK_NAV_SYNC_POINT)
            }
            MergeEditorEvent::AcceptTheirs(id) => {
                self.set_chunk_resolution(id, ChunkResolution::Theirs);
                self.scroll_to_chunk(id, HUNK_NAV_SYNC_POINT)
            }
            MergeEditorEvent::AcceptBase(id) => {
                self.set_chunk_resolution(id, ChunkResolution::Base);
                self.scroll_to_chunk(id, HUNK_NAV_SYNC_POINT)
            }
            MergeEditorEvent::AcceptAllOurs => {
                for i in 0..self.resolutions.len() {
                    self.resolutions[i] = Some(ChunkResolution::Ours);
                }
                self.rebuild_center();
                iced::Task::none()
            }
            MergeEditorEvent::AcceptAllTheirs => {
                for i in 0..self.resolutions.len() {
                    self.resolutions[i] = Some(ChunkResolution::Theirs);
                }
                self.rebuild_center();
                iced::Task::none()
            }
            MergeEditorEvent::AutoMerge => {
                for (i, chunk) in self.model.chunks.iter().enumerate() {
                    if self.resolutions[i].is_none() {
                        self.resolutions[i] = match chunk.chunk_type {
                            MergeChunkType::Equal => Some(ChunkResolution::Base),
                            MergeChunkType::BothChanged => Some(ChunkResolution::Ours),
                            MergeChunkType::OursOnly => Some(ChunkResolution::Ours),
                            MergeChunkType::TheirsOnly => Some(ChunkResolution::Theirs),
                            MergeChunkType::Conflict => None,
                        };
                    }
                }
                self.rebuild_center();
                self.current_chunk = self
                    .conflict_chunk_ids()
                    .into_iter()
                    .find(|chunk_id| self.resolutions.get(*chunk_id).is_some_and(|r| r.is_none()))
                    .or(self.current_chunk);
                self.scroll_to_current_chunk()
            }
            MergeEditorEvent::JumpToOverviewFraction(fraction) => {
                self.handle_overview_jump(fraction.clamp(0.0, 1.0))
            }
            MergeEditorEvent::JumpToChunk(id) => {
                self.current_chunk = Some(id);
                self.scroll_to_chunk(id, HUNK_NAV_SYNC_POINT)
            }
            MergeEditorEvent::PrevChunk => self.navigate_conflict(false),
            MergeEditorEvent::NextChunk => self.navigate_conflict(true),
            MergeEditorEvent::Editor { pane, message } => self.handle_editor_event(pane, message),
            MergeEditorEvent::BackToList | MergeEditorEvent::Apply => iced::Task::none(),
        }
    }

    fn set_chunk_resolution(&mut self, id: usize, resolution: ChunkResolution) {
        if id < self.resolutions.len() {
            self.resolutions[id] = Some(resolution);
            self.current_chunk = Some(id);
            self.rebuild_center();
        }
    }

    fn rebuild_center(&mut self) {
        let (center_text, chunk_layouts) = build_chunk_layouts(&self.model, &self.resolutions);
        self.center = build_editor(&center_text, Some(&self.model.path));
        self.center_line_count = line_count(&center_text);

        let (left_decos, center_decos, right_decos) =
            build_all_decorations(&self.model, &chunk_layouts, self.center_line_count);
        let (left_links, right_links) = build_all_link_blocks(&chunk_layouts);
        let overview_blocks = build_overview(&chunk_layouts);

        self.left_decorations = Arc::new(left_decos);
        self.center_decorations = Arc::new(center_decos);
        self.right_decorations = Arc::new(right_decos);
        self.left_links = Arc::from(left_links);
        self.right_links = Arc::from(right_links);
        self.overview_blocks = Arc::from(overview_blocks);
        self.chunk_layouts = Arc::from(chunk_layouts);
    }

    fn handle_editor_event(
        &mut self,
        pane: MergePane,
        message: EditorMessage,
    ) -> iced::Task<MergeEditorEvent> {
        let pane_idx = pane_index(pane);
        let should_skip_sync =
            matches!(message, EditorMessage::Scrolled(_)) && self.suppress_sync[pane_idx];
        if should_skip_sync {
            self.suppress_sync[pane_idx] = false;
        }

        // Block mutations on all panes (read-only)
        if is_mutating(&message) {
            return iced::Task::none();
        }

        let local_task = self
            .editor_mut(pane)
            .update(&message)
            .map(move |m| MergeEditorEvent::Editor { pane, message: m });

        let sync_task = match &message {
            EditorMessage::Scrolled(viewport) if !should_skip_sync => {
                self.current_chunk =
                    self.current_chunk_for_scroll(pane, viewport.absolute_offset().y);
                self.synced_scroll_3way(pane, viewport.absolute_offset().y)
            }
            _ => iced::Task::none(),
        };

        iced::Task::batch([local_task, sync_task])
    }

    fn navigate_conflict(&mut self, forward: bool) -> iced::Task<MergeEditorEvent> {
        if let Some(chunk_id) =
            navigate_conflict_target(&self.chunk_layouts, self.current_chunk, forward)
        {
            self.current_chunk = Some(chunk_id);
            self.scroll_to_chunk(chunk_id, HUNK_NAV_SYNC_POINT)
        } else {
            iced::Task::none()
        }
    }

    fn synced_scroll_3way(
        &mut self,
        source_pane: MergePane,
        source_scroll: f32,
    ) -> iced::Task<MergeEditorEvent> {
        let source_editor = self.editor(source_pane);
        let source_lh = source_editor.line_height();
        if source_lh <= 0.0 {
            return iced::Task::none();
        }

        let source_lines = self.pane_line_count(source_pane);
        let source_sp = calc_sync_point(
            source_scroll,
            source_editor.viewport_height(),
            content_height(source_lines, source_lh),
        );
        let source_anchor = anchor_line_for_scroll(
            source_scroll,
            source_sp,
            source_editor.viewport_height(),
            source_lh,
        );

        // Map source anchor to the other two panes
        let targets: [(MergePane, f32); 2] = match source_pane {
            MergePane::Left => {
                let center_anchor = self
                    .map_anchor(source_pane, MergePane::Center, source_anchor)
                    .unwrap_or(scale_anchor(
                        source_anchor,
                        source_lines,
                        self.center_line_count,
                    ));
                let right_anchor = self
                    .map_anchor(MergePane::Center, MergePane::Right, center_anchor)
                    .unwrap_or(scale_anchor(
                        center_anchor,
                        self.center_line_count,
                        self.right_line_count,
                    ));
                [
                    (MergePane::Center, center_anchor),
                    (MergePane::Right, right_anchor),
                ]
            }
            MergePane::Center => {
                let left_anchor = self
                    .map_anchor(source_pane, MergePane::Left, source_anchor)
                    .unwrap_or(scale_anchor(
                        source_anchor,
                        source_lines,
                        self.left_line_count,
                    ));
                let right_anchor = self
                    .map_anchor(source_pane, MergePane::Right, source_anchor)
                    .unwrap_or(scale_anchor(
                        source_anchor,
                        source_lines,
                        self.right_line_count,
                    ));
                [
                    (MergePane::Left, left_anchor),
                    (MergePane::Right, right_anchor),
                ]
            }
            MergePane::Right => {
                let center_anchor = self
                    .map_anchor(source_pane, MergePane::Center, source_anchor)
                    .unwrap_or(scale_anchor(
                        source_anchor,
                        source_lines,
                        self.center_line_count,
                    ));
                let left_anchor = self
                    .map_anchor(MergePane::Center, MergePane::Left, center_anchor)
                    .unwrap_or(scale_anchor(
                        center_anchor,
                        self.center_line_count,
                        self.left_line_count,
                    ));
                [
                    (MergePane::Center, center_anchor),
                    (MergePane::Left, left_anchor),
                ]
            }
        };

        let mut tasks = Vec::new();
        for (target_pane, target_anchor) in targets {
            let target_editor = self.editor(target_pane);
            let target_scroll = scroll_for_anchor_line(
                target_anchor,
                source_sp,
                target_editor.viewport_height(),
                target_editor.line_height(),
                self.pane_line_count(target_pane),
            );
            if (target_editor.viewport_scroll() - target_scroll).abs() > 0.5 {
                self.suppress_sync[pane_index(target_pane)] = true;
                tasks.push(
                    self.editor(target_pane)
                        .scroll_to_offset(None, Some(target_scroll))
                        .map(move |m| MergeEditorEvent::Editor {
                            pane: target_pane,
                            message: m,
                        }),
                );
            }
        }

        iced::Task::batch(tasks)
    }

    fn handle_overview_jump(&mut self, fraction: f32) -> iced::Task<MergeEditorEvent> {
        let source_total_lines = self.pane_line_count(MergePane::Center);
        if source_total_lines == 0 {
            return iced::Task::none();
        }

        let center_anchor = anchor_line_for_fraction(fraction, source_total_lines);
        let left_anchor = self
            .map_anchor(MergePane::Center, MergePane::Left, center_anchor)
            .unwrap_or(scale_anchor(
                center_anchor,
                source_total_lines,
                self.left_line_count,
            ));
        let right_anchor = self
            .map_anchor(MergePane::Center, MergePane::Right, center_anchor)
            .unwrap_or(scale_anchor(
                center_anchor,
                source_total_lines,
                self.right_line_count,
            ));

        self.current_chunk =
            current_chunk_from_anchor(&self.chunk_layouts, MergePane::Center, center_anchor);

        self.scroll_to_anchor_lines(
            left_anchor,
            center_anchor,
            right_anchor,
            OVERVIEW_SYNC_POINT,
        )
    }

    fn scroll_to_current_chunk(&mut self) -> iced::Task<MergeEditorEvent> {
        self.current_chunk
            .map(|chunk_id| self.scroll_to_chunk(chunk_id, HUNK_NAV_SYNC_POINT))
            .unwrap_or_else(iced::Task::none)
    }

    fn scroll_to_chunk(
        &mut self,
        chunk_id: usize,
        sync_point: f32,
    ) -> iced::Task<MergeEditorEvent> {
        let Some(layout) = self
            .chunk_layouts
            .iter()
            .find(|layout| layout.chunk_id == chunk_id)
        else {
            return iced::Task::none();
        };

        self.scroll_to_anchor_lines(
            anchor_line_for_range(&layout.left_range),
            anchor_line_for_range(&layout.center_range),
            anchor_line_for_range(&layout.right_range),
            sync_point,
        )
    }

    fn scroll_to_anchor_lines(
        &mut self,
        left_anchor: f32,
        center_anchor: f32,
        right_anchor: f32,
        sync_point: f32,
    ) -> iced::Task<MergeEditorEvent> {
        let left_scroll = scroll_for_anchor_line(
            left_anchor,
            sync_point,
            self.left.viewport_height(),
            self.left.line_height(),
            self.left_line_count,
        );
        let center_scroll = scroll_for_anchor_line(
            center_anchor,
            sync_point,
            self.center.viewport_height(),
            self.center.line_height(),
            self.center_line_count,
        );
        let right_scroll = scroll_for_anchor_line(
            right_anchor,
            sync_point,
            self.right.viewport_height(),
            self.right.line_height(),
            self.right_line_count,
        );

        let left_task = if (self.left.viewport_scroll() - left_scroll).abs() > 0.5 {
            self.suppress_sync[pane_index(MergePane::Left)] = true;
            self.left
                .scroll_to_offset(None, Some(left_scroll))
                .map(|message| MergeEditorEvent::Editor {
                    pane: MergePane::Left,
                    message,
                })
        } else {
            iced::Task::none()
        };
        let center_task = if (self.center.viewport_scroll() - center_scroll).abs() > 0.5 {
            self.suppress_sync[pane_index(MergePane::Center)] = true;
            self.center
                .scroll_to_offset(None, Some(center_scroll))
                .map(|message| MergeEditorEvent::Editor {
                    pane: MergePane::Center,
                    message,
                })
        } else {
            iced::Task::none()
        };
        let right_task = if (self.right.viewport_scroll() - right_scroll).abs() > 0.5 {
            self.suppress_sync[pane_index(MergePane::Right)] = true;
            self.right
                .scroll_to_offset(None, Some(right_scroll))
                .map(|message| MergeEditorEvent::Editor {
                    pane: MergePane::Right,
                    message,
                })
        } else {
            iced::Task::none()
        };

        iced::Task::batch([left_task, center_task, right_task])
    }

    fn map_anchor(&self, from: MergePane, to: MergePane, anchor: f32) -> Option<f32> {
        map_anchor_between_panes(&self.chunk_layouts, from, to, anchor)
    }

    pub fn view<'b>(&'b self, i18n: &'b crate::i18n::I18n) -> Element<'b, MergeEditorEvent> {
        let unresolved = self.unresolved_count();
        let conflict_total = self.conflict_count();
        let resolved_conflicts = conflict_total - unresolved;
        let current_conflict_position = self.current_conflict_position();
        let has_prev_conflict = current_conflict_position.is_some_and(|position| position > 0);
        let has_next_conflict =
            current_conflict_position.is_some_and(|position| position + 1 < conflict_total);

        // ── Toolbar ──
        let toolbar = Container::new(
            Row::new()
                .spacing(theme::spacing::XS)
                .align_y(Alignment::Center)
                .push(button::compact_ghost(
                    i18n.back_to_list,
                    Some(MergeEditorEvent::BackToList),
                ))
                .push(button::compact_ghost(
                    i18n.prev_conflict,
                    has_prev_conflict.then_some(MergeEditorEvent::PrevChunk),
                ))
                .push(button::compact_ghost(
                    i18n.next_conflict,
                    has_next_conflict.then_some(MergeEditorEvent::NextChunk),
                ))
                .push_maybe(current_conflict_position.map(|position| {
                    widgets::info_chip::<MergeEditorEvent>(
                        format!("{} / {}", position + 1, conflict_total),
                        BadgeTone::Accent,
                    )
                }))
                .push(Space::new().width(Length::Fill))
                .push(button::compact_ghost(
                    i18n.auto_merge,
                    Some(MergeEditorEvent::AutoMerge),
                ))
                .push(button::compact_ghost(
                    i18n.accept_all_ours,
                    Some(MergeEditorEvent::AcceptAllOurs),
                ))
                .push(button::compact_ghost(
                    i18n.accept_all_theirs,
                    Some(MergeEditorEvent::AcceptAllTheirs),
                )),
        )
        .padding([4, 10])
        .width(Length::Fill)
        .style(theme::frame_style(Surface::Toolbar));

        // ── Column headers ──
        let headers = Row::new()
            .spacing(0)
            .push(
                Container::new(
                    Text::new(i18n.ours_version)
                        .size(10)
                        .color(merge_pane_color(MergeChunkType::OursOnly)),
                )
                .padding([2, 8])
                .width(Length::FillPortion(5)),
            )
            .push(Space::new().width(Length::Fixed(ACTION_GUTTER_WIDTH)))
            .push(
                Container::new(
                    Text::new(i18n.merge_result)
                        .size(10)
                        .color(iced::Color::from_rgb(0.42, 0.86, 0.50)),
                )
                .padding([2, 8])
                .width(Length::FillPortion(5)),
            )
            .push(Space::new().width(Length::Fixed(ACTION_GUTTER_WIDTH)))
            .push(
                Container::new(
                    Text::new(i18n.theirs_version)
                        .size(10)
                        .color(merge_pane_color(MergeChunkType::TheirsOnly)),
                )
                .padding([2, 8])
                .width(Length::FillPortion(5)),
            )
            .push(Space::new().width(Length::Fixed(OVERVIEW_WIDTH)));

        // ── 3-pane editor ──
        let editor_row = Row::new()
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .push(
                self.pane_view(MergePane::Left)
                    .width(Length::FillPortion(5)),
            )
            .push(self.action_gutter_view(LinkMapSide::Left))
            .push(diff_core::center_divider())
            .push(
                self.pane_view(MergePane::Center)
                    .width(Length::FillPortion(5)),
            )
            .push(diff_core::center_divider())
            .push(self.action_gutter_view(LinkMapSide::Right))
            .push(
                self.pane_view(MergePane::Right)
                    .width(Length::FillPortion(5)),
            )
            .push(self.overview_view());

        // ── Footer ──
        let footer = Container::new(
            Row::new()
                .spacing(theme::spacing::XS)
                .align_y(Alignment::Center)
                .push(
                    Text::new(format!(
                        "{} conflicts, resolved {}/{}",
                        conflict_total, resolved_conflicts, conflict_total
                    ))
                    .size(11)
                    .color(theme::darcula::TEXT_SECONDARY),
                )
                .push(Space::new().width(Length::Fill))
                .push(widgets::info_chip::<MergeEditorEvent>(
                    &self.model.path,
                    BadgeTone::Neutral,
                ))
                .push(Space::new().width(Length::Fixed(8.0)))
                .push(button::ghost(
                    i18n.cancel,
                    Some(MergeEditorEvent::BackToList),
                ))
                .push(button::primary(
                    i18n.apply,
                    self.all_resolved().then_some(MergeEditorEvent::Apply),
                )),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(theme::frame_style(Surface::Toolbar));

        Container::new(
            Column::new()
                .spacing(0)
                .push(toolbar)
                .push(iced::widget::rule::horizontal(1))
                .push(headers)
                .push(iced::widget::rule::horizontal(1))
                .push(editor_row)
                .push(iced::widget::rule::horizontal(1))
                .push(footer),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel_style(Surface::Panel))
        .into()
    }

    fn pane_view(&self, pane: MergePane) -> Container<'_, MergeEditorEvent> {
        let editor = self.editor(pane);
        let decorations = match pane {
            MergePane::Left => Arc::clone(&self.left_decorations),
            MergePane::Center => Arc::clone(&self.center_decorations),
            MergePane::Right => Arc::clone(&self.right_decorations),
        };

        let active_range = self
            .current_chunk
            .and_then(|id| {
                self.chunk_layouts
                    .iter()
                    .find(|layout| layout.chunk_id == id)
            })
            .map(|layout| match pane {
                MergePane::Left => layout.left_range.clone(),
                MergePane::Right => layout.right_range.clone(),
                MergePane::Center => layout.center_range.clone(),
            });

        let background = Canvas::new(MergeDecorationCanvas {
            decorations,
            viewport_scroll: editor.viewport_scroll(),
            line_height: editor.line_height(),
            viewport_height: editor.viewport_height(),
            gutter_width: editor.gutter_width(),
            active_range,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let editor_view = editor
            .view()
            .map(move |m| MergeEditorEvent::Editor { pane, message: m });

        Container::new(Stack::new().push(background).push(editor_view))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    fn action_gutter_view(&self, side: LinkMapSide) -> Element<'_, MergeEditorEvent> {
        let blocks = match side {
            LinkMapSide::Left => Arc::clone(&self.left_links),
            LinkMapSide::Right => Arc::clone(&self.right_links),
        };

        Canvas::new(MergeActionGutterCanvas {
            side,
            blocks,
            current_chunk: self.current_chunk,
            resolutions: Arc::from(self.resolutions.clone()),
            left_scroll: match side {
                LinkMapSide::Left => self.left.viewport_scroll(),
                LinkMapSide::Right => self.center.viewport_scroll(),
            },
            right_scroll: match side {
                LinkMapSide::Left => self.center.viewport_scroll(),
                LinkMapSide::Right => self.right.viewport_scroll(),
            },
            left_line_height: match side {
                LinkMapSide::Left => self.left.line_height(),
                LinkMapSide::Right => self.center.line_height(),
            },
            right_line_height: match side {
                LinkMapSide::Left => self.center.line_height(),
                LinkMapSide::Right => self.right.line_height(),
            },
        })
        .width(Length::Fixed(ACTION_GUTTER_WIDTH))
        .height(Length::Fill)
        .into()
    }

    fn overview_view(&self) -> Element<'_, MergeEditorEvent> {
        Canvas::new(MergeOverviewCanvas {
            blocks: Arc::clone(&self.overview_blocks),
            current_chunk: self.current_chunk,
            total_lines: self.pane_line_count(MergePane::Center),
            viewport_range: self.overview_viewport_range(),
        })
        .width(Length::Fixed(OVERVIEW_WIDTH))
        .height(Length::Fill)
        .into()
    }

    fn editor(&self, pane: MergePane) -> &CodeEditor {
        match pane {
            MergePane::Left => &self.left,
            MergePane::Center => &self.center,
            MergePane::Right => &self.right,
        }
    }

    fn editor_mut(&mut self, pane: MergePane) -> &mut CodeEditor {
        match pane {
            MergePane::Left => &mut self.left,
            MergePane::Center => &mut self.center,
            MergePane::Right => &mut self.right,
        }
    }

    fn pane_line_count(&self, pane: MergePane) -> usize {
        match pane {
            MergePane::Left => self.left_line_count.max(1),
            MergePane::Center => self.center_line_count.max(1),
            MergePane::Right => self.right_line_count.max(1),
        }
    }

    fn overview_viewport_range(&self) -> Range<f32> {
        let editor = &self.center;
        let total_height = content_height(
            self.pane_line_count(MergePane::Center),
            editor.line_height(),
        )
        .max(1.0);
        let start = (editor.viewport_scroll() / total_height).clamp(0.0, 1.0);
        let end = ((editor.viewport_scroll() + editor.viewport_height()) / total_height)
            .clamp(start, 1.0);
        start..end
    }

    fn current_chunk_for_scroll(&self, pane: MergePane, scroll: f32) -> Option<usize> {
        let editor = self.editor(pane);
        let anchor = anchor_line_for_scroll(
            scroll,
            calc_sync_point(
                scroll,
                editor.viewport_height(),
                content_height(self.pane_line_count(pane), editor.line_height()),
            ),
            editor.viewport_height(),
            editor.line_height().max(1.0),
        );

        current_chunk_from_anchor(&self.chunk_layouts, pane, anchor)
    }

    fn conflict_chunk_ids(&self) -> Vec<usize> {
        self.chunk_layouts
            .iter()
            .filter(|layout| layout.chunk_type == MergeChunkType::Conflict)
            .map(|layout| layout.chunk_id)
            .collect()
    }

    fn current_conflict_position(&self) -> Option<usize> {
        self.current_chunk
            .and_then(|current| conflict_position_for_chunk(&self.chunk_layouts, current))
    }
}

// ═══════════════════════════════════════
// Canvas implementations
// ═══════════════════════════════════════

#[derive(Debug, Clone)]
struct MergeDecorationCanvas {
    decorations: Arc<PaneDecorations>,
    viewport_scroll: f32,
    line_height: f32,
    viewport_height: f32,
    gutter_width: f32,
    active_range: Option<Range<usize>>,
}

#[derive(Debug, Default)]
struct MergeDecoCacheState {
    cache: canvas::Cache<Renderer>,
    key: Cell<Option<(i32, i32, i32)>>,
}

impl<Message> canvas::Program<Message> for MergeDecorationCanvas {
    type State = MergeDecoCacheState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let key = (
            (self.viewport_scroll * 0.5).round() as i32,
            self.active_range
                .as_ref()
                .map(|r| r.start as i32)
                .unwrap_or(-1),
            bounds.width as i32,
        );
        if state.key.get() != Some(key) {
            state.cache.clear();
            state.key.set(Some(key));
        }

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let gutter_width = self.gutter_width.min(bounds.width);
            let code_width = (bounds.width - gutter_width).max(0.0);

            frame.fill_rectangle(
                Point::ORIGIN,
                Size::new(gutter_width, bounds.height),
                diff_core::chunk_gutter_bg(diff_core::ChunkTag::Equal),
            );
            frame.fill_rectangle(
                Point::new(gutter_width, 0.0),
                Size::new(code_width, bounds.height),
                theme::darcula::BG_EDITOR,
            );

            if self.line_height <= 0.0 {
                return;
            }

            let start_line = (self.viewport_scroll / self.line_height).floor().max(0.0) as usize;
            let end_line = ((self.viewport_scroll + self.viewport_height) / self.line_height)
                .ceil()
                .max(0.0) as usize
                + 1;

            for line_index in start_line..end_line.min(self.decorations.lines.len()) {
                let Some(line) = self
                    .decorations
                    .lines
                    .get(line_index)
                    .and_then(|l| l.as_ref())
                else {
                    continue;
                };

                let y = line_index as f32 * self.line_height - self.viewport_scroll;
                let (code_bg, gutter_bg) = merge_block_colors(line.chunk_type, line.resolved);

                frame.fill_rectangle(
                    Point::new(0.0, y),
                    Size::new(gutter_width, self.line_height),
                    gutter_bg,
                );
                frame.fill_rectangle(
                    Point::new(gutter_width, y),
                    Size::new(code_width, self.line_height),
                    code_bg,
                );

                if self
                    .active_range
                    .as_ref()
                    .is_some_and(|r| line_in_range(r, line_index as f32))
                {
                    frame.fill_rectangle(
                        Point::new(0.0, y),
                        Size::new(bounds.width, self.line_height),
                        theme::darcula::SELECTION_BG.scale_alpha(0.12),
                    );
                }
            }
        });

        vec![geometry]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkMapSide {
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct MergeActionGutterCanvas {
    side: LinkMapSide,
    blocks: Arc<[LinkMapBlock]>,
    current_chunk: Option<usize>,
    resolutions: Arc<[Option<ChunkResolution>]>,
    left_scroll: f32,
    right_scroll: f32,
    left_line_height: f32,
    right_line_height: f32,
}

impl canvas::Program<MergeEditorEvent> for MergeActionGutterCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<MergeEditorEvent>> {
        let cursor_position = cursor.position_in(bounds)?;

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let hit_event = gutter_event_at_position(self, bounds, cursor_position)?;
                Some(canvas::Action::publish(hit_event).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(_renderer, bounds.size());
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            theme::darcula::BG_PANEL.scale_alpha(0.92),
        );

        for block in self.blocks.iter() {
            let left = block_visual_bounds(
                &block.left_range,
                self.left_line_height,
                self.left_scroll,
                MIN_EMPTY_BLOCK_HEIGHT,
            );
            let right = block_visual_bounds(
                &block.right_range,
                self.right_line_height,
                self.right_scroll,
                MIN_EMPTY_BLOCK_HEIGHT,
            );

            if !(left.1 < 0.0 || right.1 < 0.0 || left.0 > bounds.height || right.0 > bounds.height)
            {
                let curve = canvas::Path::new(|builder| {
                    builder.move_to(Point::new(0.0, left.0));
                    builder.bezier_curve_to(
                        Point::new(bounds.width * 0.35, left.0),
                        Point::new(bounds.width * 0.65, right.0),
                        Point::new(bounds.width, right.0),
                    );
                    builder.line_to(Point::new(bounds.width, right.1));
                    builder.bezier_curve_to(
                        Point::new(bounds.width * 0.65, right.1),
                        Point::new(bounds.width * 0.35, left.1),
                        Point::new(0.0, left.1),
                    );
                    builder.close();
                });

                let is_active_chunk = self.current_chunk == Some(block.chunk_id);
                frame.fill(
                    &curve,
                    merge_link_fill(block.chunk_type, block.resolved, is_active_chunk),
                );
                frame.stroke(
                    &curve,
                    canvas::Stroke::default()
                        .with_width(if is_active_chunk { 1.3 } else { 0.8 })
                        .with_color(merge_link_stroke(block.chunk_type, block.resolved)),
                );
            }

            let buttons = gutter_buttons_for_block(self.side, block, &self.resolutions);
            if buttons.is_empty() {
                continue;
            }

            let (anchor_scroll, anchor_line_height) = match self.side {
                LinkMapSide::Left => (self.right_scroll, self.right_line_height),
                LinkMapSide::Right => (self.left_scroll, self.left_line_height),
            };
            let row_bounds =
                gutter_row_bounds(self.side, block, anchor_scroll, anchor_line_height, bounds);
            if row_bounds.y + row_bounds.height < 0.0 || row_bounds.y > bounds.height {
                continue;
            }

            let is_active_chunk = self.current_chunk == Some(block.chunk_id);
            let row_path =
                canvas::Path::rectangle(Point::new(row_bounds.x, row_bounds.y), row_bounds.size());
            frame.fill(
                &row_path,
                gutter_row_fill(block.chunk_type, block.resolved, is_active_chunk),
            );
            frame.stroke(
                &row_path,
                canvas::Stroke::default()
                    .with_width(if is_active_chunk { 1.2 } else { 0.8 })
                    .with_color(gutter_row_border(
                        block.chunk_type,
                        block.resolved,
                        is_active_chunk,
                    )),
            );

            for (index, button) in buttons.iter().enumerate() {
                let button_bounds = gutter_button_bounds(row_bounds, index, buttons.len());
                let button_path = canvas::Path::rectangle(
                    Point::new(button_bounds.x, button_bounds.y),
                    button_bounds.size(),
                );
                let (fill, border, text) = gutter_button_style(button.kind, button.active);
                frame.fill(&button_path, fill);
                frame.stroke(
                    &button_path,
                    canvas::Stroke::default().with_width(0.8).with_color(border),
                );
                frame.fill_text(canvas::Text {
                    content: gutter_button_label(button.kind).to_string(),
                    position: Point::new(button_bounds.x + 4.0, button_bounds.y + 2.5),
                    color: text,
                    size: 11.0.into(),
                    font: theme::code_font(),
                    ..canvas::Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor
            .position_in(bounds)
            .and_then(|position| gutter_event_at_position(self, bounds, position))
            .is_some()
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[derive(Debug, Clone)]
struct MergeOverviewCanvas {
    blocks: Arc<[OverviewBlock]>,
    current_chunk: Option<usize>,
    total_lines: usize,
    viewport_range: Range<f32>,
}

#[derive(Debug, Default)]
struct MergeOverviewCacheState {
    dragging: bool,
    cache: canvas::Cache<Renderer>,
    key: Cell<Option<(usize, i32, i32, Option<usize>)>>,
}

impl canvas::Program<MergeEditorEvent> for MergeOverviewCanvas {
    type State = MergeOverviewCacheState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<MergeEditorEvent>> {
        let cursor_y = cursor.position_in(bounds).map(|position| position.y);

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let y = cursor_y?;
                state.dragging = true;
                Some(
                    canvas::Action::publish(MergeEditorEvent::JumpToOverviewFraction(
                        overview_fraction(y, bounds.height),
                    ))
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let y = cursor_y?;
                Some(
                    canvas::Action::publish(MergeEditorEvent::JumpToOverviewFraction(
                        overview_fraction(y, bounds.height),
                    ))
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging =>
            {
                state.dragging = false;
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) if state.dragging => {
                state.dragging = false;
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let drawable_height = (bounds.height - OVERVIEW_PADDING_Y * 2.0).max(1.0);
        let track_x = 2.0;
        let track_width = (bounds.width - 4.0).max(1.0);

        let key = (
            self.blocks.len(),
            (self.viewport_range.start * 100.0).round() as i32,
            (self.viewport_range.end * 100.0).round() as i32,
            self.current_chunk,
        );
        if state.key.get() != Some(key) {
            state.cache.clear();
            state.key.set(Some(key));
        }

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            frame.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                theme::darcula::BG_PANEL.scale_alpha(0.94),
            );

            let total = self.total_lines.max(1) as f32;
            for block in self.blocks.iter() {
                let y = block.range.start as f32 / total * drawable_height;
                let height = (((block.range.end.max(block.range.start + 1) - block.range.start)
                    as f32
                    / total)
                    * drawable_height)
                    .max(MIN_OVERVIEW_BLOCK_HEIGHT);
                let color = merge_overview_fill(block.chunk_type, block.resolved);

                frame.fill_rectangle(
                    Point::new(track_x, y + OVERVIEW_PADDING_Y),
                    Size::new(track_width, height),
                    color,
                );

                if self.current_chunk == Some(block.chunk_id) {
                    let path = canvas::Path::rectangle(
                        Point::new(track_x - 0.5, y + OVERVIEW_PADDING_Y - 0.5),
                        Size::new(track_width + 1.0, height + 1.0),
                    );
                    frame.stroke(
                        &path,
                        canvas::Stroke::default()
                            .with_width(1.0)
                            .with_color(theme::darcula::ACCENT),
                    );
                }
            }

            // Viewport indicator
            let viewport_y = self.viewport_range.start * drawable_height + OVERVIEW_PADDING_Y;
            let viewport_height =
                ((self.viewport_range.end - self.viewport_range.start) * drawable_height).max(8.0);
            let vp_rect = canvas::Path::rectangle(
                Point::new(0.5, viewport_y),
                Size::new(bounds.width - 1.0, viewport_height),
            );
            frame.fill(&vp_rect, theme::darcula::TEXT_PRIMARY.scale_alpha(0.08));
            frame.stroke(
                &vp_rect,
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(theme::darcula::TEXT_SECONDARY.scale_alpha(0.35)),
            );
        });

        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_in(bounds).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

// ═══════════════════════════════════════
// Builder functions
// ═══════════════════════════════════════

fn build_chunk_layouts(
    model: &MergeEditorModel,
    resolutions: &[Option<ChunkResolution>],
) -> (String, Vec<ChunkLayout>) {
    let mut lines: Vec<String> = Vec::new();
    let mut layouts = Vec::with_capacity(model.chunks.len());
    let mut center_cursor = 0usize;

    for (i, chunk) in model.chunks.iter().enumerate() {
        let resolution = resolutions.get(i).copied().flatten();
        let resolved_lines = match resolution {
            Some(ChunkResolution::Ours) => chunk.lines_ours.clone(),
            Some(ChunkResolution::Theirs) => chunk.lines_theirs.clone(),
            Some(ChunkResolution::Base) => chunk.lines_base.clone(),
            None => {
                // Unresolved: show conflict markers
                let mut conflict_lines =
                    Vec::with_capacity(chunk.lines_ours.len() + chunk.lines_theirs.len() + 3);
                conflict_lines.push("<<<<<<< ours".to_string());
                conflict_lines.extend(chunk.lines_ours.iter().cloned());
                conflict_lines.push("=======".to_string());
                conflict_lines.extend(chunk.lines_theirs.iter().cloned());
                conflict_lines.push(">>>>>>> theirs".to_string());
                conflict_lines
            }
        };

        let center_range = center_cursor..(center_cursor + resolved_lines.len());
        lines.extend(resolved_lines);
        center_cursor = center_range.end;

        layouts.push(ChunkLayout {
            chunk_id: chunk.id,
            chunk_type: chunk.chunk_type,
            resolved: resolution.is_some(),
            left_range: chunk.ours_range.clone(),
            center_range,
            right_range: chunk.theirs_range.clone(),
        });
    }

    (
        join_lines_preserving_trailing_newline(
            lines,
            &[&model.base_text, &model.ours_text, &model.theirs_text],
        ),
        layouts,
    )
}

fn build_all_decorations(
    model: &MergeEditorModel,
    layouts: &[ChunkLayout],
    center_line_count: usize,
) -> (PaneDecorations, PaneDecorations, PaneDecorations) {
    let ours_lines = line_count(&model.ours_text);
    let theirs_lines = line_count(&model.theirs_text);

    let mut left = PaneDecorations {
        lines: vec![None; ours_lines],
    };
    let mut right = PaneDecorations {
        lines: vec![None; theirs_lines],
    };
    let mut center = PaneDecorations {
        lines: vec![None; center_line_count],
    };

    for layout in layouts {
        if layout.chunk_type == MergeChunkType::Equal {
            continue;
        }

        for line_idx in layout.left_range.clone() {
            if let Some(slot) = left.lines.get_mut(line_idx) {
                *slot = Some(MergeDecoratedLine {
                    chunk_type: layout.chunk_type,
                    resolved: layout.resolved,
                });
            }
        }

        for line_idx in layout.right_range.clone() {
            if let Some(slot) = right.lines.get_mut(line_idx) {
                *slot = Some(MergeDecoratedLine {
                    chunk_type: layout.chunk_type,
                    resolved: layout.resolved,
                });
            }
        }

        for line_idx in layout.center_range.clone() {
            if let Some(slot) = center.lines.get_mut(line_idx) {
                *slot = Some(MergeDecoratedLine {
                    chunk_type: layout.chunk_type,
                    resolved: layout.resolved,
                });
            }
        }
    }

    (left, center, right)
}

fn build_all_link_blocks(layouts: &[ChunkLayout]) -> (Vec<LinkMapBlock>, Vec<LinkMapBlock>) {
    let mut left_blocks = Vec::new();
    let mut right_blocks = Vec::new();

    for layout in layouts {
        if layout.chunk_type == MergeChunkType::Equal {
            continue;
        }

        left_blocks.push(LinkMapBlock {
            chunk_id: layout.chunk_id,
            chunk_type: layout.chunk_type,
            resolved: layout.resolved,
            left_range: layout.left_range.clone(),
            right_range: layout.center_range.clone(),
        });

        right_blocks.push(LinkMapBlock {
            chunk_id: layout.chunk_id,
            chunk_type: layout.chunk_type,
            resolved: layout.resolved,
            left_range: layout.center_range.clone(),
            right_range: layout.right_range.clone(),
        });
    }

    (left_blocks, right_blocks)
}

fn build_overview(layouts: &[ChunkLayout]) -> Vec<OverviewBlock> {
    layouts
        .iter()
        .filter(|layout| layout.chunk_type != MergeChunkType::Equal)
        .map(|layout| OverviewBlock {
            chunk_id: layout.chunk_id,
            chunk_type: layout.chunk_type,
            resolved: layout.resolved,
            range: layout.center_range.clone(),
        })
        .collect()
}

#[allow(dead_code)]
fn assemble_center_text(
    model: &MergeEditorModel,
    resolutions: &[Option<ChunkResolution>],
) -> String {
    build_chunk_layouts(model, resolutions).0
}

// ═══════════════════════════════════════
// Helpers
// ═══════════════════════════════════════

const DEFAULT_MERGE_FONT_SIZE: f32 = 13.0;

fn build_editor(content: &str, path_hint: Option<&str>) -> CodeEditor {
    build_editor_sized(content, path_hint, DEFAULT_MERGE_FONT_SIZE)
}

fn build_editor_sized(content: &str, path_hint: Option<&str>, font_size: f32) -> CodeEditor {
    let syntax = path_hint
        .and_then(|p| Path::new(p).extension())
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_else(|| "txt".to_string());
    let mut editor = CodeEditor::new(content, syntax.as_str());
    editor.set_font(theme::code_font());
    editor.set_font_size(font_size, true);
    editor.set_wrap_enabled(false);
    editor.set_line_numbers_enabled(true);
    editor.set_search_replace_enabled(false);
    editor.set_theme(iced_code_editor::theme::Style {
        background: iced::Color::TRANSPARENT,
        text_color: theme::darcula::TEXT_PRIMARY,
        gutter_background: iced::Color::TRANSPARENT,
        gutter_border: iced::Color::TRANSPARENT,
        line_number_color: theme::darcula::TEXT_DISABLED,
        scrollbar_background: theme::darcula::BG_PANEL.scale_alpha(0.65),
        scroller_color: theme::darcula::BORDER.scale_alpha(0.95),
        current_line_highlight: iced::Color::TRANSPARENT,
    });
    editor
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        1
    } else {
        text.lines().count().max(1)
    }
}

fn pane_index(pane: MergePane) -> usize {
    match pane {
        MergePane::Left => 0,
        MergePane::Center => 1,
        MergePane::Right => 2,
    }
}

fn is_mutating(message: &EditorMessage) -> bool {
    matches!(
        message,
        EditorMessage::CharacterInput(_)
            | EditorMessage::Backspace
            | EditorMessage::Delete
            | EditorMessage::Enter
            | EditorMessage::Tab
            | EditorMessage::Paste(_)
            | EditorMessage::DeleteSelection
            | EditorMessage::Undo
            | EditorMessage::Redo
            | EditorMessage::OpenSearch
            | EditorMessage::OpenSearchReplace
            | EditorMessage::CloseSearch
            | EditorMessage::SearchQueryChanged(_)
            | EditorMessage::ReplaceQueryChanged(_)
            | EditorMessage::ToggleCaseSensitive
            | EditorMessage::FindNext
            | EditorMessage::FindPrevious
            | EditorMessage::ReplaceNext
            | EditorMessage::ReplaceAll
            | EditorMessage::SearchDialogTab
            | EditorMessage::SearchDialogShiftTab
            | EditorMessage::ImeOpened
            | EditorMessage::ImePreedit(_, _)
            | EditorMessage::ImeCommit(_)
            | EditorMessage::ImeClosed
    )
}

// ── Colors ──

fn merge_pane_color(chunk_type: MergeChunkType) -> iced::Color {
    match chunk_type {
        MergeChunkType::Equal => theme::darcula::TEXT_SECONDARY,
        MergeChunkType::BothChanged => iced::Color::from_rgb(0.36, 0.72, 0.58),
        MergeChunkType::OursOnly => iced::Color::from_rgb(0.40, 0.65, 0.95),
        MergeChunkType::TheirsOnly => iced::Color::from_rgb(0.92, 0.44, 0.44),
        MergeChunkType::Conflict => iced::Color::from_rgb(0.75, 0.50, 0.90),
    }
}

fn merge_block_colors(chunk_type: MergeChunkType, resolved: bool) -> (iced::Color, iced::Color) {
    let alpha = if resolved { 0.10 } else { 0.18 };
    let gutter_alpha = if resolved { 0.15 } else { 0.25 };

    match chunk_type {
        MergeChunkType::Equal => (iced::Color::TRANSPARENT, iced::Color::TRANSPARENT),
        MergeChunkType::BothChanged => (
            iced::Color::from_rgba(0.36, 0.72, 0.58, alpha),
            iced::Color::from_rgba(0.36, 0.72, 0.58, gutter_alpha),
        ),
        MergeChunkType::OursOnly => (
            iced::Color::from_rgba(0.30, 0.55, 0.85, alpha),
            iced::Color::from_rgba(0.30, 0.55, 0.85, gutter_alpha),
        ),
        MergeChunkType::TheirsOnly => (
            iced::Color::from_rgba(0.92, 0.44, 0.44, alpha),
            iced::Color::from_rgba(0.92, 0.44, 0.44, gutter_alpha),
        ),
        MergeChunkType::Conflict => {
            if resolved {
                (
                    iced::Color::from_rgba(0.42, 0.86, 0.50, 0.12),
                    iced::Color::from_rgba(0.42, 0.86, 0.50, 0.18),
                )
            } else {
                (
                    iced::Color::from_rgba(0.75, 0.50, 0.90, alpha),
                    iced::Color::from_rgba(0.75, 0.50, 0.90, gutter_alpha),
                )
            }
        }
    }
}

fn merge_link_fill(chunk_type: MergeChunkType, resolved: bool, active: bool) -> iced::Color {
    let alpha = if active { 0.36 } else { 0.22 };
    if resolved {
        return iced::Color::from_rgba(0.42, 0.86, 0.50, alpha * 0.6);
    }
    match chunk_type {
        MergeChunkType::Equal => iced::Color::TRANSPARENT,
        MergeChunkType::BothChanged => iced::Color::from_rgba(0.36, 0.72, 0.58, alpha),
        MergeChunkType::OursOnly => iced::Color::from_rgba(0.30, 0.55, 0.85, alpha),
        MergeChunkType::TheirsOnly => iced::Color::from_rgba(0.92, 0.44, 0.44, alpha),
        MergeChunkType::Conflict => iced::Color::from_rgba(0.75, 0.50, 0.90, alpha),
    }
}

fn merge_link_stroke(chunk_type: MergeChunkType, resolved: bool) -> iced::Color {
    if resolved {
        return iced::Color::from_rgba(0.42, 0.86, 0.50, 0.50);
    }
    match chunk_type {
        MergeChunkType::Equal => iced::Color::TRANSPARENT,
        MergeChunkType::BothChanged => iced::Color::from_rgba(0.36, 0.72, 0.58, 0.72),
        MergeChunkType::OursOnly => iced::Color::from_rgba(0.30, 0.55, 0.85, 0.70),
        MergeChunkType::TheirsOnly => iced::Color::from_rgba(0.92, 0.44, 0.44, 0.70),
        MergeChunkType::Conflict => iced::Color::from_rgba(0.75, 0.50, 0.90, 0.75),
    }
}

fn merge_overview_fill(chunk_type: MergeChunkType, resolved: bool) -> iced::Color {
    if resolved {
        return iced::Color::from_rgba(0.42, 0.86, 0.50, 0.55);
    }
    match chunk_type {
        MergeChunkType::Equal => iced::Color::TRANSPARENT,
        MergeChunkType::BothChanged => iced::Color::from_rgba(0.36, 0.72, 0.58, 0.68),
        MergeChunkType::OursOnly => iced::Color::from_rgba(0.30, 0.55, 0.85, 0.65),
        MergeChunkType::TheirsOnly => iced::Color::from_rgba(0.92, 0.44, 0.44, 0.65),
        MergeChunkType::Conflict => iced::Color::from_rgba(0.75, 0.50, 0.90, 0.75),
    }
}

fn blend(base: iced::Color, overlay: iced::Color, amount: f32) -> iced::Color {
    let amount = amount.clamp(0.0, 1.0);
    iced::Color {
        r: (base.r * (1.0 - amount)) + (overlay.r * amount),
        g: (base.g * (1.0 - amount)) + (overlay.g * amount),
        b: (base.b * (1.0 - amount)) + (overlay.b * amount),
        a: (base.a * (1.0 - amount)) + (overlay.a * amount),
    }
}

// ── Scroll math ──

fn calc_sync_point(viewport_scroll: f32, viewport_height: f32, content_height: f32) -> f32 {
    if viewport_height <= 0.0 || content_height <= viewport_height {
        return 0.5;
    }
    let half_screen = viewport_height / 2.0;
    if half_screen <= 0.0 {
        return 0.5;
    }
    let first_scale = viewport_scroll / half_screen;
    let bottom_val = content_height - 1.5 * viewport_height;
    let last_scale = (viewport_scroll - bottom_val) / half_screen;
    (0.5 * first_scale.min(1.0) + 0.5 * last_scale.max(0.0)).clamp(0.0, 1.0)
}

fn anchor_line_for_scroll(
    viewport_scroll: f32,
    sync_point: f32,
    viewport_height: f32,
    line_height: f32,
) -> f32 {
    if line_height <= 0.0 {
        return 0.0;
    }
    (viewport_scroll + viewport_height * sync_point) / line_height
}

fn scroll_for_anchor_line(
    anchor_line: f32,
    sync_point: f32,
    viewport_height: f32,
    line_height: f32,
    total_lines: usize,
) -> f32 {
    if line_height <= 0.0 {
        return 0.0;
    }
    let ch = content_height(total_lines, line_height);
    let max_scroll = (ch - viewport_height).max(0.0);
    let anchor_y = anchor_line.max(0.0) * line_height;
    (anchor_y - viewport_height * sync_point).clamp(0.0, max_scroll)
}

fn scale_anchor(anchor: f32, source_total: usize, target_total: usize) -> f32 {
    if source_total <= 1 || target_total <= 1 {
        return 0.0;
    }
    let ratio = anchor / (source_total.saturating_sub(1)) as f32;
    ratio.clamp(0.0, 1.0) * (target_total.saturating_sub(1)) as f32
}

fn anchor_line_for_fraction(fraction: f32, total_lines: usize) -> f32 {
    if total_lines <= 1 {
        0.0
    } else {
        (fraction.clamp(0.0, 1.0) * (total_lines.saturating_sub(1)) as f32)
            .clamp(0.0, (total_lines.saturating_sub(1)) as f32)
    }
}

fn anchor_line_for_range(range: &Range<usize>) -> f32 {
    range.start as f32
}

fn content_height(total_lines: usize, line_height: f32) -> f32 {
    total_lines.max(1) as f32 * line_height.max(1.0)
}

fn line_in_range(range: &Range<usize>, line: f32) -> bool {
    if range.is_empty() {
        (line - range.start as f32).abs() < f32::EPSILON
    } else {
        line >= range.start as f32 && line < range.end as f32
    }
}

fn interpolate_in_range(anchor: f32, source: &Range<usize>, target: &Range<usize>) -> f32 {
    if source.is_empty() {
        return target.start as f32;
    }
    if target.is_empty() {
        return target.start as f32;
    }
    let source_len = (source.end - source.start) as f32;
    let target_len = (target.end - target.start) as f32;
    let offset = (anchor - source.start as f32).clamp(0.0, source_len);
    target.start as f32 + offset / source_len.max(1.0) * target_len
}

fn interpolate_between_ranges(
    anchor: f32,
    previous_source: &Range<usize>,
    next_source: &Range<usize>,
    previous_target: &Range<usize>,
    next_target: &Range<usize>,
) -> f32 {
    let source_start = previous_source.end as f32;
    let source_end = next_source.start as f32;
    let target_start = previous_target.end as f32;
    let target_end = next_target.start as f32;

    if (source_end - source_start).abs() <= f32::EPSILON {
        return target_start;
    }

    let fraction = ((anchor - source_start) / (source_end - source_start)).clamp(0.0, 1.0);
    target_start + fraction * (target_end - target_start)
}

fn block_visual_bounds(
    range: &Range<usize>,
    line_height: f32,
    scroll: f32,
    minimum_height: f32,
) -> (f32, f32) {
    let start = range.start as f32 * line_height - scroll;
    let height = if range.is_empty() {
        minimum_height
    } else {
        ((range.end - range.start) as f32 * line_height).max(minimum_height)
    };
    (start, start + height)
}

fn gutter_buttons_for_block(
    side: LinkMapSide,
    block: &LinkMapBlock,
    resolutions: &[Option<ChunkResolution>],
) -> Vec<GutterActionButton> {
    let resolution = resolutions.get(block.chunk_id).copied().flatten();

    let base_button = GutterActionButton {
        kind: GutterButtonKind::Base,
        event: MergeEditorEvent::AcceptBase(block.chunk_id),
        active: resolution == Some(ChunkResolution::Base),
    };
    let ours_button = GutterActionButton {
        kind: GutterButtonKind::Ours,
        event: MergeEditorEvent::AcceptOurs(block.chunk_id),
        active: resolution == Some(ChunkResolution::Ours),
    };
    let theirs_button = GutterActionButton {
        kind: GutterButtonKind::Theirs,
        event: MergeEditorEvent::AcceptTheirs(block.chunk_id),
        active: resolution == Some(ChunkResolution::Theirs),
    };

    match (side, block.chunk_type) {
        (_, MergeChunkType::Equal) => Vec::new(),
        (LinkMapSide::Left, MergeChunkType::OursOnly | MergeChunkType::Conflict) => {
            vec![base_button, ours_button]
        }
        (LinkMapSide::Left, MergeChunkType::BothChanged) => vec![base_button, ours_button],
        (LinkMapSide::Right, MergeChunkType::TheirsOnly | MergeChunkType::Conflict) => {
            vec![theirs_button, base_button]
        }
        _ => Vec::new(),
    }
}

fn gutter_anchor_range(side: LinkMapSide, block: &LinkMapBlock) -> &Range<usize> {
    match side {
        LinkMapSide::Left if !block.right_range.is_empty() => &block.right_range,
        LinkMapSide::Right if !block.left_range.is_empty() => &block.left_range,
        LinkMapSide::Left => &block.left_range,
        LinkMapSide::Right => &block.right_range,
    }
}

fn gutter_row_bounds(
    side: LinkMapSide,
    block: &LinkMapBlock,
    viewport_scroll: f32,
    line_height: f32,
    bounds: Rectangle,
) -> Rectangle {
    let (top, bottom) = block_visual_bounds(
        gutter_anchor_range(side, block),
        line_height,
        viewport_scroll,
        ACTION_ROW_HEIGHT,
    );
    let desired_y = top + ACTION_ROW_PADDING;
    let y = desired_y.clamp(2.0, (bounds.height - ACTION_ROW_HEIGHT - 2.0).max(2.0));
    let width = (bounds.width - ACTION_ROW_PADDING * 2.0).max(1.0);
    let height = ACTION_ROW_HEIGHT.min((bottom - top).max(ACTION_ROW_HEIGHT));

    Rectangle {
        x: ACTION_ROW_PADDING,
        y,
        width,
        height,
    }
}

fn gutter_button_bounds(row_bounds: Rectangle, index: usize, button_count: usize) -> Rectangle {
    let total_gap = ACTION_BUTTON_GAP * button_count.saturating_sub(1) as f32;
    let button_width = ((row_bounds.width - total_gap) / button_count.max(1) as f32).max(10.0);
    let x = row_bounds.x + index as f32 * (button_width + ACTION_BUTTON_GAP);

    Rectangle {
        x,
        y: row_bounds.y,
        width: button_width,
        height: row_bounds.height,
    }
}

fn gutter_row_fill(chunk_type: MergeChunkType, resolved: bool, active_chunk: bool) -> iced::Color {
    let base = merge_link_fill(chunk_type, resolved, active_chunk);
    if active_chunk {
        blend(base, theme::darcula::ACCENT, 0.08)
    } else {
        base
    }
}

fn gutter_row_border(
    chunk_type: MergeChunkType,
    resolved: bool,
    active_chunk: bool,
) -> iced::Color {
    let base = merge_link_stroke(chunk_type, resolved);
    if active_chunk {
        blend(base, theme::darcula::TEXT_PRIMARY, 0.16)
    } else {
        base
    }
}

fn gutter_button_style(
    kind: GutterButtonKind,
    active: bool,
) -> (iced::Color, iced::Color, iced::Color) {
    let (fill, border) = match kind {
        GutterButtonKind::Base => (
            blend(
                theme::darcula::BG_RAISED,
                theme::darcula::WARNING,
                if active { 0.24 } else { 0.10 },
            ),
            theme::darcula::WARNING.scale_alpha(if active { 0.78 } else { 0.42 }),
        ),
        GutterButtonKind::Ours => (
            blend(
                theme::darcula::BG_RAISED,
                theme::darcula::ACCENT,
                if active { 0.28 } else { 0.12 },
            ),
            theme::darcula::ACCENT.scale_alpha(if active { 0.82 } else { 0.48 }),
        ),
        GutterButtonKind::Theirs => (
            blend(
                theme::darcula::BG_RAISED,
                theme::darcula::DANGER,
                if active { 0.28 } else { 0.12 },
            ),
            theme::darcula::DANGER.scale_alpha(if active { 0.82 } else { 0.48 }),
        ),
    };

    (fill, border, theme::darcula::TEXT_PRIMARY)
}

fn gutter_button_label(kind: GutterButtonKind) -> &'static str {
    match kind {
        GutterButtonKind::Base => "x",
        GutterButtonKind::Ours => ">>",
        GutterButtonKind::Theirs => "<<",
    }
}

fn point_in_rect(point: Point, rect: Rectangle) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

fn gutter_event_at_position(
    canvas: &MergeActionGutterCanvas,
    bounds: Rectangle,
    point: Point,
) -> Option<MergeEditorEvent> {
    for block in canvas.blocks.iter() {
        let buttons = gutter_buttons_for_block(canvas.side, block, &canvas.resolutions);
        if buttons.is_empty() {
            continue;
        }

        let (anchor_scroll, anchor_line_height) = match canvas.side {
            LinkMapSide::Left => (canvas.right_scroll, canvas.right_line_height),
            LinkMapSide::Right => (canvas.left_scroll, canvas.left_line_height),
        };
        let row_bounds = gutter_row_bounds(
            canvas.side,
            block,
            anchor_scroll,
            anchor_line_height,
            bounds,
        );
        if !point_in_rect(point, row_bounds) {
            continue;
        }

        for (index, button) in buttons.iter().enumerate() {
            if point_in_rect(
                point,
                gutter_button_bounds(row_bounds, index, buttons.len()),
            ) {
                return Some(button.event.clone());
            }
        }

        return Some(MergeEditorEvent::JumpToChunk(block.chunk_id));
    }

    None
}

fn chunk_range_for_pane(layout: &ChunkLayout, pane: MergePane) -> Range<usize> {
    match pane {
        MergePane::Left if !layout.left_range.is_empty() => layout.left_range.clone(),
        MergePane::Center if !layout.center_range.is_empty() => layout.center_range.clone(),
        MergePane::Right if !layout.right_range.is_empty() => layout.right_range.clone(),
        MergePane::Left if !layout.center_range.is_empty() => layout.center_range.clone(),
        MergePane::Right if !layout.center_range.is_empty() => layout.center_range.clone(),
        MergePane::Center if !layout.left_range.is_empty() => layout.left_range.clone(),
        MergePane::Center if !layout.right_range.is_empty() => layout.right_range.clone(),
        MergePane::Left => layout.right_range.clone(),
        MergePane::Right => layout.left_range.clone(),
        MergePane::Center => layout.center_range.clone(),
    }
}

fn chunk_index(layouts: &[ChunkLayout], chunk_id: usize) -> Option<usize> {
    layouts
        .iter()
        .position(|layout| layout.chunk_id == chunk_id)
}

fn current_chunk_from_anchor(
    layouts: &[ChunkLayout],
    pane: MergePane,
    anchor: f32,
) -> Option<usize> {
    let mut last_changed = None;

    for layout in layouts
        .iter()
        .filter(|layout| layout.chunk_type != MergeChunkType::Equal)
    {
        last_changed = Some(layout.chunk_id);
        let range = chunk_range_for_pane(layout, pane);
        let end = range.end.max(range.start + 1) as f32;
        if anchor < end {
            return Some(layout.chunk_id);
        }
    }

    last_changed
}

fn conflict_position_for_chunk(layouts: &[ChunkLayout], chunk_id: usize) -> Option<usize> {
    let conflicts: Vec<(usize, usize)> = layouts
        .iter()
        .enumerate()
        .filter(|(_, layout)| layout.chunk_type == MergeChunkType::Conflict)
        .map(|(index, layout)| (index, layout.chunk_id))
        .collect();
    if conflicts.is_empty() {
        return None;
    }

    let current_index = chunk_index(layouts, chunk_id)?;
    conflicts
        .iter()
        .position(|(_, conflict_id)| *conflict_id == chunk_id)
        .or_else(|| {
            conflicts
                .iter()
                .position(|(index, _)| *index >= current_index)
        })
        .or(Some(conflicts.len().saturating_sub(1)))
}

fn navigate_conflict_target(
    layouts: &[ChunkLayout],
    current_chunk: Option<usize>,
    forward: bool,
) -> Option<usize> {
    let conflicts: Vec<(usize, usize)> = layouts
        .iter()
        .enumerate()
        .filter(|(_, layout)| layout.chunk_type == MergeChunkType::Conflict)
        .map(|(index, layout)| (index, layout.chunk_id))
        .collect();
    if conflicts.is_empty() {
        return None;
    }

    let current_index = current_chunk.and_then(|chunk_id| chunk_index(layouts, chunk_id));
    let selected = match current_index {
        Some(index) if forward => conflicts
            .iter()
            .find(|(conflict_index, _)| *conflict_index > index)
            .or_else(|| {
                conflicts
                    .iter()
                    .find(|(conflict_index, _)| *conflict_index == index)
            })
            .or_else(|| conflicts.last()),
        Some(index) => conflicts
            .iter()
            .rev()
            .find(|(conflict_index, _)| *conflict_index < index)
            .or_else(|| {
                conflicts
                    .iter()
                    .find(|(conflict_index, _)| *conflict_index == index)
            })
            .or_else(|| conflicts.first()),
        None if forward => conflicts.first(),
        None => conflicts.last(),
    }?;

    Some(selected.1)
}

fn map_anchor_between_panes(
    layouts: &[ChunkLayout],
    from: MergePane,
    to: MergePane,
    anchor: f32,
) -> Option<f32> {
    let mut previous: Option<(Range<usize>, Range<usize>)> = None;

    for layout in layouts
        .iter()
        .filter(|layout| layout.chunk_type != MergeChunkType::Equal)
    {
        let source_range = chunk_range_for_pane(layout, from);
        let target_range = chunk_range_for_pane(layout, to);

        if anchor < source_range.start as f32 {
            return Some(match previous {
                Some((previous_source, previous_target)) => interpolate_between_ranges(
                    anchor,
                    &previous_source,
                    &source_range,
                    &previous_target,
                    &target_range,
                ),
                None => anchor.max(0.0),
            });
        }

        if line_in_range(&source_range, anchor) {
            return Some(interpolate_in_range(anchor, &source_range, &target_range));
        }

        previous = Some((source_range, target_range));
    }

    previous.map(|(previous_source, previous_target)| {
        previous_target.end as f32 + (anchor - previous_source.end as f32).max(0.0)
    })
}

fn overview_fraction(y: f32, height: f32) -> f32 {
    ((y - OVERVIEW_PADDING_Y) / (height - OVERVIEW_PADDING_Y * 2.0).max(1.0)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_layouts() -> Vec<ChunkLayout> {
        vec![
            ChunkLayout {
                chunk_id: 0,
                chunk_type: MergeChunkType::Equal,
                resolved: true,
                left_range: 0..5,
                center_range: 0..5,
                right_range: 0..5,
            },
            ChunkLayout {
                chunk_id: 1,
                chunk_type: MergeChunkType::Conflict,
                resolved: false,
                left_range: 5..7,
                center_range: 5..10,
                right_range: 5..8,
            },
            ChunkLayout {
                chunk_id: 2,
                chunk_type: MergeChunkType::OursOnly,
                resolved: true,
                left_range: 7..9,
                center_range: 10..12,
                right_range: 8..8,
            },
            ChunkLayout {
                chunk_id: 3,
                chunk_type: MergeChunkType::Conflict,
                resolved: false,
                left_range: 12..13,
                center_range: 15..18,
                right_range: 10..12,
            },
        ]
    }

    #[test]
    fn current_chunk_from_anchor_tracks_next_changed_chunk() {
        let layouts = sample_layouts();

        assert_eq!(
            current_chunk_from_anchor(&layouts, MergePane::Center, 12.5),
            Some(3)
        );
    }

    #[test]
    fn navigate_conflict_target_skips_non_conflict_chunks() {
        let layouts = sample_layouts();

        assert_eq!(navigate_conflict_target(&layouts, Some(2), true), Some(3));
        assert_eq!(navigate_conflict_target(&layouts, Some(2), false), Some(1));
    }

    #[test]
    fn conflict_position_for_chunk_uses_following_conflict() {
        let layouts = sample_layouts();

        assert_eq!(conflict_position_for_chunk(&layouts, 2), Some(1));
    }

    #[test]
    fn map_anchor_between_panes_preserves_tail_offset_after_last_change() {
        let layouts = sample_layouts();

        assert_eq!(
            map_anchor_between_panes(&layouts, MergePane::Left, MergePane::Center, 15.0),
            Some(20.0)
        );
    }

    #[test]
    fn left_gutter_exposes_base_and_ours_actions_for_ours_only_chunk() {
        let block = LinkMapBlock {
            chunk_id: 2,
            chunk_type: MergeChunkType::OursOnly,
            resolved: true,
            left_range: 7..9,
            right_range: 10..12,
        };

        let buttons = gutter_buttons_for_block(
            LinkMapSide::Left,
            &block,
            &[
                Some(ChunkResolution::Base),
                None,
                Some(ChunkResolution::Ours),
            ],
        );

        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].kind, GutterButtonKind::Base);
        assert_eq!(buttons[1].kind, GutterButtonKind::Ours);
        assert!(buttons[1].active);
    }

    #[test]
    fn right_gutter_exposes_theirs_and_base_actions_for_conflict_chunk() {
        let block = LinkMapBlock {
            chunk_id: 3,
            chunk_type: MergeChunkType::Conflict,
            resolved: true,
            left_range: 15..18,
            right_range: 10..12,
        };

        let buttons = gutter_buttons_for_block(
            LinkMapSide::Right,
            &block,
            &[
                Some(ChunkResolution::Base),
                None,
                Some(ChunkResolution::Ours),
                Some(ChunkResolution::Base),
            ],
        );

        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].kind, GutterButtonKind::Theirs);
        assert_eq!(buttons[1].kind, GutterButtonKind::Base);
        assert!(buttons[1].active);
    }
}
