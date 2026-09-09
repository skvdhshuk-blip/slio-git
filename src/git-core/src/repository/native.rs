//! Native execution for repository.

use super::*;

pub(super) fn abort_merge(_repo: &Repository) -> Result<(), GitError> {
    Err(GitError::RecoveryRequired {
        reason: "请回到发起合并的工具中止当前流程".into(),
    })
}

pub(super) fn current_upstream_ref(repo: &Repository) -> Option<String> {
    let branch_name = repo.current_branch().ok().flatten()?;
    let repo_lock = repo.inner.read().ok()?;
    let branch = repo_lock
        .find_branch(&branch_name, git2::BranchType::Local)
        .ok()?;
    let upstream = branch.upstream().ok()?;
    let name = upstream.name().ok().flatten().map(str::to_string)?;
    Some(
        name.strip_prefix("refs/remotes/")
            .unwrap_or(&name)
            .to_string(),
    )
}

pub(super) fn sync_status(repo: &Repository) -> SyncStatus {
    // Get current branch name
    let branch_name = match repo.current_branch() {
        Ok(Some(name)) => name,
        _ => return SyncStatus::NoUpstream,
    };

    let repo_lock = match repo.inner.read() {
        Ok(lock) => lock,
        Err(_) => return SyncStatus::Unknown,
    };
    let branch = match repo_lock.find_branch(&branch_name, git2::BranchType::Local) {
        Ok(branch) => branch,
        Err(_) => return SyncStatus::NoUpstream,
    };
    let local_oid = match branch.get().target() {
        Some(oid) => oid,
        None => return SyncStatus::Unknown,
    };
    let upstream = match branch.upstream() {
        Ok(upstream) => upstream,
        Err(_) => return SyncStatus::NoUpstream,
    };
    let upstream_oid = match upstream.get().target() {
        Some(oid) => oid,
        None => return SyncStatus::Unknown,
    };
    match repo_lock.graph_ahead_behind(local_oid, upstream_oid) {
        Ok((0, 0)) => SyncStatus::Synced,
        Ok((ahead, 0)) => SyncStatus::Ahead(ahead),
        Ok((0, behind)) => SyncStatus::Behind(behind),
        Ok((ahead, behind)) => SyncStatus::Diverged { ahead, behind },
        Err(_) => SyncStatus::Unknown,
    }
}
