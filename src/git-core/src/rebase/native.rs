//! MAS rebase execution and explicit refusal of foreign sequencers.

use super::*;
use crate::native::{journal::Kind, sequencer};

pub(super) fn rebase_start(repo: &Repository, onto: &str) -> Result<String, GitError> {
    start_onto(repo, onto, false)
}

pub(crate) fn start_onto(
    repo: &Repository,
    onto: &str,
    autocrlf: bool,
) -> Result<String, GitError> {
    let (onto, entries) = {
        let raw = repo.inner.read().unwrap();
        let onto = raw.revparse_single(onto)?.peel_to_commit()?.id();
        let head = raw.head()?.peel_to_commit()?.id();
        if head == onto || raw.graph_descendant_of(head, onto)? {
            return Ok("Already up to date".into());
        }
        let mut walk = raw.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
        walk.push_head()?;
        walk.hide(onto)?;
        let mut entries = Vec::new();
        for id in walk {
            let commit = raw.find_commit(id?)?;
            if commit.parent_count() <= 1 {
                entries.push(RebaseTodoEntry {
                    action: "pick".into(),
                    commit: commit.id().to_string(),
                    message: commit.summary().unwrap_or_default().into(),
                });
            }
        }
        (onto.to_string(), entries)
    };
    let completed = sequencer::start(repo, Kind::Rebase, Some(onto), &entries, autocrlf)?;
    Ok(outcome(completed))
}

fn outcome(completed: bool) -> String {
    if completed {
        "Rebase completed"
    } else {
        "Resolve or edit the current step, then continue"
    }
    .into()
}

pub(super) fn start_interactive_rebase(
    repo: &Repository,
    base_ref: Option<&str>,
    entries: &[RebaseTodoEntry],
) -> Result<String, GitError> {
    ensure_clean_worktree(repo, "interactive_rebase_start")?;
    let chain = current_branch_first_parent_chain(repo, "interactive_rebase_start")?;
    let base = base_ref
        .map(|value| resolve_commit_oid(repo, value, "interactive_rebase_start"))
        .transpose()?;
    let start = match base {
        Some(base) => {
            chain
                .iter()
                .position(|id| *id == base)
                .ok_or_else(|| GitError::InvalidInput {
                    message: "base is not on this branch's first-parent chain".into(),
                })?
                + 1
        }
        None => 0,
    };
    let expected: std::collections::BTreeSet<_> = chain[start..].iter().copied().collect();
    let submitted = entries
        .iter()
        .map(|entry| Oid::from_str(&entry.commit))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    if expected.is_empty() || !submitted.is_subset(&expected) || submitted.len() != entries.len() {
        return Err(GitError::InvalidInput {
            message: "history plan no longer matches the selected branch".into(),
        });
    }
    let mut steps = entries.to_vec();
    for missing in expected.difference(&submitted) {
        steps.push(RebaseTodoEntry {
            action: "drop".into(),
            commit: missing.to_string(),
            message: String::new(),
        });
    }
    ensure_local_interactive_rebase_allowed(repo, chain[start], "interactive_rebase_start")?;
    if repo.current_branch()?.is_none() {
        return Err(GitError::InvalidInput {
            message: "interactive rebase needs a checked-out branch".into(),
        });
    }
    Ok(outcome(sequencer::start(
        repo,
        Kind::Rebase,
        base.map(|id| id.to_string()),
        &steps,
        false,
    )?))
}

fn foreign() -> GitError {
    GitError::RecoveryRequired {
        reason: "finish or abort this existing Git operation in the tool that started it".into(),
    }
}

pub(super) fn rebase_continue(_repo: &Repository) -> Result<RebaseResult, GitError> {
    Err(foreign())
}
pub(super) fn rebase_abort(_repo: &Repository) -> Result<(), GitError> {
    Err(foreign())
}
pub(super) fn rebase_skip(_repo: &Repository) -> Result<RebaseResult, GitError> {
    Err(foreign())
}
pub(super) fn has_rebase_conflicts(repo: &Repository) -> Result<bool, GitError> {
    Ok(repo.inner.read().unwrap().index()?.has_conflicts())
}
