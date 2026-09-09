//! Desktop execution for branch.

use super::*;
use crate::index;
use crate::process::git_command;

pub(super) fn find_merged_branches(
    repo: &Repository,
    target: &str,
) -> Result<Vec<String>, GitError> {
    info!("Finding branches merged into '{}'", target);

    let repo_path = repo.command_cwd();
    let output = git_command()
        .args(["branch", "--merged", target])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "find_merged_branches".to_string(),
            details: format!("Failed to execute git branch --merged: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "find_merged_branches".to_string(),
            details: format!(
                "git branch --merged failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let branches: Vec<String> = output_str
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            // Skip the current branch marker "* " prefix
            let name = trimmed.strip_prefix("* ").unwrap_or(trimmed);
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(name.to_string())
        })
        .collect();

    info!("Found {} branches merged into '{}'", branches.len(), target);
    Ok(branches)
}

pub(super) fn create_branch_from_start_point(
    repo: &Repository,
    name: &str,
    start_point: &str,
) -> Result<Branch, GitError> {
    info!("Creating branch '{}' from '{}'", name, start_point);

    let repo_path = repo.command_cwd();

    // Use git branch command
    let output = git_command()
        .args(["branch", name, start_point])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "create_branch".to_string(),
            details: format!("Failed to execute git branch: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "create_branch".to_string(),
            details: format!(
                "git branch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(Branch {
        name: name.to_string(),
        oid: String::new(),
        is_remote: false,
        is_head: false,
        upstream: None,
        tracking_status: None,
        sync_hint: None,
        recency_hint: None,
        last_commit_timestamp: None,
        group_path: None,
    })
}

