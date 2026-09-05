//! MAS commit actions implemented without a Git subprocess.

use super::*;
use crate::native::{journal::Kind, sequencer};

pub(super) fn export_commit_patch(
    repo: &Repository,
    commit_id: &str,
    output_path: &Path,
) -> Result<(), GitError> {
    let bytes = {
        let raw = repo.inner.read().unwrap();
        let commit = raw.revparse_single(commit_id)?.peel_to_commit()?;
        git2::Email::from_commit(&commit, &mut git2::EmailCreateOptions::new())?
            .as_slice()
            .to_vec()
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    // NSSavePanel authorizes this file, not arbitrary sibling temporary files.
    // Generate the complete email before opening the selected destination.
    use std::io::Write;
    let mut file = fs::File::create(output_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn start(repo: &Repository, commit_id: &str, kind: Kind) -> Result<(), GitError> {
    let (head, commit) = {
        let raw = repo.inner.read().unwrap();
        (
            raw.head()?.peel_to_commit()?.id(),
            raw.revparse_single(commit_id)?.peel_to_commit()?.id(),
        )
    };
    let complete = sequencer::start(
        repo,
        kind,
        Some(head.to_string()),
        &[crate::RebaseTodoEntry {
            action: "pick".into(),
            commit: commit.to_string(),
            message: String::new(),
        }],
        false,
    )?;
    if complete {
        Ok(())
    } else {
        Err(GitError::OperationFailed {
            operation: if kind == Kind::CherryPick {
                "cherry-pick"
            } else {
                "revert"
            }
            .into(),
            details: "存在冲突，请解决后继续，或中止当前操作".into(),
        })
    }
}

pub(super) fn cherry_pick_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    start(repo, commit_id, Kind::CherryPick)
}
pub(super) fn revert_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    start(repo, commit_id, Kind::Revert)
}

pub(super) fn run_scripted_interactive_rebase(
    repo: &Repository,
    _operation: &str,
    base_spec: Option<&str>,
    todo_contents: &str,
    _auto_accept_editor: bool,
) -> Result<RewriteExecution, GitError> {
    let entries: Vec<_> = todo_contents
        .lines()
        .map(|line| {
            let mut parts = line.splitn(3, ' ');
            crate::RebaseTodoEntry {
                action: parts.next().unwrap_or_default().into(),
                commit: parts.next().unwrap_or_default().into(),
                message: parts.next().unwrap_or_default().into(),
            }
        })
        .collect();
    crate::rebase::start_interactive_rebase(repo, base_spec, &entries)?;
    Ok(if crate::native::active(repo) {
        RewriteExecution::InProgress
    } else {
        RewriteExecution::Completed
    })
}

pub(super) fn continue_in_progress_commit_action(
    _repo: &Repository,
    _kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    Err(GitError::RecoveryRequired {
        reason: "continue the existing operation in the tool that started it".into(),
    })
}
pub(super) fn abort_in_progress_commit_action(
    _repo: &Repository,
    _kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    Err(GitError::RecoveryRequired {
        reason: "abort the existing operation in the tool that started it".into(),
    })
}

pub(super) fn reset(repo: &Repository, commit_id: &str, mode: ResetMode) -> Result<(), GitError> {
    let raw = repo.inner.write().unwrap();
    let object = raw.revparse_single(commit_id)?;
    let kind = match mode {
        ResetMode::Soft => git2::ResetType::Soft,
        ResetMode::Mixed => git2::ResetType::Mixed,
        ResetMode::Hard => git2::ResetType::Hard,
    };
    Ok(raw.reset(&object, kind, None)?)
}

pub(super) fn push_current_branch_to_commit(
    repo: &Repository,
    target: &PushCurrentBranchTarget,
) -> Result<(), GitError> {
    ensure_no_in_progress_operation(repo, "push-to-here")?;
    crate::remote::push_selected_commit(repo, target)
}
