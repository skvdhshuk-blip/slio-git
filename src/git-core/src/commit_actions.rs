//! Commit-level actions for branch/history workflows.
//!
//! All operations use libgit2 only — no system `git` process — so they work
//! in the App Store sandbox.

use crate::commit;
use crate::error::GitError;
use crate::git_utils::{current_head_oid, is_ancestor, resolve_commit_oid};
use crate::index;
use crate::repository::{Repository, RepositoryState};
use log::info;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgressCommitActionKind {
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InProgressCommitAction {
    pub kind: InProgressCommitActionKind,
    pub commit_id: Option<String>,
    pub subject: Option<String>,
    pub conflicted_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushCurrentBranchTarget {
    pub remote_name: String,
    pub local_branch_name: String,
    pub upstream_ref: String,
    pub upstream_branch_name: String,
    pub selected_commit: String,
    pub expected_remote_oid: String,
    pub is_fast_forward: bool,
    pub requires_force_with_lease: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteExecution {
    Completed,
    InProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RewriteKind {
    EditMessage,
    Fixup,
    Squash,
    Drop,
}

#[derive(Debug, Clone)]
struct RewriteSelection {
    selected_oid: git2::Oid,
    base_spec: Option<String>,
    chain: Vec<git2::Oid>,
    selected_index: usize,
}

fn parse_upstream_ref(reference: &str) -> Option<(&str, &str)> {
    reference.split_once('/')
}

fn git_dir(repo: &Repository) -> &Path {
    &repo.path
}

fn commit_subject(message: &str) -> &str {
    message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(message)
}

fn ensure_no_in_progress_operation(repo: &Repository, operation: &str) -> Result<(), GitError> {
    match repo.get_state() {
        RepositoryState::Clean | RepositoryState::Dirty => Ok(()),
        _ => Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: format!(
                "当前仓库正处于 {}，请先完成或中止当前流程",
                repo.state_hint()
                    .unwrap_or_else(|| "进行中的 Git 操作".to_string())
            ),
        }),
    }
}

fn ensure_clean_worktree(repo: &Repository, operation: &str) -> Result<(), GitError> {
    ensure_no_in_progress_operation(repo, operation)?;

    if index::get_status(repo)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?
        .is_empty()
    {
        Ok(())
    } else {
        Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "当前仓库还有未提交改动，请先提交、暂存或清理工作区".to_string(),
        })
    }
}

fn signature_for(repo_lock: &git2::Repository) -> Result<git2::Signature<'static>, GitError> {
    commit::signature_from_locked(repo_lock)
}

/// Apply through the repository API: it owns the index, worktree and recovery markers.
fn apply_commit_replay(
    repo: &Repository,
    commit_id: &str,
    kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    let operation = match kind {
        InProgressCommitActionKind::CherryPick => "cherry-pick",
        InProgressCommitActionKind::Revert => "revert",
    };
    ensure_no_in_progress_operation(repo, operation)?;
    let raw = repo.inner.write().unwrap();
    let source = raw.revparse_single(commit_id)?.peel_to_commit()?;
    let signature = signature_for(&raw)?;
    let result = match kind {
        InProgressCommitActionKind::CherryPick => raw.cherrypick(&source, None),
        InProgressCommitActionKind::Revert => raw.revert(&source, None),
    };
    result.map_err(|error| GitError::OperationFailed {
        operation: operation.into(),
        details: error.to_string(),
    })?;
    if raw.index()?.has_conflicts() {
        return Err(GitError::OperationFailed {
            operation: operation.into(),
            details: "存在冲突，请解决后继续或中止".into(),
        });
    }
    finish_commit_replay(&raw, kind, &signature)
}

fn finish_commit_replay(
    raw: &git2::Repository,
    kind: InProgressCommitActionKind,
    signature: &git2::Signature<'_>,
) -> Result<(), GitError> {
    let head_file = match kind {
        InProgressCommitActionKind::CherryPick => "CHERRY_PICK_HEAD",
        InProgressCommitActionKind::Revert => "REVERT_HEAD",
    };
    let source_oid = fs::read_to_string(raw.path().join(head_file))?;
    let source = raw.find_commit(git2::Oid::from_str(source_oid.trim())?)?;
    let head = raw.head()?.peel_to_commit()?;
    let mut index = raw.index()?;
    if index.has_conflicts() {
        return Err(GitError::MergeConflict);
    }
    let tree = raw.find_tree(index.write_tree()?)?;
    let message = raw.message()?;
    let author = match kind {
        InProgressCommitActionKind::CherryPick => source.author(),
        InProgressCommitActionKind::Revert => signature.to_owned(),
    };
    raw.commit(Some("HEAD"), &author, signature, &message, &tree, &[&head])?;
    raw.cleanup_state()?;
    Ok(())
}

