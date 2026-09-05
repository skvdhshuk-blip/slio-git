//! Native merge shares the durable index/worktree transition machinery.
use super::{journal::Kind, sequencer};
use crate::{GitError, RebaseTodoEntry, Repository};

pub(crate) fn start(
    repo: &Repository,
    target: &str,
    no_ff: bool,
    squash: bool,
    autocrlf: bool,
) -> Result<(), GitError> {
    let (head, target, ff) = {
        let raw = repo.inner.read().unwrap();
        let head = raw.head()?.peel_to_commit()?.id();
        let target = raw.revparse_single(target)?.peel_to_commit()?.id();
        if head == target || raw.graph_descendant_of(head, target)? {
            return Ok(());
        }
        (
            head.to_string(),
            target.to_string(),
            raw.graph_descendant_of(target, head)?,
        )
    };
    let action = if squash {
        "squash-merge"
    } else if ff && !no_ff {
        "fast-forward"
    } else {
        "merge"
    };
    let done = sequencer::start(
        repo,
        Kind::Merge,
        Some(head),
        &[RebaseTodoEntry {
            action: action.into(),
            commit: target,
            message: String::new(),
        }],
        autocrlf,
    )?;
    if !done && !squash {
        return Err(GitError::MergeConflict);
    }
    Ok(())
}
