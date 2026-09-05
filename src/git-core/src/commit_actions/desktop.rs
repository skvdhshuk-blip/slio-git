//! Desktop execution; preserves the system Git behavior.

use super::*;
use crate::process::git_command;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn export_commit_patch(
    repo: &Repository,
    commit_id: &str,
    output_path: &Path,
) -> Result<(), GitError> {
    info!(
        "Exporting patch for commit '{}' to '{}'",
        commit_id,
        output_path.display()
    );

    let output = git_command()
        .args(["format-patch", "--stdout", "-1", commit_id])
        .current_dir(repo.command_cwd())
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "format-patch".to_string(),
            details: format!("Failed to execute git format-patch: {e}"),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "format-patch".to_string(),
            details: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output_path, output.stdout)?;
    Ok(())
}

pub(super) fn cherry_pick_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    info!("Cherry-picking commit '{}'", commit_id);
    // Let git handle dirty worktree errors naturally with its own messages

    let args = vec![
        "cherry-pick".to_string(),
        "--no-edit".to_string(),
        commit_id.to_string(),
    ];
    run_git_command(repo, "cherry-pick", &args)
}

pub(super) fn revert_commit(repo: &Repository, commit_id: &str) -> Result<(), GitError> {
    info!("Reverting commit '{}'", commit_id);
    // No clean worktree check — git revert works with dirty worktree (matches IDEA behavior)

    let args = vec![
        "revert".to_string(),
        "--no-edit".to_string(),
        commit_id.to_string(),
    ];
    run_git_command(repo, "revert", &args)
}

pub(super) fn continue_in_progress_commit_action(
    repo: &Repository,
    kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    let operation = match kind {
        InProgressCommitActionKind::CherryPick => "cherry-pick",
        InProgressCommitActionKind::Revert => "revert",
    };

    let add_output = git_command()
        .args(["add", "-A"])
        .current_dir(repo.command_cwd())
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: format!("{operation}_continue"),
            details: format!("Failed to execute git add: {e}"),
        })?;

    if !add_output.status.success() {
        return Err(GitError::OperationFailed {
            operation: format!("{operation}_continue"),
            details: format!(
                "git add failed: {}",
                String::from_utf8_lossy(&add_output.stderr)
            ),
        });
    }

    let args = match kind {
        InProgressCommitActionKind::CherryPick => {
            vec![
                "-c".to_string(),
                "core.editor=true".to_string(),
                "cherry-pick".to_string(),
                "--continue".to_string(),
            ]
        }
        InProgressCommitActionKind::Revert => {
            vec![
                "-c".to_string(),
                "core.editor=true".to_string(),
                "revert".to_string(),
                "--continue".to_string(),
            ]
        }
    };

    run_git_command(repo, &format!("{operation}_continue"), &args)
}

pub(super) fn abort_in_progress_commit_action(
    repo: &Repository,
    kind: InProgressCommitActionKind,
) -> Result<(), GitError> {
    let (operation, args) = match kind {
        InProgressCommitActionKind::CherryPick => (
            "cherry-pick",
            vec!["cherry-pick".to_string(), "--abort".to_string()],
        ),
        InProgressCommitActionKind::Revert => {
            ("revert", vec!["revert".to_string(), "--abort".to_string()])
        }
    };

    run_git_command(repo, &format!("{operation}_abort"), &args)
}

pub(super) fn run_scripted_interactive_rebase(
    repo: &Repository,
    operation: &str,
    base_spec: Option<&str>,
    todo_contents: &str,
    auto_accept_editor: bool,
) -> Result<RewriteExecution, GitError> {
    let temp_dir = rewrite_temp_dir(operation);
    fs::create_dir_all(&temp_dir).map_err(GitError::Io)?;

    let todo_path = temp_dir.join("git-rebase-todo");
    let script_path = if cfg!(windows) {
        temp_dir.join("sequence-editor.cmd")
    } else {
        temp_dir.join("sequence-editor.sh")
    };

    fs::write(&todo_path, todo_contents).map_err(GitError::Io)?;
    write_sequence_editor_script(operation, &todo_path, &script_path)?;

    let mut command = git_command();
    command.current_dir(repo.command_cwd());
    command.env("GIT_SEQUENCE_EDITOR", &script_path);
    if auto_accept_editor {
        command.env("GIT_EDITOR", "true");
    }

    command.arg("rebase").arg("-i");
    if let Some(base_spec) = base_spec {
        command.arg(base_spec);
    } else {
        command.arg("--root");
    }

    let output = command.output().map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: format!("Failed to execute git {operation}: {e}"),
    })?;

    let cleanup_result = fs::remove_dir_all(&temp_dir);
    if cleanup_result.is_err() {
        let _ = cleanup_result;
    }

    if output.status.success() {
        return Ok(if has_rebase_in_progress(repo) {
            RewriteExecution::InProgress
        } else {
            RewriteExecution::Completed
        });
    }

    if has_rebase_in_progress(repo) {
        return Ok(RewriteExecution::InProgress);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let details = if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    };

    Err(GitError::OperationFailed {
        operation: operation.to_string(),
        details: format!("git {operation} failed: {details}"),
    })
}

pub(super) fn push_current_branch_to_commit(
    repo: &Repository,
    target: &PushCurrentBranchTarget,
) -> Result<(), GitError> {
    info!(
        "Pushing current branch '{}' to '{}' at '{}'",
        target.local_branch_name, target.upstream_ref, target.selected_commit
    );

    ensure_no_in_progress_operation(repo, "push-to-here")?;

    let refspec = format!(
        "{}:refs/heads/{}",
        target.selected_commit, target.upstream_branch_name
    );
    let mut args = vec!["push".to_string()];
    if target.requires_force_with_lease {
        args.push("--force-with-lease".to_string());
    }
    args.push(target.remote_name.clone());
    args.push(refspec);

    run_git_command(repo, "push", &args)
}

pub(super) fn run_git_command(
    repo: &Repository,
    operation: &str,
    args: &[String],
) -> Result<(), GitError> {
    let output = git_command()
        .args(args)
        .current_dir(repo.command_cwd())
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: format!("Failed to execute git {operation}: {e}"),
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let details = if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    };

    Err(GitError::OperationFailed {
        operation: operation.to_string(),
        details: format!("git {operation} failed: {details}"),
    })
}

pub(super) fn rewrite_temp_dir(operation: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "slio-git-{operation}-{}-{timestamp}",
        std::process::id()
    ))
}

pub(super) fn write_sequence_editor_script(
    operation: &str,
    todo_path: &Path,
    script_path: &Path,
) -> Result<(), GitError> {
    #[cfg(unix)]
    let contents = format!("#!/bin/sh\ncat '{}' > \"$1\"\n", todo_path.display());
    #[cfg(windows)]
    let contents = format!("@echo off\r\ntype \"{}\" > %1\r\n", todo_path.display());

    fs::write(script_path, contents).map_err(GitError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(script_path)
            .map_err(GitError::Io)?
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(script_path, permissions).map_err(GitError::Io)?;
    }

    let _ = operation;
    Ok(())
}

pub(super) fn reset(repo: &Repository, commit_id: &str, mode: ResetMode) -> Result<(), GitError> {
    run_git_command(
        repo,
        "reset",
        &["reset".into(), mode.git_flag().into(), commit_id.into()],
    )
}
