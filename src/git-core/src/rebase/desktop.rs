//! Desktop execution; preserves the system Git behavior.

use super::*;
use crate::process::git_command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn rebase_start(repo: &Repository, onto: &str) -> Result<String, GitError> {
    info!("Starting rebase onto '{}'", onto);

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["rebase", onto])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "rebase_start".to_string(),
            details: format!("Failed to execute git rebase: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "rebase_start".to_string(),
            details: format!(
                "git rebase failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Rebase started successfully");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        Ok(format!("Rebase started onto {onto}"))
    } else {
        Ok(stdout)
    }
}

pub(super) fn start_interactive_rebase(
    repo: &Repository,
    base_ref: Option<&str>,
    entries: &[RebaseTodoEntry],
) -> Result<String, GitError> {
    info!(
        "Starting interactive rebase with {} todo entries",
        entries.len()
    );
    ensure_clean_worktree(repo, "interactive_rebase_start")?;

    let todo_contents = build_todo_contents(entries)?;
    let temp_dir = interactive_rebase_temp_dir("interactive_rebase");
    fs::create_dir_all(&temp_dir).map_err(GitError::Io)?;

    let todo_path = temp_dir.join("git-rebase-todo");
    let script_path = if cfg!(windows) {
        temp_dir.join("sequence-editor.cmd")
    } else {
        temp_dir.join("sequence-editor.sh")
    };

    fs::write(&todo_path, todo_contents).map_err(GitError::Io)?;
    write_sequence_editor_script(&todo_path, &script_path)?;

    let mut command = git_command();
    command.current_dir(repo.command_cwd());
    command.env("GIT_SEQUENCE_EDITOR", &script_path);
    if entries
        .iter()
        .any(|entry| entry.action.eq_ignore_ascii_case("squash"))
    {
        command.env("GIT_EDITOR", "true");
    }

    command.arg("rebase").arg("-i");
    if let Some(base_ref) = base_ref {
        command.arg(base_ref);
    } else {
        command.arg("--root");
    }

    let output = command.output().map_err(|e| GitError::OperationFailed {
        operation: "interactive_rebase_start".to_string(),
        details: format!("Failed to execute git rebase -i: {e}"),
    })?;

    let _ = fs::remove_dir_all(&temp_dir);

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok(if stdout.is_empty() {
            "交互式变基已启动".to_string()
        } else {
            stdout
        });
    }

    if is_rebase_in_progress(repo) {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        return Ok(if message.is_empty() {
            "交互式变基已进入待继续状态".to_string()
        } else {
            message
        });
    }

    Err(GitError::OperationFailed {
        operation: "interactive_rebase_start".to_string(),
        details: format!(
            "git rebase -i failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    })
}

pub(super) fn rebase_continue(repo: &Repository) -> Result<RebaseResult, GitError> {
    info!("Continuing rebase");

    let repo_path = repo.command_cwd();

    if !repo.path.join("rebase-merge").exists() && !repo.path.join("rebase-apply").exists() {
        return Err(GitError::InvalidInput {
            message: "no rebase is active".into(),
        });
    }
    crate::index::stage_resolved_conflicts(&repo.inner.write().unwrap())?;

    // Then continue the rebase
    let output = git_command()
        .args(["-c", "core.editor=true", "rebase", "--continue"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "rebase_continue".to_string(),
            details: format!("Failed to execute git rebase: {}", e),
        })?;

    let result = RebaseResult {
        success: output.status.success(),
        message: command_result_message(&output),
    };

    if !output.status.success() {
        info!("Rebase continue failed: {}", result.message);
    } else {
        info!("Rebase continued successfully");
    }

    Ok(result)
}

pub(super) fn rebase_abort(repo: &Repository) -> Result<(), GitError> {
    info!("Aborting rebase");

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["rebase", "--abort"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "rebase_abort".to_string(),
            details: format!("Failed to execute git rebase: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "rebase_abort".to_string(),
            details: format!(
                "git rebase --abort failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Rebase aborted successfully");
    Ok(())
}

pub(super) fn rebase_skip(repo: &Repository) -> Result<RebaseResult, GitError> {
    info!("Skipping current commit during rebase");

    let repo_path = repo.command_cwd();

    let output = git_command()
        .args(["rebase", "--skip"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "rebase_skip".to_string(),
            details: format!("Failed to execute git rebase: {}", e),
        })?;

    let result = RebaseResult {
        success: output.status.success(),
        message: command_result_message(&output),
    };

    if !output.status.success() {
        info!("Rebase skip failed: {}", result.message);
    } else {
        info!("Rebase skipped successfully");
    }

    Ok(result)
}

pub(super) fn has_rebase_conflicts(repo: &Repository) -> Result<bool, GitError> {
    let repo_path = repo.command_cwd();

    // Check for conflict markers in the index
    let output = git_command()
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "has_rebase_conflicts".to_string(),
            details: format!("Failed to execute git diff: {}", e),
        })?;

    let conflicted_files = String::from_utf8_lossy(&output.stdout);
    let has_conflicts = !conflicted_files.trim().is_empty();

    Ok(has_conflicts)
}

pub(super) fn interactive_rebase_temp_dir(operation: &str) -> PathBuf {
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

    Ok(())
}

pub(super) fn command_result_message(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stderr}\n{stdout}"),
    }
}
