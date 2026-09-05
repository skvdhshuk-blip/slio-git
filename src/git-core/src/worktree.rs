//! Working tree management for git-core

#[cfg_attr(feature = "app-store", path = "worktree/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "worktree/desktop.rs")]
mod backend;

use crate::error::GitError;
use crate::repository::Repository;
use log::info;
use std::path::{Path, PathBuf};

/// A linked git worktree
#[derive(Debug, Clone)]
pub struct WorkingTree {
    /// Worktree name
    pub name: String,
    /// Absolute path to worktree directory
    pub path: PathBuf,
    /// Branch checked out in worktree
    pub branch: Option<String>,
    /// Whether this is the main worktree
    pub is_main: bool,
    /// Whether the worktree is locked
    pub is_locked: bool,
    /// Whether the worktree path exists and is valid
    pub is_valid: bool,
}

/// List all worktrees for the repository
pub fn list_worktrees(repo: &Repository) -> Result<Vec<WorkingTree>, GitError> {
    backend::list_worktrees(repo)
}

/// Create a new worktree
pub fn create_worktree(
    repo: &Repository,
    path: &Path,
    branch: Option<&str>,
) -> Result<WorkingTree, GitError> {
    crate::native::reject_active(repo)?;
    backend::create_worktree(repo, path, branch)
}

/// Remove a worktree
pub fn remove_worktree(repo: &Repository, path: &Path) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::remove_worktree(repo, path)
}