pub(super) fn delete_branch(repo: &Repository, name: &str, force: bool) -> Result<(), GitError> {
    info!("Deleting branch '{}'", name);

    let repo_path = repo.command_cwd();

    // Only an explicit confirmation permits deleting an unmerged branch.
    let output = git_command()
        .args(["branch", if force { "-D" } else { "-d" }, "--", name])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "delete_branch".to_string(),
            details: format!("Failed to execute git branch: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "delete_branch".to_string(),
            details: format!(
                "git branch deletion failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}

pub(super) fn rename_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<Branch, GitError> {
    info!("Renaming branch '{}' to '{}'", old_name, new_name);

    let repo_path = repo.command_cwd();

    // Use git branch -m command
    let output = git_command()
        .args(["branch", "-m", old_name, new_name])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "rename_branch".to_string(),
            details: format!("Failed to execute git branch: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "rename_branch".to_string(),
            details: format!(
                "git branch -m failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(Branch {
        name: new_name.to_string(),
        oid: String::new(),
        is_remote: false,
        is_head: false,
        upstream: None,
        tracking_status: None,
        sync_hint: None,
        recency_hint: None,
        last_commit_timestamp: None,
        group_path: None,
    })
}

pub(super) fn set_branch_upstream(
    repo: &Repository,
    branch_name: &str,
    upstream: &str,
) -> Result<(), GitError> {
    info!(
        "Setting upstream of branch '{}' to '{}'",
        branch_name, upstream
    );

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["branch", "--set-upstream-to", upstream, branch_name])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "set_branch_upstream".to_string(),
            details: format!("Failed to execute git branch: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "set_branch_upstream".to_string(),
            details: format!(
                "git branch --set-upstream-to failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}

pub(super) fn list_uncommitted_files(repo: &Repository) -> Vec<String> {
    let repo_path = repo.command_cwd();
    git_command()
        .args(["status", "--porcelain"])
        .current_dir(&repo_path)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|line| {
                    // porcelain format: "XY filename" — skip first 3 chars
                    line.get(3..).map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn has_uncommitted_changes(repo: &Repository) -> bool {
    let repo_path = repo.command_cwd();
    // `git status --porcelain` outputs nothing when clean
    git_command()
        .args(["status", "--porcelain"])
        .current_dir(&repo_path)
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

pub(super) fn force_checkout_branch(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!("Force checking out branch '{}' (discarding changes)", name);
    let repo_path = repo.command_cwd();
    let output = git_command()
        .args(["checkout", "--force", name])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "force_checkout_branch".to_string(),
            details: format!("Failed to execute git checkout --force: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "force_checkout_branch".to_string(),
            details: format!(
                "git checkout --force failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

pub(super) fn smart_checkout_branch(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!(
        "Smart checking out branch '{}' (stash → checkout → unstash)",
        name
    );
    let repo_path = repo.command_cwd();

    // Step 1: stash including untracked files
    let stash_output = git_command()
        .args([
            "stash",
            "push",
            "-m",
            "slio-git: smart checkout auto-stash",
            "--include-untracked",
        ])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "smart_checkout_branch.stash".to_string(),
            details: format!("Failed to stash: {}", e),
        })?;

    let stash_created = stash_output.status.success()
        && !String::from_utf8_lossy(&stash_output.stdout).contains("No local changes");

    // Step 2: checkout
    let checkout_result = repo.checkout_branch(name);

    // Step 3: if stash was created, pop it regardless of checkout result
    if stash_created {
        let pop_output = git_command()
            .args(["stash", "pop"])
            .current_dir(&repo_path)
            .output();

        if let Err(e) = &pop_output {
            info!("Warning: stash pop failed after smart checkout: {}", e);
        } else if let Ok(output) = &pop_output {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                info!("Warning: stash pop had conflicts: {}", stderr);
                // Conflicts from stash pop are expected — user will see them in the UI
            }
        }
    }

    checkout_result
}

pub(super) fn checkout_branch(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!("Checking out branch '{}'", name);

    let repo_path = repo.command_cwd();

    // Use git checkout command
    let output = git_command()
        .args(["checkout", name])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_branch".to_string(),
            details: format!("Failed to execute git checkout: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "checkout_branch".to_string(),
            details: format!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}

pub(super) fn checkout_remote_branch(
    repo: &Repository,
    remote_ref: &str,
) -> Result<String, GitError> {
    info!("Checking out remote branch '{}'", remote_ref);

    let Some((_, local_branch_name)) = remote_ref.split_once('/') else {
        return Err(GitError::OperationFailed {
            operation: "checkout_remote_branch".to_string(),
            details: format!("invalid remote branch ref: {remote_ref}"),
        });
    };

    let repo_path = repo.command_cwd();
    let local_ref = format!("refs/heads/{local_branch_name}");
    let local_branch_exists = git_command()
        .args(["show-ref", "--verify", "--quiet", &local_ref])
        .current_dir(&repo_path)
        .status()
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_remote_branch".to_string(),
            details: format!("Failed to inspect local branch refs: {}", e),
        })?
        .success();

    let args = if local_branch_exists {
        vec!["checkout", local_branch_name]
    } else {
        vec!["checkout", "--track", remote_ref]
    };

    let output = git_command()
        .args(&args)
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_remote_branch".to_string(),
            details: format!("Failed to execute git checkout: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "checkout_remote_branch".to_string(),
            details: format!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(local_branch_name.to_string())
}

pub(super) fn merge_branch(repo: &Repository, branch_name: &str) -> Result<(), GitError> {
    info!("Merging branch '{}' into current branch", branch_name);

    let repo_path = repo.command_cwd();

    // Use git merge command
    let output = git_command()
        .args(["merge", branch_name])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "merge_branch".to_string(),
            details: format!("Failed to execute git merge: {}", e),
        })?;

    if !output.status.success() {
        if index::has_conflicts(repo) {
            return Err(GitError::MergeConflict);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "git merge 返回失败，但没有输出可读错误详情".to_string()
        };

        return Err(GitError::OperationFailed {
            operation: "merge_branch".to_string(),
            details: format!("git merge failed: {details}"),
        });
    }

    Ok(())
}

pub(super) fn list_branches(repo: &Repository) -> Result<Vec<Branch>, GitError> {
    info!("Listing all branches");

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)\t%(objectname:short)\t%(upstream:short)\t%(upstream:trackshort)\t%(committerdate:unix)\t%(HEAD)",
            "refs/heads",
        ])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "list_branches".to_string(),
            details: format!("Failed to execute git branch: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "list_branches".to_string(),
            details: format!(
                "git branch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();
    let current_branch = repo.current_branch().ok().flatten().unwrap_or_default();

    for line in output_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        let name = parts.first().copied().unwrap_or("").trim().to_string();
        let oid = parts.get(1).copied().unwrap_or("").trim().to_string();
        let upstream = parts
            .get(2)
            .copied()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let tracking_status = normalize_tracking_status(parts.get(3).copied().unwrap_or(""));
        let last_commit_timestamp = parts
            .get(4)
            .copied()
            .and_then(|value| value.trim().parse::<i64>().ok());
        let is_head = parts
            .get(5)
            .copied()
            .map(str::trim)
            .is_some_and(|value| value == "*")
            || name == current_branch;

        if name.is_empty() {
            continue;
        }

        let tracking_status = if is_head {
            current_branch_tracking(repo)
        } else {
            tracking_status
        };
        let sync_hint = compact_branch_sync_hint(upstream.as_deref(), tracking_status.as_deref());

        let mut branch = Branch {
            name: name.clone(),
            oid,
            is_remote: false,
            is_head,
            upstream,
            sync_hint,
            recency_hint: compact_relative_time(last_commit_timestamp),
            tracking_status,
            last_commit_timestamp,
            group_path: None,
        };
        branch.compute_group_path();
        branches.push(branch);
    }

    let remote_output = git_command()
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)\t%(symref)\t%(objectname:short)\t%(committerdate:unix)",
            "refs/remotes",
        ])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "list_branches".to_string(),
            details: format!("Failed to execute git branch -r: {}", e),
        })?;

    if remote_output.status.success() {
        let remote_str = String::from_utf8_lossy(&remote_output.stdout);
        for line in remote_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.contains("->") {
                continue;
            }

            let parts: Vec<&str> = line.split('\t').collect();
            let name = parts.first().copied().unwrap_or("").trim().to_string();
            let symref = parts.get(1).copied().unwrap_or("").trim();
            let oid = parts.get(2).copied().unwrap_or("").trim().to_string();
            let last_commit_timestamp = parts
                .get(3)
                .copied()
                .and_then(|value| value.trim().parse::<i64>().ok());

            if !name.is_empty() && symref.is_empty() && !name.ends_with("/HEAD") {
                let mut branch = Branch {
                    name,
                    oid,
                    is_remote: true,
                    is_head: false,
                    upstream: None,
                    tracking_status: None,
                    sync_hint: None,
                    recency_hint: compact_relative_time(last_commit_timestamp),
                    last_commit_timestamp,
                    group_path: None,
                };
                branch.compute_group_path();
                branches.push(branch);
            }
        }
    }

    Ok(branches)
}

fn current_branch_tracking(repo: &Repository) -> Option<String> {
    match repo.sync_status() {
        SyncStatus::Ahead(count) => Some(format!("↑{count}")),
        SyncStatus::Behind(count) => Some(format!("↓{count}")),
        SyncStatus::Diverged { ahead, behind } => Some(format!("↕{ahead}/{behind}")),
        SyncStatus::Synced => Some("✓".to_string()),
        SyncStatus::NoUpstream => None,
        SyncStatus::Unknown => Some("?".to_string()),
    }
}

fn normalize_tracking_status(raw: &str) -> Option<String> {
    match raw.trim() {
        ">" => Some("↑".to_string()),
        "<" => Some("↓".to_string()),
        "<>" => Some("↕".to_string()),
        "=" => Some("✓".to_string()),
        "" => None,
        other => Some(other.to_string()),
    }
}
