use super::*;
use crate::process::git_command;
use std::io::Write;

pub(super) fn apply_patch_cached(repo: &Repository, patch: &str) -> Result<(), GitError> {
    let repo_path = repo.command_cwd();

    // Allocate an exclusive file: clock timestamps can collide across workers.
    let mut patch_file = tempfile::NamedTempFile::new()?;
    patch_file.write_all(patch.as_bytes()).map_err(|e| GitError::OperationFailed {
        operation: "apply_patch_cached".to_string(),
        details: format!("Failed to write patch file: {}", e),
    })?;

    // Run git apply --cached
    let output = git_command()
        .args(["apply", "--cached", "--unidiff-zero", "--whitespace=nowarn"])
        .arg(patch_file.path())
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "apply_patch_cached".to_string(),
            details: format!("Failed to execute git apply: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "apply_patch_cached".to_string(),
            details: format!(
                "git apply failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}

pub(super) fn apply_patch_workdir(repo: &Repository, patch: &str) -> Result<(), GitError> {
    let repo_path = repo.command_cwd();

    // Allocate an exclusive file: clock timestamps can collide across workers.
    let mut patch_file = tempfile::NamedTempFile::new()?;
    patch_file.write_all(patch.as_bytes()).map_err(|e| GitError::OperationFailed {
        operation: "apply_patch_workdir".to_string(),
        details: format!("Failed to write patch file: {}", e),
    })?;

    // Run git apply (not --cached, applies to workdir)
    let output = git_command()
        .args(["apply", "--unidiff-zero", "--whitespace=nowarn"])
        .arg(patch_file.path())
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "apply_patch_workdir".to_string(),
            details: format!("Failed to execute git apply: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "apply_patch_workdir".to_string(),
            details: format!(
                "git apply failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}
