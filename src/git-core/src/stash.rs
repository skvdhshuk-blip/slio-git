//! Stash operations for git-core

#[cfg_attr(feature = "app-store", path = "stash/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "stash/desktop.rs")]
mod backend;

use crate::error::GitError;
use crate::repository::Repository;
use log::info;

/// A Git stash
#[derive(Debug, Clone)]
pub struct StashInfo {
    pub index: u32,
    pub message: String,
    pub branch: String,
    pub oid: String,
    /// Timestamp when the stash was created
    pub timestamp: Option<i64>,
    /// Whether untracked files were included in this stash
    pub includes_untracked: bool,
}

/// List all stashes
pub fn list_stashes(repo: &Repository) -> Result<Vec<StashInfo>, GitError> {
    backend::list_stashes(repo)
}

/// Save current changes to stash with optional include-untracked flag
pub fn stash_save_with_options(
    repo: &Repository,
    message: Option<&str>,
    include_untracked: bool,
    keep_index: bool,
) -> Result<String, GitError> {
    crate::native::reject_active(repo)?;
    backend::stash_save_with_options(repo, message, include_untracked, keep_index)
}

/// Save current changes to stash (convenience wrapper)
pub fn stash_save(repo: &Repository, message: Option<&str>) -> Result<String, GitError> {
    stash_save_with_options(repo, message, false, false)
}

/// Apply a stash
pub fn stash_pop(repo: &Repository, index: u32) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::stash_pop(repo, index)
}

/// Drop a stash
pub fn stash_drop(repo: &Repository, index: u32) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::stash_drop(repo, index)
}

/// Apply a stash without removing it from the stash list
pub fn stash_apply(repo: &Repository, index: u32) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::stash_apply(repo, index)
}

/// Get the diff contents of a stash for preview
pub fn stash_diff(repo: &Repository, index: u32) -> Result<String, GitError> {
    backend::stash_diff(repo, index)
}

/// Apply a stash to a new branch (git stash branch <name> stash@{N}).
pub fn unstash_as_branch(repo: &Repository, index: u32, branch_name: &str) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::unstash_as_branch(repo, index, branch_name)
}

/// Clear all stashes (git stash clear)
pub fn stash_clear(repo: &Repository) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::stash_clear(repo)
}
