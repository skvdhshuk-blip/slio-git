//! Index (staging area) operations for git-core

use crate::error::GitError;
#[cfg_attr(feature = "app-store", path = "index/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "index/desktop.rs")]
mod backend;
use crate::repository::Repository;
use git2::{DiffOptions, StatusOptions};
use log::info;
use std::path::Path;

/// Status of a file change
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Ignored,
    Conflict,
}

/// A file change
#[derive(Debug, Clone)]
pub struct Change {
    pub path: String,
    pub status: ChangeStatus,
    pub staged: bool,
    pub unstaged: bool,
    pub old_oid: Option<String>,
    pub new_oid: Option<String>,
    /// Whether this change is a submodule entry
    pub is_submodule: bool,
    /// Commit range summary for submodule changes (e.g., "abc1234..def5678")
    pub submodule_summary: Option<String>,
}

/// An entry in the index
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: String,
    pub oid: String,
    pub mode: u32,
    pub stage: u32,
}

/// Get the current index
pub fn get_index(repo: &Repository) -> Result<Index, GitError> {
    let repo_lock = repo.inner.read().unwrap();
    let index = repo_lock.index().map_err(|e| GitError::OperationFailed {
        operation: "get_index".to_string(),
        details: e.to_string(),
    })?;

    Ok(Index { inner: index })
}

/// Stage a file
pub fn stage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    info!("Staging file: {:?}", path);

    let repo_lock = repo.inner.write().unwrap();
    let mut index = repo_lock.index().map_err(|e| GitError::OperationFailed {
        operation: "stage_file".to_string(),
        details: e.to_string(),
    })?;

    match std::fs::symlink_metadata(repo.command_cwd().join(path)) {
        Ok(_) => index.add_path(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let indexed = (0..=3).any(|stage| index.get_path(path, stage).is_some());
            let head = head_commit(&repo_lock)?;
            let tree = head.as_ref().map(|head| head.tree()).transpose()?;
            if !indexed && !tree.as_ref().is_some_and(|tree| tree.get_path(path).is_ok()) {
                return Err(error.into());
            }
            index.remove_path(path)?;
        }
        Err(error) => return Err(error.into()),
    }

    index.write().map_err(|e| GitError::OperationFailed {
        operation: "stage_file".to_string(),
        details: e.to_string(),
    })?;

    Ok(())
}

/// Reset only the selected index entry, including before the first commit.
pub fn unstage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    let raw = repo.inner.write().unwrap();
    let head = head_commit(&raw)?;
    let tree = head.as_ref().map(|commit| commit.tree()).transpose()?;
    let mut index = raw.index()?;
    index.remove_path(path)?;
    if let Some(entry) = tree
        .as_ref()
        .map(|tree| tree.get_path(path))
        .transpose()
        .or_else(|error| {
            if error.code() == git2::ErrorCode::NotFound {
                Ok(None)
            } else {
                Err(error)
            }
        })?
    {
        index.add(&git2::IndexEntry {
            id: entry.id(),
            mode: entry.filemode() as u32,
            path: path.as_os_str().as_encoded_bytes().to_vec(),
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            uid: 0,
            gid: 0,
            file_size: 0,
            flags: 0,
            flags_extended: 0,
        })?;
    }
    index.write()?;
    Ok(())
}

fn head_commit(raw: &git2::Repository) -> Result<Option<git2::Commit<'_>>, GitError> {
    match raw.head() {
        Ok(head) => Ok(Some(head.peel_to_commit()?)),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Stage all files
pub fn stage_all(repo: &Repository) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    info!("Staging all files");

    let repo_lock = repo.inner.write().unwrap();
    let mut index = repo_lock.index().map_err(|e| GitError::OperationFailed {
        operation: "stage_all".to_string(),
        details: e.to_string(),
    })?;

    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .map_err(|e| GitError::OperationFailed {
            operation: "stage_all".to_string(),
            details: e.to_string(),
        })?;

    index.write().map_err(|e| GitError::OperationFailed {
        operation: "stage_all".to_string(),
        details: e.to_string(),
    })?;

    Ok(())
}

