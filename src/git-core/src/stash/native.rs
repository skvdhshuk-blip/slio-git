//! Native execution for stash.

use super::*;
use crate::commit::get_default_signature;
use git2::StashFlags;

pub(super) fn list_stashes(repo: &Repository) -> Result<Vec<StashInfo>, GitError> {
    info!("Listing all stashes");

    let mut repo_lock = repo.inner.write().unwrap();
    let mut stashes = Vec::new();
    repo_lock
        .stash_foreach(|index, message, oid| {
            let branch = message
                .split(':')
                .nth(1)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            stashes.push(StashInfo {
                index: index as u32,
                message: message.to_string(),
                branch,
                oid: oid.to_string(),
                timestamp: None,
                includes_untracked: message.contains("untracked"),
            });
            true
        })
        .map_err(|e| GitError::OperationFailed {
            operation: "list_stashes".to_string(),
            details: e.to_string(),
        })?;
    for stash in &mut stashes {
        let commit = repo_lock.find_commit(git2::Oid::from_str(&stash.oid)?)?;
        stash.timestamp = Some(commit.time().seconds());
        stash.includes_untracked = commit.parent_count() > 2;
        stash.branch = stash
            .message
            .split_once(": ")
            .map(|(prefix, _)| {
                prefix
                    .trim_start_matches("WIP on ")
                    .trim_start_matches("On ")
                    .to_string()
            })
            .unwrap_or_default();
    }
    Ok(stashes)
}

pub(super) fn stash_save_with_options(
    repo: &Repository,
    message: Option<&str>,
    include_untracked: bool,
    keep_index: bool,
) -> Result<String, GitError> {
    info!(
        "Saving changes to stash (include_untracked={}, keep_index={})",
        include_untracked, keep_index
    );

    let signature = get_default_signature(repo)?;
    let mut flags = StashFlags::empty();
    if include_untracked {
        flags |= StashFlags::INCLUDE_UNTRACKED;
    }
    if keep_index {
        flags |= StashFlags::KEEP_INDEX;
    }

    let mut repo_lock = repo.inner.write().unwrap();
    let oid = repo_lock
        .stash_save(&signature, message.unwrap_or("WIP"), Some(flags))
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_save".to_string(),
            details: e.to_string(),
        })?;
    Ok(oid.to_string())
}

pub(super) fn stash_pop(repo: &Repository, index: u32) -> Result<(), GitError> {
    info!("Applying stash@{{{}}}", index);
    let mut repo_lock = repo.inner.write().unwrap();
    repo_lock
        .stash_pop(index as usize, None)
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_pop".to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

pub(super) fn stash_drop(repo: &Repository, index: u32) -> Result<(), GitError> {
    info!("Dropping stash@{{{}}}", index);
    let mut repo_lock = repo.inner.write().unwrap();
    repo_lock
        .stash_drop(index as usize)
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_drop".to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

pub(super) fn stash_apply(repo: &Repository, index: u32) -> Result<(), GitError> {
    info!("Applying stash@{{{}}} (without pop)", index);
    let mut repo_lock = repo.inner.write().unwrap();
    repo_lock
        .stash_apply(index as usize, None)
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_apply".to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

pub(super) fn stash_diff(repo: &Repository, index: u32) -> Result<String, GitError> {
    info!("Getting diff for stash@{{{}}}", index);
    let repo_lock = repo.inner.read().unwrap();
    let commit = repo_lock
        .revparse_single(&format!("stash@{{{index}}}"))
        .and_then(|object| object.peel_to_commit())
        .map_err(|_| GitError::StashNotFound { index })?;
    let parent = commit.parent(0).map_err(|e| GitError::OperationFailed {
        operation: "stash_diff".to_string(),
        details: e.to_string(),
    })?;
    let old_tree = parent.tree()?;
    let new_tree = commit.tree()?;
    let diff = repo_lock.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;
    let mut out = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        if let Ok(text) = std::str::from_utf8(line.content()) {
            out.push_str(text);
        }
        true
    })?;
    Ok(out)
}

pub(super) fn unstash_as_branch(
    repo: &Repository,
    index: u32,
    branch_name: &str,
) -> Result<(), GitError> {
    info!(
        "Applying stash@{{{}}} to new branch '{}'",
        index, branch_name
    );

    if repo.has_uncommitted_changes() {
        return Err(GitError::DirtyWorkingTree);
    }
    let mut repo_lock = repo.inner.write().unwrap();
    {
        let commit = repo_lock
            .revparse_single(&format!("stash@{{{index}}}"))
            .and_then(|object| object.peel_to_commit())
            .map_err(|_| GitError::StashNotFound { index })?;
        let base = commit.parent(0).map_err(|e| GitError::OperationFailed {
            operation: "unstash_as_branch".to_string(),
            details: e.to_string(),
        })?;
        repo_lock.branch(branch_name, &base, false)?;
    }
    let reference = format!("refs/heads/{branch_name}");
    let object = repo_lock.revparse_single(&reference)?;
    repo_lock.checkout_tree(
        &object,
        Some(git2::build::CheckoutBuilder::default().safe()),
    )?;
    drop(object);
    repo_lock.set_head(&reference)?;
    repo_lock
        .stash_pop(index as usize, None)
        .map_err(|e| GitError::OperationFailed {
            operation: "unstash_as_branch".to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

pub(super) fn stash_clear(repo: &Repository) -> Result<(), GitError> {
    info!("Clearing all stashes");
    let mut repo_lock = repo.inner.write().unwrap();
    loop {
        match repo_lock.stash_drop(0) {
            Ok(()) => {}
            Err(error) if error.code() == git2::ErrorCode::NotFound => break,
            Err(error) => {
                return Err(GitError::OperationFailed {
                    operation: "stash_clear".to_string(),
                    details: error.to_string(),
                });
            }
        }
    }
    Ok(())
}
