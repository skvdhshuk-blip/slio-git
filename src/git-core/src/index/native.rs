use super::*;

fn apply_patch_git2(
    repo: &Repository,
    patch: &str,
    location: git2::ApplyLocation,
    operation: &str,
) -> Result<(), GitError> {
    let diff =
        git2::Diff::from_buffer(patch.as_bytes()).map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    let repo_lock = repo.inner.write().unwrap();
    repo_lock
        .apply(&diff, location, None)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}
pub(super) fn apply_patch_cached(repo: &Repository, patch: &str) -> Result<(), GitError> {
    apply_patch_git2(
        repo,
        patch,
        git2::ApplyLocation::Index,
        "apply_patch_cached",
    )
}
pub(super) fn apply_patch_workdir(repo: &Repository, patch: &str) -> Result<(), GitError> {
    apply_patch_git2(
        repo,
        patch,
        git2::ApplyLocation::WorkDir,
        "apply_patch_workdir",
    )
}