/// Unstage all files
pub fn unstage_all(repo: &Repository) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    let repo_lock = repo.inner.write().unwrap();
    let head = head_commit(&repo_lock)?;
    let mut index = repo_lock.index()?;
    if let Some(head) = head {
        index.read_tree(&head.tree()?)?;
    } else {
        index.clear()?;
    }
    index.write()?;
    Ok(())
}

/// Convert git2 status flags to our ChangeStatus
fn convert_status(status: git2::Status) -> ChangeStatus {
    use git2::Status;
    if status.intersects(Status::INDEX_NEW) {
        ChangeStatus::Added
    } else if status.intersects(Status::INDEX_MODIFIED) {
        ChangeStatus::Modified
    } else if status.intersects(Status::INDEX_DELETED) {
        ChangeStatus::Deleted
    } else if status.intersects(Status::INDEX_RENAMED) {
        ChangeStatus::Renamed
    } else if status.intersects(Status::WT_NEW) {
        ChangeStatus::Untracked
    } else if status.intersects(Status::WT_MODIFIED) {
        ChangeStatus::Modified
    } else if status.intersects(Status::WT_DELETED) {
        ChangeStatus::Deleted
    } else if status.intersects(Status::CONFLICTED) {
        ChangeStatus::Conflict
    } else if status.intersects(Status::IGNORED) {
        ChangeStatus::Ignored
    } else {
        ChangeStatus::Modified
    }
}

fn has_staged_status(status: git2::Status) -> bool {
    status.intersects(
        git2::Status::INDEX_NEW
            | git2::Status::INDEX_MODIFIED
            | git2::Status::INDEX_DELETED
            | git2::Status::INDEX_RENAMED
            | git2::Status::INDEX_TYPECHANGE,
    )
}

fn has_unstaged_status(status: git2::Status) -> bool {
    status.intersects(
        git2::Status::WT_MODIFIED
            | git2::Status::WT_DELETED
            | git2::Status::WT_RENAMED
            | git2::Status::WT_TYPECHANGE
            | git2::Status::WT_NEW,
    )
}

/// Get file status changes
pub fn get_status(repo: &Repository) -> Result<Vec<Change>, GitError> {
    let repo_lock = repo.inner.read().unwrap();

    let mut status_options = StatusOptions::new();
    status_options.include_untracked(true);
    status_options.recurse_untracked_dirs(true);

    let statuses =
        repo_lock
            .statuses(Some(&mut status_options))
            .map_err(|e| GitError::OperationFailed {
                operation: "get_status".to_string(),
                details: e.to_string(),
            })?;

    let mut changes = Vec::new();

    for entry in statuses.iter() {
        let status = entry.status();

        // Skip ignored files
        if status.intersects(git2::Status::IGNORED) {
            continue;
        }

        let file_path = entry.path().unwrap_or("").to_string();
        let is_submodule = entry.status().intersects(git2::Status::WT_TYPECHANGE)
            || crate::submodule::is_submodule(repo, &file_path);
        let submodule_summary = if is_submodule {
            crate::submodule::submodule_summary(repo, &file_path)
        } else {
            None
        };

        changes.push(Change {
            path: file_path,
            status: convert_status(status),
            staged: has_staged_status(status),
            unstaged: has_unstaged_status(status),
            old_oid: None,
            new_oid: None,
            is_submodule,
            submodule_summary,
        });
    }

    Ok(changes)
}

/// Check if repository has merge conflicts
pub fn has_conflicts(repo: &Repository) -> bool {
    let changes = get_status(repo).ok();
    changes
        .map(|c| c.iter().any(|ch| ch.status == ChangeStatus::Conflict))
        .unwrap_or(false)
}

/// Get list of conflicted files
pub fn get_conflicted_files(repo: &Repository) -> Result<Vec<String>, GitError> {
    let changes = get_status(repo)?;
    Ok(changes
        .into_iter()
        .filter(|c| c.status == ChangeStatus::Conflict)
        .map(|c| c.path)
        .collect())
}

/// The index/staging area
pub struct Index {
    inner: git2::Index,
}

