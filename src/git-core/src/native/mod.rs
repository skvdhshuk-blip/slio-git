//! In-process Git execution and recovery, with no external programs.
//!
//! Desktop uses its original adapter for new operations, but includes the
//! sequencer so a native operation can be recovered after changing channels.

pub(crate) mod journal;
pub(crate) mod merge;
pub(crate) mod sequencer;

use crate::{GitError, Repository};

pub(crate) fn active(repo: &Repository) -> bool {
    journal::directory(&repo.path).join("state.json").exists()
}

pub(crate) fn reject_active(repo: &Repository) -> Result<(), GitError> {
    if active(repo) {
        Err(GitError::RecoveryRequired {
            reason: "finish or abort the active slio operation before starting another operation"
                .into(),
        })
    } else {
        #[cfg(feature = "app-store")]
        if repo.inner.read().unwrap().state() != git2::RepositoryState::Clean {
            return Err(journal::recovery(
                "请回到发起该流程的工具继续或中止；商店版保留当前现场",
            ));
        }
        Ok(())
    }
}

// A damaged native journal must keep recovery visible instead of reporting clean.
pub(crate) fn state(repo: &git2::Repository) -> Option<crate::repository::RepositoryState> {
    use crate::repository::RepositoryState;
    match journal::Journal::load(repo) {
        Ok(None) => None,
        Ok(Some(operation)) => Some(match operation.kind {
            journal::Kind::Rebase => RepositoryState::Rebasing,
            journal::Kind::CherryPick => RepositoryState::CherryPick,
            journal::Kind::Revert => RepositoryState::Revert,
            journal::Kind::Merge => RepositoryState::Merging,
        }),
        Err(_) => Some(RepositoryState::Rebasing),
    }
}

pub(crate) fn require_kind(repo: &Repository, kind: journal::Kind) -> Result<(), GitError> {
    let operation =
        sequencer::status(repo)?.ok_or_else(|| journal::recovery("no native operation"))?;
    if operation.kind != kind {
        return Err(journal::recovery(
            "use the recovery controls for the active operation",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// Edits are allowed only at a user-editable native checkpoint. MAS must leave
/// foreign sequencers untouched, including their conflict/index files.
pub(crate) fn allow_edit(repo: &Repository) -> Result<(), GitError> {
    if let Some(operation) = sequencer::status(repo)? {
        if !matches!(
            operation.phase.as_str(),
            "conflict" | "edit" | "merge-ready"
        ) {
            return Err(journal::recovery("请先继续或中止恢复，再修改文件"));
        }
    } else {
        #[cfg(feature = "app-store")]
        if repo.inner.read().unwrap().state() != git2::RepositoryState::Clean {
            return Err(journal::recovery(
                "请回到发起该流程的工具解决冲突并继续或中止",
            ));
        }
    }
    Ok(())
}