/// Replay `source` onto `onto` (or empty tree when `onto` is None) and return the new commit OID.
fn cherry_pick_commit_onto(
    repo_lock: &git2::Repository,
    source: &git2::Commit<'_>,
    onto: Option<&git2::Commit<'_>>,
    message: &str,
) -> Result<git2::Oid, GitError> {
    let empty = repo_lock.find_tree(repo_lock.treebuilder(None)?.write()?)?;
    let base = if source.parent_count() == 0 {
        None
    } else {
        Some(source.parent(0)?.tree()?)
    };
    let parent_tree = onto.map(|parent| parent.tree()).transpose()?;
    let index = repo_lock.merge_trees(
        base.as_ref().unwrap_or(&empty),
        parent_tree.as_ref().unwrap_or(&empty),
        &source.tree()?,
        None,
    )?;

    if index.has_conflicts() {
        return Err(GitError::OperationFailed {
            operation: "rewrite".to_string(),
            details: format!("改写历史时 {} 产生冲突，请先清理工作区后重试", source.id()),
        });
    }

    let mut index = index;
    let tree_oid = index
        .write_tree_to(repo_lock)
        .map_err(|e| GitError::OperationFailed {
            operation: "rewrite".to_string(),
            details: e.to_string(),
        })?;
    let tree = repo_lock
        .find_tree(tree_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: "rewrite".to_string(),
            details: e.to_string(),
        })?;
    let signature = signature_for(repo_lock)?;
    let parents: Vec<&git2::Commit<'_>> = onto.into_iter().collect();
    Ok(repo_lock.commit(None, &source.author(), &signature, message, &tree, &parents)?)
}

fn move_head_to_commit(
    raw: &git2::Repository,
    commit: &git2::Commit<'_>,
    branch_name: Option<&str>,
) -> Result<(), GitError> {
    let original = raw.head()?.peel_to_commit()?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    raw.checkout_tree(commit.as_object(), Some(&mut checkout))?;
    let result = if let Some(name) = branch_name {
        raw.reference_matching(
            &format!("refs/heads/{name}"),
            commit.id(),
            true,
            original.id(),
            "rewrite history",
        )
        .map(|_| ())
    } else {
        raw.set_head_detached(commit.id())
    };
    if let Err(error) = result {
        // The branch was not published. Restore only this operation's checkout.
        let mut restore = git2::build::CheckoutBuilder::new();
        restore.safe();
        raw.checkout_tree(original.as_object(), Some(&mut restore))?;
        return Err(error.into());
    }
    Ok(())
}

fn combine_commit_into(
    repo_lock: &git2::Repository,
    parent_oid: git2::Oid,
    source: &git2::Commit<'_>,
    message: &str,
) -> Result<git2::Oid, GitError> {
    let parent = repo_lock
        .find_commit(parent_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: "squash".to_string(),
            details: e.to_string(),
        })?;
    let index = repo_lock
        .cherrypick_commit(source, &parent, 0, None)
        .map_err(|e| GitError::OperationFailed {
            operation: "squash".to_string(),
            details: e.to_string(),
        })?;
    if index.has_conflicts() {
        return Err(GitError::OperationFailed {
            operation: "squash".to_string(),
            details: "合并提交时产生冲突，请先手动整理历史".to_string(),
        });
    }
    let mut index = index;
    let tree_oid = index
        .write_tree_to(repo_lock)
        .map_err(|e| GitError::OperationFailed {
            operation: "squash".to_string(),
            details: e.to_string(),
        })?;
    let tree = repo_lock
        .find_tree(tree_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: "squash".to_string(),
            details: e.to_string(),
        })?;
    let signature = signature_for(repo_lock)?;
    let parents: Vec<_> = parent.parents().collect();
    let parents: Vec<_> = parents.iter().collect();
    Ok(repo_lock.commit(None, &parent.author(), &signature, message, &tree, &parents)?)
}

