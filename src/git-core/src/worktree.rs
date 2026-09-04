//! Working tree management for git-core

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
    info!("Listing worktrees");
    let repo_lock = repo.inner.read().unwrap();
    let mut worktrees = Vec::new();

    let main_path = repo_lock
        .workdir()
        .unwrap_or_else(|| repo_lock.path())
        .to_path_buf();
    let main_branch = repo_lock
        .head()
        .ok()
        .and_then(|head| head.shorthand().map(str::to_string));
    worktrees.push(WorkingTree {
        name: main_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main")
            .to_string(),
        path: main_path,
        branch: main_branch,
        is_main: true,
        is_locked: false,
        is_valid: true,
    });

    if let Ok(list) = repo_lock.worktrees() {
        for name in list.iter().flatten() {
            let Ok(worktree) = repo_lock.find_worktree(name) else {
                continue;
            };
            let path = worktree.path().to_path_buf();
            worktrees.push(WorkingTree {
                name: name.to_string(),
                path: path.clone(),
                branch: None,
                is_main: false,
                is_locked: matches!(
                    worktree.is_locked(),
                    Ok(git2::WorktreeLockStatus::Locked(_))
                ),
                is_valid: path.exists(),
            });
        }
    }

    info!("Found {} worktrees", worktrees.len());
    Ok(worktrees)
}

/// Create a new worktree
pub fn create_worktree(
    repo: &Repository,
    path: &Path,
    branch: Option<&str>,
) -> Result<WorkingTree, GitError> {
    info!("Creating worktree at {:?} with branch {:?}", path, branch);
    let repo_lock = repo.inner.write().unwrap();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worktree")
        .to_string();

    let mut opts = git2::WorktreeAddOptions::new();
    let checkout_ref = branch.and_then(|branch_name| {
        repo_lock
            .find_reference(&format!("refs/heads/{branch_name}"))
            .ok()
    });
    if let Some(reference) = checkout_ref.as_ref() {
        opts.reference(Some(reference));
    }

    repo_lock
        .worktree(&name, path, Some(&opts))
        .map_err(|e| GitError::OperationFailed {
            operation: "create_worktree".to_string(),
            details: e.to_string(),
        })?;

    Ok(WorkingTree {
        name,
        path: path.to_path_buf(),
        branch: branch.map(str::to_string),
        is_main: false,
        is_locked: false,
        is_valid: true,
    })
}

/// Remove a worktree
pub fn remove_worktree(repo: &Repository, path: &Path) -> Result<(), GitError> {
    info!("Removing worktree at {:?}", path);
    let repo_lock = repo.inner.write().unwrap();
    let list = repo_lock.worktrees().map_err(|e| GitError::OperationFailed {
        operation: "remove_worktree".to_string(),
        details: e.to_string(),
    })?;
    for name in list.iter().flatten() {
        let Ok(worktree) = repo_lock.find_worktree(name) else {
            continue;
        };
        if same_worktree_path(worktree.path(), path) {
            let mut opts = git2::WorktreePruneOptions::new();
            opts.valid(true);
            opts.working_tree(true);
            worktree
                .prune(Some(&mut opts))
                .map_err(|e| GitError::OperationFailed {
                    operation: "remove_worktree".to_string(),
                    details: e.to_string(),
                })?;
            return Ok(());
        }
    }
    Err(GitError::OperationFailed {
        operation: "remove_worktree".to_string(),
        details: format!("worktree not found: {}", path.display()),
    })
}

fn same_worktree_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.file_name() == right.file_name(),
    }
}