impl Index {
    /// List entries in the index (simplified)
    pub fn list_entries(&self) -> Vec<IndexEntry> {
        self.inner
            .iter()
            .map(|entry| IndexEntry {
                path: String::from_utf8_lossy(&entry.path).to_string(),
                oid: entry.id.to_string(),
                mode: entry.mode,
                stage: ((entry.flags >> 12) & 0x3) as u32,
            })
            .collect()
    }
}

/// A hunk in a file diff
#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<HunkLine>,
}

/// A line in a hunk
#[derive(Debug, Clone)]
pub struct HunkLine {
    pub origin: char,
    pub content: String,
}

/// Which side of the diff a line selection refers to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineSide {
    /// Old (deletion) side — lines prefixed with `-`
    Old,
    /// New (addition) side — lines prefixed with `+`
    New,
}

/// Get hunks for a specific file (between workdir and index)
pub fn get_file_hunks(repo: &Repository, file_path: &Path) -> Result<Vec<Hunk>, GitError> {
    let repo_lock = repo.inner.read().unwrap();

    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(file_path).disable_pathspec_match(true);

    let diff = repo_lock
        .diff_index_to_workdir(None, Some(&mut diff_opts))
        .map_err(|e| GitError::OperationFailed {
            operation: "get_file_hunks".to_string(),
            details: e.to_string(),
        })?;

    collect_diff_hunks(&diff)
}

/// Collect hunks from a git2 Diff, correctly aggregating lines per hunk.
///
/// In git2's `diff.print` callback, `hunk` is `Some` for *every* line that
/// belongs to a hunk (not just the first line).  We detect hunk boundaries by
/// comparing the hunk header.  Lines with origin `'H'` are the hunk-header
/// pseudo-lines that git2 emits and are skipped (the header text is taken from
/// `hunk.header()` instead).
fn collect_diff_hunks(diff: &git2::Diff) -> Result<Vec<Hunk>, GitError> {
    let mut hunks: Vec<Hunk> = Vec::new();

    diff.print(git2::DiffFormat::Patch, |_delta, hunk_opt, line| {
        let origin = line.origin();

        // Display hunks contain only real lines. EOF markers are derived from
        // each line's bytes when generating a patch, never counted as content.
        if !matches!(origin, ' ' | '+' | '-') {
            return true;
        }

        if let Some(hunk_info) = hunk_opt {
            let header = String::from_utf8_lossy(hunk_info.header()).to_string();
            let needs_new = match hunks.last() {
                Some(last) => last.header != header,
                None => true,
            };
            if needs_new {
                let (old_start, old_lines, new_start, new_lines) = parse_hunk_header(&header);
                hunks.push(Hunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header,
                    lines: Vec::new(),
                });
            }
        }

        // Append the line to the current (last) hunk.
        if let Some(last_hunk) = hunks.last_mut() {
            let content = String::from_utf8_lossy(line.content()).to_string();
            last_hunk.lines.push(HunkLine { origin, content });
        }

        true
    })
    .map_err(|e| GitError::OperationFailed {
        operation: "collect_diff_hunks".to_string(),
        details: e.to_string(),
    })?;

    Ok(hunks)
}

/// Parse hunk header to extract line information
fn parse_hunk_header(header: &str) -> (u32, u32, u32, u32) {
    // Format: "@@ -old_start[,old_lines] +new_start[,new_lines] @@"
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 2 {
        return (0, 0, 0, 0);
    }

    let old_part = parts.get(1).unwrap_or(&"");
    let new_part = parts.get(2).unwrap_or(&"");

    let (old_start, old_lines) = parse_range_part(old_part);
    let (new_start, new_lines) = parse_range_part(new_part);

    (old_start, old_lines, new_start, new_lines)
}

/// Parse a range part like "-1,5" or "-1"
fn parse_range_part(part: &str) -> (u32, u32) {
    let s = part.trim_start_matches('-').trim_start_matches('+');
    let nums: Vec<&str> = s.split(',').collect();

    let start = nums.first().and_then(|n| n.parse().ok()).unwrap_or(0);
    let lines = nums.get(1).and_then(|n| n.parse().ok()).unwrap_or(1);

    (start, lines)
}