fn rewrite_first_parent_chain(
    repo: &Repository,
    selection: &RewriteSelection,
    kind: RewriteKind,
    override_message: Option<&str>,
) -> Result<RewriteExecution, GitError> {
    let branch_name = repo
        .current_branch()
        .map_err(|e| GitError::OperationFailed {
            operation: "rewrite".to_string(),
            details: e.to_string(),
        })?;

    let repo_lock = repo.inner.write().unwrap();
    let base_oid = match &selection.base_spec {
        Some(oid_str) => {
            Some(
                git2::Oid::from_str(oid_str).map_err(|_| GitError::CommitNotFound {
                    id: oid_str.clone(),
                })?,
            )
        }
        None => None,
    };

    let start_index = selection
        .base_spec
        .as_ref()
        .and_then(|base| {
            selection
                .chain
                .iter()
                .position(|oid| oid.to_string() == *base)
        })
        .map(|index| index + 1)
        .unwrap_or(0);

    let mut new_head_oid = base_oid;
    for oid in &selection.chain[start_index..] {
        let source = repo_lock
            .find_commit(*oid)
            .map_err(|e| GitError::OperationFailed {
                operation: "rewrite".to_string(),
                details: e.to_string(),
            })?;
        if source.parent_count() > 1 {
            return Err(GitError::InvalidInput {
                message: "整理范围包含 merge 提交，不能按线性历史改写".into(),
            });
        }
        let is_selected = *oid == selection.selected_oid;

        if is_selected {
            match kind {
                RewriteKind::Drop => continue,
                RewriteKind::EditMessage => {
                    let message = override_message
                        .map(str::to_string)
                        .unwrap_or_else(|| source.message().unwrap_or("").to_string());
                    let parent = match new_head_oid {
                        Some(pid) => Some(repo_lock.find_commit(pid).map_err(|e| {
                            GitError::OperationFailed {
                                operation: "rewrite".to_string(),
                                details: e.to_string(),
                            }
                        })?),
                        None => None,
                    };
                    new_head_oid = Some(cherry_pick_commit_onto(
                        &repo_lock,
                        &source,
                        parent.as_ref(),
                        &message,
                    )?);
                }
                RewriteKind::Fixup => {
                    let parent_oid = new_head_oid.ok_or_else(|| GitError::OperationFailed {
                        operation: "fixup".to_string(),
                        details: "根提交前面没有可合并的目标提交".to_string(),
                    })?;
                    let parent_msg = repo_lock
                        .find_commit(parent_oid)
                        .ok()
                        .and_then(|parent| parent.message().map(str::to_string))
                        .unwrap_or_default();
                    new_head_oid = Some(combine_commit_into(
                        &repo_lock,
                        parent_oid,
                        &source,
                        &parent_msg,
                    )?);
                }
                RewriteKind::Squash => {
                    let parent_oid = new_head_oid.ok_or_else(|| GitError::OperationFailed {
                        operation: "squash".to_string(),
                        details: "根提交前面没有可合并的目标提交".to_string(),
                    })?;
                    let parent_msg = repo_lock
                        .find_commit(parent_oid)
                        .ok()
                        .and_then(|parent| parent.message().map(str::to_string))
                        .unwrap_or_default();
                    let source_msg = source.message().unwrap_or("").trim();
                    let parent_trim = parent_msg.trim_end();
                    let message = if source_msg.is_empty() {
                        parent_msg
                    } else if parent_trim.is_empty() {
                        source_msg.to_string()
                    } else {
                        format!("{parent_trim}\n\n{source_msg}")
                    };
                    new_head_oid = Some(combine_commit_into(
                        &repo_lock, parent_oid, &source, &message,
                    )?);
                }
            }
        } else {
            let message = source.message().unwrap_or("");
            let parent = match new_head_oid {
                Some(pid) => {
                    Some(
                        repo_lock
                            .find_commit(pid)
                            .map_err(|e| GitError::OperationFailed {
                                operation: "rewrite".to_string(),
                                details: e.to_string(),
                            })?,
                    )
                }
                None => None,
            };
            new_head_oid = Some(cherry_pick_commit_onto(
                &repo_lock,
                &source,
                parent.as_ref(),
                message,
            )?);
        }
    }

    let new_head_oid = new_head_oid.ok_or_else(|| GitError::OperationFailed {
        operation: "rewrite".to_string(),
        details: "历史改写结果为空，无法更新分支".to_string(),
    })?;
    let new_head = repo_lock
        .find_commit(new_head_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: "rewrite".to_string(),
            details: e.to_string(),
        })?;
    move_head_to_commit(&repo_lock, &new_head, branch_name.as_deref())?;
    Ok(RewriteExecution::Completed)
}

