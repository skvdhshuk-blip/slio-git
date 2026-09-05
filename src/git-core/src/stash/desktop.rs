//! Desktop execution for stash.

use super::*;
use crate::process::git_command;

pub(super) fn list_stashes(repo: &Repository) -> Result<Vec<StashInfo>, GitError> {
    info!("Listing all stashes");

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["stash", "list", "--format=full"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "list_stashes".to_string(),
            details: format!("Failed to execute git stash list: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "list_stashes".to_string(),
            details: format!(
                "git stash list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut stashes = Vec::new();

    for (i, line) in output_str.lines().enumerate() {
        // Format: stash@{index}: BranchName: message
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        let index = i as u32;
        let (branch, message) = if parts.len() >= 3 {
            (parts[1].trim().to_string(), parts[2].trim().to_string())
        } else if parts.len() == 2 {
            (parts[0].to_string(), parts[1].trim().to_string())
        } else {
            (String::new(), line.to_string())
        };

        // Extract OID from the stash reference if possible
        let oid = format!("stash@{{{}}}", index);

        stashes.push(StashInfo {
            index,
            message,
            branch,
            oid,
            timestamp: None, // Populated below if available
            includes_untracked: false,
        });
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

    let repo_path = repo.command_cwd();

    let mut args = vec!["stash".to_string(), "push".to_string()];
    if include_untracked {
        args.push("--include-untracked".to_string());
    }
    if keep_index {
        args.push("--keep-index".to_string());
    }
    if let Some(msg) = message {
        args.push("-m".to_string());
        args.push(msg.to_string());
    }

    let output = git_command()
        .args(&args)
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_save".to_string(),
            details: format!("Failed to execute git stash: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "stash_save".to_string(),
            details: format!(
                "git stash failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let stash_ref = output_str
        .lines()
        .find(|l| l.contains("stash@"))
        .map(|l| l.to_string())
        .unwrap_or_else(|| "stash@{0}".to_string());

    info!("Changes saved to {}", stash_ref);
    Ok(stash_ref)
}

pub(super) fn stash_pop(repo: &Repository, index: u32) -> Result<(), GitError> {
    info!("Applying stash@{{{}}}", index);

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["stash", "pop", &format!("stash@{{{}}}", index)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_pop".to_string(),
            details: format!("Failed to execute git stash pop: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "stash_pop".to_string(),
            details: format!(
                "git stash pop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Stash applied successfully");
    Ok(())
}

pub(super) fn stash_drop(repo: &Repository, index: u32) -> Result<(), GitError> {
    info!("Dropping stash@{{{}}}", index);

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["stash", "drop", &format!("stash@{{{}}}", index)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_drop".to_string(),
            details: format!("Failed to execute git stash drop: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "stash_drop".to_string(),
            details: format!(
                "git stash drop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Stash dropped successfully");
    Ok(())
}

pub(super) fn stash_apply(repo: &Repository, index: u32) -> Result<(), GitError> {
    info!("Applying stash@{{{}}} (without pop)", index);

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["stash", "apply", &format!("stash@{{{}}}", index)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_apply".to_string(),
            details: format!("Failed to execute git stash apply: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "stash_apply".to_string(),
            details: format!(
                "git stash apply failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Stash applied successfully (kept in list)");
    Ok(())
}

pub(super) fn stash_diff(repo: &Repository, index: u32) -> Result<String, GitError> {
    info!("Getting diff for stash@{{{}}}", index);

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["stash", "show", "-p", &format!("stash@{{{}}}", index)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_diff".to_string(),
            details: format!("Failed to execute git stash show: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "stash_diff".to_string(),
            details: format!(
                "git stash show failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

    let repo_path = repo.command_cwd();
    let stash_ref = format!("stash@{{{}}}", index);

    let output = git_command()
        .args(["stash", "branch", branch_name, &stash_ref])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "unstash_as_branch".to_string(),
            details: format!("Failed to execute git stash branch: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "unstash_as_branch".to_string(),
            details: format!(
                "git stash branch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Stash applied to new branch '{}' successfully", branch_name);
    Ok(())
}

pub(super) fn stash_clear(repo: &Repository) -> Result<(), GitError> {
    info!("Clearing all stashes");

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["stash", "clear"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "stash_clear".to_string(),
            details: format!("Failed to execute git stash clear: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "stash_clear".to_string(),
            details: format!(
                "git stash clear failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("All stashes cleared");
    Ok(())
}