/// Stage a specific hunk without serializing raw diff bytes through UI text.
pub fn stage_hunk(repo: &Repository, file_path: &Path, hunk_index: usize) -> Result<(), GitError> {
    apply_index_hunk(repo, file_path, hunk_index, false)
}

/// Unstage only this hunk; the worktree and other index hunks stay untouched.
pub fn unstage_hunk(
    repo: &Repository,
    file_path: &Path,
    hunk_index: usize,
) -> Result<(), GitError> {
    apply_index_hunk(repo, file_path, hunk_index, true)
}

fn apply_index_hunk(
    repo: &Repository,
    path: &Path,
    selected: usize,
    reverse: bool,
) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    let raw = repo.inner.write().unwrap();
    let mut options = DiffOptions::new();
    options
        .pathspec(path)
        .disable_pathspec_match(true)
        .reverse(reverse);
    let head = head_commit(&raw)?;
    let tree = head.as_ref().map(|commit| commit.tree()).transpose()?;
    let diff = if reverse {
        raw.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?
    } else {
        raw.diff_index_to_workdir(None, Some(&mut options))?
    };
    let patch = git2::Patch::from_diff(&diff, 0)?;
    if patch
        .as_ref()
        .is_none_or(|patch| selected >= patch.num_hunks())
    {
        return Err(GitError::InvalidInput {
            message: format!("Hunk {selected} not found"),
        });
    }
    let mut position = 0;
    let mut apply = git2::ApplyOptions::new();
    apply.hunk_callback(|_| {
        let include = position == selected;
        position += 1;
        include
    });
    raw.apply(&diff, git2::ApplyLocation::Index, Some(&mut apply))?;
    Ok(())
}

fn apply_patch_cached(repo: &Repository, patch: &str) -> Result<(), GitError> {
    backend::apply_patch_cached(repo, patch)
}

/// Get hunks between index and HEAD for a specific file
pub fn get_index_hunks(repo: &Repository, file_path: &Path) -> Result<Vec<Hunk>, GitError> {
    let repo_lock = repo.inner.read().unwrap();

    let head = head_commit(&repo_lock)?;
    let head_tree = head.as_ref().map(|commit| commit.tree()).transpose()?;

    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(file_path).disable_pathspec_match(true);

    let diff = repo_lock
        .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))
        .map_err(|e| GitError::OperationFailed {
            operation: "get_index_hunks".to_string(),
            details: e.to_string(),
        })?;

    collect_diff_hunks(&diff)
}

/// Build the actual unified-diff patch body (shared by stage and unstage
/// generators).  `out_lines` is a list of `(origin_char, content)` pairs
/// already filtered/counted by the caller.
fn build_patch_string(
    file_path: &Path,
    hunk: &Hunk,
    out_lines: &[(char, &str)],
    old_count: u32,
    new_count: u32,
) -> String {
    let path_str = file_path.to_string_lossy();
    let mut patch = String::new();
    patch.push_str(&format!("diff --git a/{path} b/{path}\n", path = path_str));
    patch.push_str(&format!("--- a/{path}\n", path = path_str));
    patch.push_str(&format!("+++ b/{path}\n", path = path_str));
    patch.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        hunk.old_start, old_count, hunk.new_start, new_count,
    ));

    for (origin, content) in out_lines {
        patch.push(*origin);
        patch.push_str(content);
        if !content.ends_with('\n') {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    }

    patch
}