fn current_branch_first_parent_chain(
    repo: &Repository,
    operation: &str,
) -> Result<Vec<git2::Oid>, GitError> {
    let head_oid = current_head_oid(repo, operation)?;
    let repo_lock = repo.inner.read().unwrap();
    let mut commit = repo_lock
        .find_commit(head_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;

    let mut chain = Vec::new();
    loop {
        chain.push(commit.id());
        if commit.parent_count() == 0 {
            break;
        }
        commit = commit.parent(0).map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    }
    chain.reverse();
    Ok(chain)
}

fn ensure_local_rewrite_allowed(
    repo: &Repository,
    selected_oid: git2::Oid,
    operation: &str,
) -> Result<(), GitError> {
    if let Some(upstream_ref) = repo.current_upstream_ref() {
        let upstream_oid = resolve_commit_oid(repo, &upstream_ref, operation)?;
        if is_ancestor(repo, selected_oid, upstream_oid)? {
            return Err(GitError::OperationFailed {
                operation: operation.to_string(),
                details: format!(
                    "提交已经包含在当前上游 {upstream_ref} 中，暂不支持直接改写已发布历史"
                ),
            });
        }
    }
    Ok(())
}

fn resolve_rewrite_selection(
    repo: &Repository,
    commit_id: &str,
    operation: &str,
    kind: RewriteKind,
) -> Result<RewriteSelection, GitError> {
    ensure_clean_worktree(repo, operation)?;

    let current_branch = repo
        .current_branch()
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    if current_branch.is_none() {
        return Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "当前为 detached HEAD，不能直接改写当前分支历史".to_string(),
        });
    }

    let selected_oid = resolve_commit_oid(repo, commit_id, operation)?;
    let head_oid = current_head_oid(repo, operation)?;
    if !is_ancestor(repo, selected_oid, head_oid)? {
        return Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "只能改写当前分支主线上的历史提交".to_string(),
        });
    }

    let chain = current_branch_first_parent_chain(repo, operation)?;
    let Some(selected_index) = chain.iter().position(|oid| *oid == selected_oid) else {
        return Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "暂只支持改写当前分支第一父链上的提交".to_string(),
        });
    };

    let selected_info = commit::get_commit(repo, commit_id)?;
    if selected_info.parent_ids.len() > 1 {
        return Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "merge 提交暂不支持直接改说明、fixup、squash 或删除".to_string(),
        });
    }

    if matches!(kind, RewriteKind::Fixup | RewriteKind::Squash) && selected_index == 0 {
        return Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "根提交前面没有可合并的目标提交".to_string(),
        });
    }

    ensure_local_rewrite_allowed(repo, selected_oid, operation)?;

    let range_start = match kind {
        RewriteKind::EditMessage | RewriteKind::Drop => selected_index,
        RewriteKind::Fixup | RewriteKind::Squash => selected_index.saturating_sub(1),
    };

    let base_spec = if range_start == 0 {
        None
    } else {
        Some(chain[range_start - 1].to_string())
    };

    Ok(RewriteSelection {
        selected_oid,
        base_spec,
        chain,
        selected_index,
    })
}

pub fn export_commit_patch(
    repo: &Repository,
    commit_id: &str,
    output_path: &Path,
) -> Result<(), GitError> {
    info!(
        "Exporting patch for commit '{}' to '{}'",
        commit_id,
        output_path.display()
    );

    let repo_lock = repo.inner.read().unwrap();
    let commit = repo_lock.revparse_single(commit_id)?.peel_to_commit()?;
    let patch = git2::Email::from_commit(&commit, &mut git2::EmailCreateOptions::new())?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, patch.as_slice())?;
    Ok(())
}

