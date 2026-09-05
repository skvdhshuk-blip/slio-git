//! Native execution for worktree.

use super::*;

pub(super) fn list_worktrees(repo: &Repository) -> Result<Vec<WorkingTree>, GitError> {
    info!("Listing worktrees");
    let repo_lock = repo.inner.read().unwrap();
    let mut worktrees = Vec::new();

    let common = match std::fs::read_to_string(repo_lock.path().join("commondir")) {
        Ok(relative) => repo_lock.path().join(relative.trim()).canonicalize()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            repo_lock.path().to_path_buf()
        }
        Err(error) => return Err(error.into()),
    };
    let main = git2::Repository::open(&common)?;
    let main_path = main.workdir().unwrap_or(main.path()).to_path_buf();
    let main_branch = main
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
                branch: git2::Repository::open_from_worktree(&worktree)
                    .ok()
                    .and_then(|raw| {
                        raw.head().ok().and_then(|head| {
                            if head.is_branch() {
                                head.shorthand().map(str::to_owned)
                            } else {
                                None
                            }
                        })
                    }),
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

pub(super) fn create_worktree(
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
    let checkout_ref = branch
        .map(|branch_name| repo_lock.find_reference(&format!("refs/heads/{branch_name}")))
        .transpose()?;
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

pub(super) fn remove_worktree(repo: &Repository, path: &Path) -> Result<(), GitError> {
    info!("Removing worktree at {:?}", path);
    let repo_lock = repo.inner.write().unwrap();
    let list = repo_lock
        .worktrees()
        .map_err(|e| GitError::OperationFailed {
            operation: "remove_worktree".to_string(),
            details: e.to_string(),
        })?;
    for name in list.iter().flatten() {
        let Ok(worktree) = repo_lock.find_worktree(name) else {
            continue;
        };
        if same_worktree_path(worktree.path(), path) {
            if matches!(worktree.is_locked()?, git2::WorktreeLockStatus::Locked(_)) {
                return Err(GitError::OperationFailed {
                    operation: "remove_worktree".into(),
                    details: "worktree is locked".into(),
                });
            }
            if path.exists() {
                let linked = git2::Repository::open(path)?;
                let mut status = git2::StatusOptions::new();
                status
                    .include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .include_ignored(true)
                    .recurse_ignored_dirs(true);
                if !linked.statuses(Some(&mut status))?.is_empty()
                    || linked.state() != git2::RepositoryState::Clean
                    || crate::native::state(&linked).is_some()
                {
                    return Err(GitError::DirtyWorkingTree);
                }
            }
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
        _ => false,
    }
}