/// Generate a unified-diff patch for **staging** selected lines from a
/// workdir-vs-index hunk.
///
/// The index currently holds the *old* content.  Selected change lines are
/// emitted as-is; opposite-side change lines are demoted to context (they
/// still exist in the index); non-selected same-side change lines are
/// dropped entirely (they do not exist in the index).
fn generate_line_patch(
    file_path: &Path,
    hunk: &Hunk,
    selected: &[usize],
    side: LineSide,
) -> Result<String, GitError> {
    let selected_set: std::collections::HashSet<usize> = selected.iter().copied().collect();

    let mut out_lines: Vec<(char, &str)> = Vec::with_capacity(hunk.lines.len());
    let mut old_count: u32 = 0;
    let mut new_count: u32 = 0;

    for (idx, line) in hunk.lines.iter().enumerate() {
        let emit: Option<char> = match line.origin {
            ' ' => {
                old_count += 1;
                new_count += 1;
                Some(' ')
            }
            '+' => {
                if side == LineSide::New && selected_set.contains(&idx) {
                    new_count += 1;
                    Some('+')
                } else if side == LineSide::New {
                    // Non-selected addition — drop (not in index).
                    None
                } else {
                    // Opposite side (addition when staging deletions) — drop.
                    None
                }
            }
            '-' => {
                if side == LineSide::Old && selected_set.contains(&idx) {
                    old_count += 1;
                    Some('-')
                } else if side == LineSide::Old {
                    // Non-selected deletion — demote to context (still in index).
                    old_count += 1;
                    new_count += 1;
                    Some(' ')
                } else {
                    // Opposite side (deletion when staging additions) — demote to context
                    // because the line still exists in the index.
                    old_count += 1;
                    new_count += 1;
                    Some(' ')
                }
            }
            other => {
                old_count += 1;
                new_count += 1;
                Some(other)
            }
        };
        if let Some(origin) = emit {
            out_lines.push((origin, &line.content));
        }
    }

    if old_count == new_count && out_lines.iter().all(|(c, _)| *c == ' ') {
        return Err(GitError::OperationFailed {
            operation: "generate_line_patch".to_string(),
            details: "Selected lines produce an empty diff".to_string(),
        });
    }

    Ok(build_patch_string(file_path, hunk, &out_lines, old_count, new_count))
}

/// Generate a unified-diff patch for **unstaging** selected lines from an
/// index-vs-HEAD hunk.
///
/// The index currently holds the *new* (staged) content.  Selected change
/// lines become deletions (`-`); non-selected same-side change lines are
/// demoted to context (they stay in the index); opposite-side change lines
/// are dropped (they are not in the index).
fn generate_line_unstage_patch(
    file_path: &Path,
    hunk: &Hunk,
    selected: &[usize],
    side: LineSide,
) -> Result<String, GitError> {
    let selected_set: std::collections::HashSet<usize> = selected.iter().copied().collect();

    let mut out_lines: Vec<(char, &str)> = Vec::with_capacity(hunk.lines.len());
    let mut old_count: u32 = 0;
    let mut new_count: u32 = 0;

    for (idx, line) in hunk.lines.iter().enumerate() {
        let emit: Option<char> = match line.origin {
            ' ' => {
                old_count += 1;
                new_count += 1;
                Some(' ')
            }
            '+' => {
                if side == LineSide::New && selected_set.contains(&idx) {
                    // Selected staged addition — turn into deletion (remove from index).
                    old_count += 1;
                    Some('-')
                } else if side == LineSide::New {
                    // Non-selected staged addition — demote to context (stays in index).
                    old_count += 1;
                    new_count += 1;
                    Some(' ')
                } else {
                    // Opposite side (addition when unstaging deletions) — drop
                    // (these lines are not in the index).
                    None
                }
            }
            '-' => {
                if side == LineSide::Old && selected_set.contains(&idx) {
                    // Selected staged deletion — turn into addition (restore to index).
                    new_count += 1;
                    Some('+')
                } else if side == LineSide::Old {
                    // Non-selected staged deletion — drop (not in index, stays deleted).
                    None
                } else {
                    // Opposite side (deletion when unstaging additions) — drop.
                    None
                }
            }
            other => {
                old_count += 1;
                new_count += 1;
                Some(other)
            }
        };
        if let Some(origin) = emit {
            out_lines.push((origin, &line.content));
        }
    }

    if old_count == new_count && out_lines.iter().all(|(c, _)| *c == ' ') {
        return Err(GitError::OperationFailed {
            operation: "generate_line_unstage_patch".to_string(),
            details: "Selected lines produce an empty diff".to_string(),
        });
    }

    Ok(build_patch_string(file_path, hunk, &out_lines, old_count, new_count))
}