pub fn get_in_progress_commit_action(
    repo: &Repository,
) -> Result<Option<InProgressCommitAction>, GitError> {
    let (kind, head_file) = match repo.get_state() {
        RepositoryState::CherryPick => (
            InProgressCommitActionKind::CherryPick,
            git_dir(repo).join("CHERRY_PICK_HEAD"),
        ),
        RepositoryState::Revert => (
            InProgressCommitActionKind::Revert,
            git_dir(repo).join("REVERT_HEAD"),
        ),
        _ => return Ok(None),
    };

    let commit_id = fs::read_to_string(&head_file)
        .ok()
        .map(|contents| contents.trim().to_string())
        .filter(|contents| !contents.is_empty());
    let subject = commit_id.as_deref().and_then(|commit_id| {
        commit::get_commit(repo, commit_id)
            .ok()
            .map(|info| commit_subject(&info.message).to_string())
    });
    let conflicted_files = index::get_conflicted_files(repo).unwrap_or_default();

    Ok(Some(InProgressCommitAction {
        kind,
        commit_id,
        subject,
        conflicted_files,
    }))
}

pub fn cherry_pick_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    apply_commit_replay(repo, commit_id, InProgressCommitActionKind::CherryPick)
}

pub fn revert_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    apply_commit_replay(repo, commit_id, InProgressCommitActionKind::Revert)
}

pub fn edit_commit_message(
    repo: &Repository,
    commit_id: &str,
) -> Result<RewriteExecution, GitError> {
    info!("Starting edit-message rewrite for '{}'", commit_id);
    let selection = resolve_rewrite_selection(repo, commit_id, "reword", RewriteKind::EditMessage)?;
    let start = selection
        .base_spec
        .as_ref()
        .and_then(|base| {
            selection
                .chain
                .iter()
                .position(|id| id.to_string() == *base)
        })
        .map_or(0, |index| index + 1);
    let entries: Vec<_> = selection.chain[start..]
        .iter()
        .map(|id| crate::RebaseTodoEntry {
            action: if *id == selection.selected_oid {
                "edit"
            } else {
                "pick"
            }
            .into(),
            commit: id.to_string(),
            message: String::new(),
        })
        .collect();
    crate::rebase::start_interactive_rebase(repo, selection.base_spec.as_deref(), &entries)?;
    Ok(RewriteExecution::InProgress)
}

/// Rewrite a commit message in place (libgit2 chain rewrite).
pub fn reword_commit(
    repo: &Repository,
    commit_id: &str,
    new_message: &str,
) -> Result<RewriteExecution, GitError> {
    info!("Rewording commit '{}'", commit_id);
    if new_message.trim().is_empty() {
        return Err(GitError::InvalidInput {
            message: "提交说明不能为空".into(),
        });
    }
    let selection = resolve_rewrite_selection(repo, commit_id, "reword", RewriteKind::EditMessage)?;
    rewrite_first_parent_chain(
        repo,
        &selection,
        RewriteKind::EditMessage,
        Some(new_message),
    )
}

pub fn fixup_commit_to_previous(
    repo: &Repository,
    commit_id: &str,
) -> Result<RewriteExecution, GitError> {
    info!("Fixup commit '{}' into its previous commit", commit_id);
    let selection = resolve_rewrite_selection(repo, commit_id, "fixup", RewriteKind::Fixup)?;
    rewrite_first_parent_chain(repo, &selection, RewriteKind::Fixup, None)
}

pub fn squash_commit_to_previous(
    repo: &Repository,
    commit_id: &str,
) -> Result<RewriteExecution, GitError> {
    info!("Squashing commit '{}' into its previous commit", commit_id);
    let selection = resolve_rewrite_selection(repo, commit_id, "squash", RewriteKind::Squash)?;
    rewrite_first_parent_chain(repo, &selection, RewriteKind::Squash, None)
}

pub fn drop_commit_from_history(
    repo: &Repository,
    commit_id: &str,
) -> Result<RewriteExecution, GitError> {
    info!("Dropping commit '{}' from current history", commit_id);
    let selection = resolve_rewrite_selection(repo, commit_id, "drop", RewriteKind::Drop)?;
    if selection.selected_index == 0 && selection.chain.len() == 1 {
        return Err(GitError::OperationFailed {
            operation: "drop".to_string(),
            details: "当前分支只剩下这一条根提交，不能直接删除".to_string(),
        });
    }
    rewrite_first_parent_chain(repo, &selection, RewriteKind::Drop, None)
}

pub fn continue_in_progress_commit_action(
    repo: &Repository,
    kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    require_commit_action(repo, kind)?;
    let raw = repo.inner.write().unwrap();
    let signature = signature_for(&raw)?;
    stage_resolved_conflicts(&raw)?;
    finish_commit_replay(&raw, kind, &signature)
}

