use super::*;
use crate::process::git_command;

pub(super) fn apply_patch_cached(repo: &Repository, patch: &str) -> Result<(), GitError> {
    let repo_path = repo.command_cwd();

    // Write patch to a temporary file
    let mut temp_path = std::env::temp_dir();
    temp_path.push(format!(
        "patch_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    std::fs::write(&temp_path, patch).map_err(|e| GitError::OperationFailed {
        operation: "apply_patch_cached".to_string(),
        details: format!("Failed to write patch file: {}", e),
    })?;

    // Run git apply --cached
    let output = git_command()
        .args(["apply", "--cached", "--unidiff-zero", "--whitespace=nowarn"])
        .arg(temp_path.as_path())
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "apply_patch_cached".to_string(),
            details: format!("Failed to execute git apply: {}", e),
        })?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

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

    // Write patch to a temporary file
    let mut temp_path = std::env::temp_dir();
    temp_path.push(format!(
        "patch_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    std::fs::write(&temp_path, patch).map_err(|e| GitError::OperationFailed {
        operation: "apply_patch_workdir".to_string(),
        details: format!("Failed to write patch file: {}", e),
    })?;

    // Run git apply (not --cached, applies to workdir)
    let output = git_command()
        .args(["apply", "--unidiff-zero", "--whitespace=nowarn"])
        .arg(temp_path.as_path())
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "apply_patch_workdir".to_string(),
            details: format!("Failed to execute git apply: {}", e),
        })?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

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