/// Stage specific lines within a hunk of a file.
///
/// `line_indices` are indices into the hunk's `lines` vector.  Only lines
/// whose `origin` matches `side` ('+' for `New`, '-' for `Old`) are acted
/// upon; all other change lines in the hunk are demoted to context so that
/// the resulting patch is a valid unified diff.
pub fn stage_lines(
    repo: &Repository,
    file_path: &Path,
    hunk_index: usize,
    line_indices: &[usize],
    side: LineSide,
) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    info!(
        "Staging lines {:?} (side {:?}) from hunk {} of {:?}",
        line_indices, side, hunk_index, file_path
    );

    let hunks = get_file_hunks(repo, file_path)?;
    let hunk = hunks
        .get(hunk_index)
        .ok_or_else(|| GitError::OperationFailed {
            operation: "stage_lines".to_string(),
            details: format!("Hunk {} not found", hunk_index),
        })?;

    let patch = generate_line_patch(file_path, hunk, line_indices, side)?;
    apply_patch_cached(repo, &patch)?;

    Ok(())
}

/// Unstage specific lines within a staged hunk of a file.
///
/// Works the same as [`stage_lines`] but operates on the index-to-HEAD diff
/// and applies a reverse patch (`git apply --cached --reverse`).
pub fn unstage_lines(
    repo: &Repository,
    file_path: &Path,
    hunk_index: usize,
    line_indices: &[usize],
    side: LineSide,
) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    info!(
        "Unstaging lines {:?} (side {:?}) from hunk {} of {:?}",
        line_indices, side, hunk_index, file_path
    );

    let hunks = get_index_hunks(repo, file_path)?;
    let hunk = hunks
        .get(hunk_index)
        .ok_or_else(|| GitError::OperationFailed {
            operation: "unstage_lines".to_string(),
            details: format!("Hunk {} not found in index-to-HEAD diff", hunk_index),
        })?;

    let patch = generate_line_unstage_patch(file_path, hunk, line_indices, side)?;
    apply_patch_cached(repo, &patch)?;

    Ok(())
}

/// Discard changes for a file: reset both index and worktree to HEAD.
/// For untracked files, removes the file from the working directory.
pub fn discard_file(repo: &Repository, file_path: &Path) -> Result<(), GitError> {
    crate::native::allow_edit(repo)?;
    let repo_path = repo.command_cwd();
    let full_path = repo_path.join(file_path);

    // Check if file is tracked
    let tracked = {
        let repo_lock = repo.inner.read().unwrap();
        repo_lock
            .revparse_single("HEAD")
            .ok()
            .and_then(|obj| obj.peel_to_commit().ok())
            .and_then(|commit| commit.tree().ok())
            .map(|tree| tree.get_path(file_path).is_ok())
            .unwrap_or(false)
    };

    if tracked {
        let repo_lock = repo.inner.write().unwrap();
        let head = repo_lock
            .head()
            .and_then(|head| head.peel_to_commit())
            .map_err(|e| GitError::OperationFailed {
                operation: "discard_file".to_string(),
                details: e.to_string(),
            })?;
        repo_lock
            .reset_default(Some(head.as_object()), [file_path])
            .map_err(|e| GitError::OperationFailed {
                operation: "discard_file".to_string(),
                details: e.to_string(),
            })?;
        let mut opts = git2::build::CheckoutBuilder::new();
        opts.force();
        opts.path(file_path);
        repo_lock
            .checkout_tree(head.as_object(), Some(&mut opts))
            .map_err(|e| GitError::OperationFailed {
                operation: "discard_file".to_string(),
                details: e.to_string(),
            })?;
    } else {
        // A new index entry may already be absent from the working directory.
        match std::fs::symlink_metadata(&full_path) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(&full_path)?,
            Ok(_) => std::fs::remove_file(&full_path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let raw = repo.inner.write().unwrap();
        let mut index = raw.index()?;
        index.remove_path(file_path)?;
        index.write()?;
    }

    Ok(())
}

/// Test-only wrapper that exposes `generate_line_patch` publicly.
#[doc(hidden)]
pub fn generate_line_patch_for_test(
    file_path: &Path,
    hunk: &Hunk,
    selected: &[usize],
    side: LineSide,
) -> String {
    generate_line_patch(file_path, hunk, selected, side).unwrap()
}