fn require_commit_action(
    repo: &Repository,
    kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    let expected = match kind {
        InProgressCommitActionKind::CherryPick => RepositoryState::CherryPick,
        InProgressCommitActionKind::Revert => RepositoryState::Revert,
    };
    if repo.get_state() != expected {
        return Err(GitError::InvalidInput {
            message: "没有对应的进行中操作".into(),
        });
    }
    Ok(())
}

/// Stage only paths still marked conflicted, leaving unrelated work alone.
pub(crate) fn stage_resolved_conflicts(raw: &git2::Repository) -> Result<(), GitError> {
    let mut index = raw.index()?;
    let paths: Vec<_> = index
        .conflicts()?
        .map(|entry| {
            let entry = entry?;
            Ok(entry.our.or(entry.their).or(entry.ancestor).unwrap().path)
        })
        .collect::<Result<_, git2::Error>>()?;
    for bytes in paths {
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            Path::new(std::ffi::OsStr::from_bytes(&bytes))
        };
        #[cfg(not(unix))]
        let path = Path::new(
            std::str::from_utf8(&bytes).map_err(|_| GitError::InvalidInput {
                message: "invalid conflict path".into(),
            })?,
        );
        let full_path = raw
            .workdir()
            .ok_or_else(|| GitError::InvalidInput {
                message: "bare repository".into(),
            })?
            .join(path);
        if full_path.symlink_metadata().is_ok() {
            if fs::read(&full_path).is_ok_and(|contents| {
                contents
                    .split(|byte| *byte == b'\n')
                    .any(|line| line.starts_with(b"<<<<<<< "))
            }) {
                return Err(GitError::MergeConflict);
            }
            index.add_path(path)?;
        } else {
            index.remove_path(path)?;
        }
    }
    index.write()?;
    Ok(())
}

pub fn abort_in_progress_commit_action(
    repo: &Repository,
    kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    require_commit_action(repo, kind)?;
    let raw = repo.inner.write().unwrap();
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    raw.checkout_head(Some(&mut checkout))?;
    raw.cleanup_state()?;
    Ok(())
}

/// Reset mode matching IDEA's GitNewResetDialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Keep changes in working tree and index
    Soft,
    /// Keep changes in working tree, unstage from index (default)
    Mixed,
    /// Discard all changes
    Hard,
}

impl ResetMode {
    pub fn git_flag(&self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ResetMode::Soft => "Soft",
            ResetMode::Mixed => "Mixed",
            ResetMode::Hard => "Hard",
        }
    }

    fn reset_type(self) -> git2::ResetType {
        match self {
            ResetMode::Soft => git2::ResetType::Soft,
            ResetMode::Mixed => git2::ResetType::Mixed,
            ResetMode::Hard => git2::ResetType::Hard,
        }
    }
}

pub fn reset_current_branch_to_commit(
    repo: &Repository,
    commit_id: &str,
    mode: ResetMode,
) -> Result<(), GitError> {
    info!(
        "Resetting current branch to '{}' (mode: {:?})",
        commit_id, mode
    );

    if mode == ResetMode::Hard {
        ensure_clean_worktree(repo, "reset")?;
    }

    let current_branch = repo
        .current_branch()
        .map_err(|e| GitError::OperationFailed {
            operation: "reset".to_string(),
            details: e.to_string(),
        })?;
    if current_branch.is_none() {
        return Err(GitError::OperationFailed {
            operation: "reset".to_string(),
            details: "当前为 detached HEAD，无法重置当前分支".to_string(),
        });
    }

    let head_oid = current_head_oid(repo, "reset")?;
    let target_oid = resolve_commit_oid(repo, commit_id, "reset")?;
    if !is_ancestor(repo, target_oid, head_oid)? {
        return Err(GitError::OperationFailed {
            operation: "reset".to_string(),
            details: "只能把当前分支重置到它自己的历史祖先提交".to_string(),
        });
    }

    let repo_lock = repo.inner.write().unwrap();
    let target = repo_lock
        .find_commit(target_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: "reset".to_string(),
            details: e.to_string(),
        })?;
    repo_lock
        .reset(target.as_object(), mode.reset_type(), None)
        .map_err(|e| GitError::OperationFailed {
            operation: "reset".to_string(),
            details: e.to_string(),
        })
}

