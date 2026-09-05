//! Desktop execution for repository.

use super::*;
use crate::process::git_command;

pub(super) fn current_upstream_ref(repo: &Repository) -> Option<String> {
    let branch_name = repo.current_branch().ok().flatten()?;
    let upstream_ref_spec = format!("{branch_name}@{{upstream}}");
    let repo_path = repo.command_cwd();
    let output = git_command()
        .args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            &upstream_ref_spec,
        ])
        .current_dir(&repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let upstream = String::from_utf8_lossy(&output.stdout);
    let upstream = upstream.trim();
    (!upstream.is_empty()).then(|| upstream.to_string())
}

pub(super) fn sync_status(repo: &Repository) -> SyncStatus {
    // Get current branch name
    let branch_name = match repo.current_branch() {
        Ok(Some(name)) => name,
        _ => return SyncStatus::NoUpstream,
    };

    let upstream_ref = format!("{}@{{upstream}}", branch_name);
    let revspec = format!("{branch_name}...{upstream_ref}");
    let repo_path = repo.command_cwd();
    let output = git_command()
        .args(["rev-list", "--left-right", "--count", &revspec])
        .current_dir(&repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = output_str.split_whitespace().collect();
            if parts.len() >= 2 {
                let ahead: usize = parts[0].parse().unwrap_or(0);
                let behind: usize = parts[1].parse().unwrap_or(0);

                if ahead == 0 && behind == 0 {
                    SyncStatus::Synced
                } else if ahead > 0 && behind == 0 {
                    SyncStatus::Ahead(ahead)
                } else if ahead == 0 && behind > 0 {
                    SyncStatus::Behind(behind)
                } else {
                    SyncStatus::Diverged { ahead, behind }
                }
            } else {
                SyncStatus::NoUpstream
            }
        }
        _ => SyncStatus::NoUpstream,
    }
}