pub fn resolve_push_current_branch_target(
    repo: &Repository,
    commit_id: &str,
) -> Result<PushCurrentBranchTarget, GitError> {
    info!("Resolving push-to-here target for '{}'", commit_id);

    let local_branch_name = repo
        .current_branch()
        .map_err(|e| GitError::OperationFailed {
            operation: "push-to-here".to_string(),
            details: e.to_string(),
        })?
        .ok_or_else(|| GitError::OperationFailed {
            operation: "push-to-here".to_string(),
            details: "当前为 detached HEAD，无法解析当前分支的远端目标".to_string(),
        })?;
    let upstream_ref = repo
        .current_upstream_ref()
        .ok_or_else(|| GitError::OperationFailed {
            operation: "push-to-here".to_string(),
            details: format!("当前分支 {local_branch_name} 还没有配置上游"),
        })?;
    let (remote_name, upstream_branch_name) =
        parse_upstream_ref(&upstream_ref).ok_or_else(|| GitError::OperationFailed {
            operation: "push-to-here".to_string(),
            details: format!("无法解析上游分支 {upstream_ref}"),
        })?;
    let remote_name = remote_name.to_string();
    let upstream_branch_name = upstream_branch_name.to_string();

    let head_oid = current_head_oid(repo, "push-to-here")?;
    let selected_oid = resolve_commit_oid(repo, commit_id, "push-to-here")?;
    if !is_ancestor(repo, selected_oid, head_oid)? {
        return Err(GitError::OperationFailed {
            operation: "push-to-here".to_string(),
            details: "只能把当前分支的远端发布到当前分支历史上的祖先提交".to_string(),
        });
    }

    let upstream_oid = resolve_commit_oid(repo, &upstream_ref, "push-to-here")?;
    let is_fast_forward = is_ancestor(repo, upstream_oid, selected_oid)?;

    Ok(PushCurrentBranchTarget {
        remote_name,
        local_branch_name,
        upstream_ref,
        upstream_branch_name,
        selected_commit: selected_oid.to_string(),
        expected_remote_oid: upstream_oid.to_string(),
        is_fast_forward,
        requires_force_with_lease: !is_fast_forward,
    })
}

pub fn push_current_branch_to_commit(
    repo: &Repository,
    target: &PushCurrentBranchTarget,
) -> Result<(), GitError> {
    info!(
        "Pushing current branch '{}' to '{}' at '{}'",
        target.local_branch_name, target.upstream_ref, target.selected_commit
    );

    ensure_no_in_progress_operation(repo, "push-to-here")?;

    let selected_oid = resolve_commit_oid(repo, &target.selected_commit, "push-to-here")?;
    let refspec = format!(
        "{}:refs/heads/{}",
        selected_oid, target.upstream_branch_name
    );

    let refspec = if target.requires_force_with_lease {
        format!("+{refspec}")
    } else {
        refspec
    };
    crate::remote::push_refspecs(
        repo,
        &target.remote_name,
        &[refspec],
        Some((
            &format!("refs/heads/{}", target.upstream_branch_name),
            git2::Oid::from_str(&target.expected_remote_oid)?,
        )),
        None,
    )
}

/// Uncommit: soft-reset from HEAD to the parent of the given commit.
/// All changes from the removed commits are returned to the staging area.
/// Equivalent to IDEA's "Uncommit" action.
pub fn uncommit_to_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    info!(
        "Uncommitting from HEAD to commit {} (soft reset to parent)",
        commit_id
    );

    let repo_lock = repo.inner.write().unwrap();
    let commit = repo_lock
        .revparse_single(commit_id)
        .and_then(|obj| obj.peel_to_commit())
        .map_err(|e| GitError::OperationFailed {
            operation: "uncommit_to_commit".to_string(),
            details: e.to_string(),
        })?;
    let parent = commit.parent(0).map_err(|e| GitError::OperationFailed {
        operation: "uncommit_to_commit".to_string(),
        details: e.to_string(),
    })?;
    repo_lock
        .reset(parent.as_object(), git2::ResetType::Soft, None)
        .map_err(|e| GitError::OperationFailed {
            operation: "uncommit_to_commit".to_string(),
            details: e.to_string(),
        })?;

    info!("Uncommit completed — changes returned to staging area");
    Ok(())
}
