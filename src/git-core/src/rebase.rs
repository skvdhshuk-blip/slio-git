//! Rebase operations for git-core
//!
//! Provides rebase functionality using git commands

#[cfg_attr(feature = "app-store", path = "rebase/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "rebase/desktop.rs")]
mod backend;

use crate::error::GitError;
use crate::index;
use crate::repository::{Repository, RepositoryState};
use git2::Oid;
use log::info;
use std::fs;
use std::path::{Path, PathBuf};

/// Rebase operation result
#[derive(Debug, Clone)]
pub struct RebaseResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseTodoEntry {
    pub action: String,
    pub commit: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct InteractiveRebasePlan {
    pub base_ref: Option<String>,
    pub start_commit: String,
    pub entries: Vec<RebaseTodoEntry>,
}

fn git_dir(repo: &Repository) -> PathBuf {
    repo.path.clone()
}

#[cfg(any(test, not(feature = "app-store")))]
fn is_rebase_in_progress(repo: &Repository) -> bool {
    if crate::native::active(repo) {
        return true;
    }
    let git_dir = git_dir(repo);
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

fn current_head_oid(repo: &Repository, operation: &str) -> Result<Oid, GitError> {
    let repo_lock = repo.inner.read().unwrap();
    let head = repo_lock.head().map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: e.to_string(),
    })?;
    let commit = head
        .peel_to_commit()
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    Ok(commit.id())
}

fn resolve_commit_oid(repo: &Repository, spec: &str, operation: &str) -> Result<Oid, GitError> {
    let repo_lock = repo.inner.read().unwrap();
    let object = repo_lock
        .revparse_single(spec)
        .map_err(|_| GitError::CommitNotFound {
            id: spec.to_string(),
        })?;
    let commit = object
        .peel_to_commit()
        .map_err(|_| GitError::CommitNotFound {
            id: spec.to_string(),
        })?;
    let _ = operation;
    Ok(commit.id())
}

fn is_ancestor(repo: &Repository, ancestor: Oid, descendant: Oid) -> Result<bool, GitError> {
    if ancestor == descendant {
        return Ok(true);
    }

    let repo_lock = repo.inner.read().unwrap();
    repo_lock
        .graph_descendant_of(descendant, ancestor)
        .map_err(|e| GitError::OperationFailed {
            operation: "graph_descendant_of".to_string(),
            details: e.to_string(),
        })
}

fn ensure_clean_worktree(repo: &Repository, operation: &str) -> Result<(), GitError> {
    match repo.get_state() {
        RepositoryState::Clean | RepositoryState::Dirty => {}
        _ => {
            return Err(GitError::OperationFailed {
                operation: operation.to_string(),
                details: format!(
                    "当前仓库正处于 {}，请先完成或中止当前流程",
                    repo.state_hint()
                        .unwrap_or_else(|| "进行中的 Git 操作".to_string())
                ),
            });
        }
    }

    if index::get_status(repo)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?
        .is_empty()
    {
        Ok(())
    } else {
        Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: "当前仓库还有未提交改动，请先提交、暂存或清理工作区".to_string(),
        })
    }
}

fn current_branch_first_parent_chain(
    repo: &Repository,
    operation: &str,
) -> Result<Vec<Oid>, GitError> {
    let head_oid = current_head_oid(repo, operation)?;
    let repo_lock = repo.inner.read().unwrap();
    let mut commit = repo_lock
        .find_commit(head_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;

    let mut chain = Vec::new();
    loop {
        chain.push(commit.id());
        if commit.parent_count() == 0 {
            break;
        }
        commit = commit.parent(0).map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    }

    chain.reverse();
    Ok(chain)
}

fn commit_subject_for_oid(
    repo_lock: &git2::Repository,
    oid: Oid,
    operation: &str,
) -> Result<String, GitError> {
    let commit = repo_lock
        .find_commit(oid)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    Ok(commit
        .summary()
        .unwrap_or("(no subject)")
        .replace(['\n', '\r'], " "))
}

fn ensure_local_interactive_rebase_allowed(
    repo: &Repository,
    start_oid: Oid,
    operation: &str,
) -> Result<(), GitError> {
    if let Some(upstream_ref) = repo.current_upstream_ref() {
        let upstream_oid = resolve_commit_oid(repo, &upstream_ref, operation)?;
        if is_ancestor(repo, start_oid, upstream_oid)? {
            return Err(GitError::OperationFailed {
                operation: operation.to_string(),
                details: format!(
                    "提交已经包含在当前上游 {upstream_ref} 中，暂不支持直接改写已发布历史"
                ),
            });
        }
    }
    Ok(())
}

fn parse_todo_line(line: &str) -> Option<RebaseTodoEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "noop" {
        return None;
    }

    let mut parts = trimmed.splitn(3, char::is_whitespace);
    let action = parts.next()?.trim();
    let commit = parts.next()?.trim();
    let message = parts.next().unwrap_or("").trim();
    if action.is_empty() || commit.is_empty() {
        return None;
    }

    Some(RebaseTodoEntry {
        action: action.to_string(),
        commit: commit.to_string(),
        message: message.to_string(),
    })
}

fn read_last_todo_entry(path: &Path) -> Result<Option<RebaseTodoEntry>, GitError> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).map_err(|e| GitError::OperationFailed {
        operation: "read_rebase_done".to_string(),
        details: format!("读取 rebase 步骤文件失败: {e}"),
    })?;

    Ok(contents.lines().rev().find_map(parse_todo_line))
}

#[cfg(any(test, not(feature = "app-store")))]
fn build_todo_contents(entries: &[RebaseTodoEntry]) -> Result<String, GitError> {
    if entries.is_empty() {
        return Err(GitError::OperationFailed {
            operation: "interactive_rebase".to_string(),
            details: "至少需要保留一条 todo 项".to_string(),
        });
    }

    let mut contents = String::new();
    for (index, entry) in entries.iter().enumerate() {
        let normalized_action = entry.action.trim().to_lowercase();
        let action = if normalized_action == "reword" {
            "edit"
        } else {
            normalized_action.as_str()
        };
        if action.is_empty() {
            return Err(GitError::OperationFailed {
                operation: "interactive_rebase".to_string(),
                details: format!("第 {} 条 todo 缺少动作", index + 1),
            });
        }
        if index == 0 && (action == "fixup" || action == "squash") {
            return Err(GitError::OperationFailed {
                operation: "interactive_rebase".to_string(),
                details: "首条 todo 不能直接使用 fixup 或 squash".to_string(),
            });
        }
        if entry.commit.trim().is_empty() {
            return Err(GitError::OperationFailed {
                operation: "interactive_rebase".to_string(),
                details: format!("第 {} 条 todo 缺少提交哈希", index + 1),
            });
        }
        contents.push_str(action);
        contents.push(' ');
        contents.push_str(entry.commit.trim());
        if !entry.message.trim().is_empty() {
            contents.push(' ');
            contents.push_str(entry.message.trim());
        }
        contents.push('\n');
    }

    Ok(contents)
}

/// Start a rebase onto the given branch / ref.
pub fn rebase_start(repo: &Repository, onto: &str) -> Result<String, GitError> {
    crate::native::reject_active(repo)?;
    backend::rebase_start(repo, onto)
}

pub fn prepare_interactive_rebase_plan(
    repo: &Repository,
    start_commit: &str,
) -> Result<InteractiveRebasePlan, GitError> {
    info!(
        "Preparing interactive rebase plan from commit '{}'",
        start_commit
    );
    ensure_clean_worktree(repo, "interactive_rebase_prepare")?;

    let current_branch = repo
        .current_branch()
        .map_err(|e| GitError::OperationFailed {
            operation: "interactive_rebase_prepare".to_string(),
            details: e.to_string(),
        })?;
    if current_branch.is_none() {
        return Err(GitError::OperationFailed {
            operation: "interactive_rebase_prepare".to_string(),
            details: "当前为 detached HEAD，不能围绕当前分支开始交互式变基".to_string(),
        });
    }

    let start_oid = resolve_commit_oid(repo, start_commit, "interactive_rebase_prepare")?;
    let head_oid = current_head_oid(repo, "interactive_rebase_prepare")?;
    if !is_ancestor(repo, start_oid, head_oid)? {
        return Err(GitError::OperationFailed {
            operation: "interactive_rebase_prepare".to_string(),
            details: "只能从当前分支第一父链上的祖先提交开始整理历史".to_string(),
        });
    }

    ensure_local_interactive_rebase_allowed(repo, start_oid, "interactive_rebase_prepare")?;

    let chain = current_branch_first_parent_chain(repo, "interactive_rebase_prepare")?;
    let Some(start_index) = chain.iter().position(|oid| *oid == start_oid) else {
        return Err(GitError::OperationFailed {
            operation: "interactive_rebase_prepare".to_string(),
            details: "暂只支持围绕当前分支第一父链开始交互式变基".to_string(),
        });
    };

    let repo_lock = repo.inner.read().unwrap();
    let mut entries = Vec::new();
    for oid in &chain[start_index..] {
        let commit = repo_lock
            .find_commit(*oid)
            .map_err(|e| GitError::OperationFailed {
                operation: "interactive_rebase_prepare".to_string(),
                details: e.to_string(),
            })?;
        if commit.parent_count() > 1 {
            return Err(GitError::OperationFailed {
                operation: "interactive_rebase_prepare".to_string(),
                details: "当前整理范围里包含 merge 提交，暂不支持在这里开始交互式变基".to_string(),
            });
        }
        entries.push(RebaseTodoEntry {
            action: "pick".to_string(),
            commit: oid.to_string(),
            message: commit_subject_for_oid(&repo_lock, *oid, "interactive_rebase_prepare")?,
        });
    }

    let base_ref = if start_index == 0 {
        None
    } else {
        Some(chain[start_index - 1].to_string())
    };

    Ok(InteractiveRebasePlan {
        base_ref,
        start_commit: start_commit.to_string(),
        entries,
    })
}

pub fn start_interactive_rebase(
    repo: &Repository,
    base_ref: Option<&str>,
    entries: &[RebaseTodoEntry],
) -> Result<String, GitError> {
    crate::native::reject_active(repo)?;
    backend::start_interactive_rebase(repo, base_ref, entries)
}

/// Continue a rebase after resolving conflicts
pub fn rebase_continue(repo: &Repository) -> Result<RebaseResult, GitError> {
    if crate::native::active(repo) {
        crate::native::require_kind(repo, crate::native::journal::Kind::Rebase)?;
        let success = crate::native::sequencer::continue_operation(repo)?;
        return Ok(RebaseResult {
            success,
            message: if success {
                "变基完成"
            } else {
                "变基等待冲突处理或修改提交"
            }
            .into(),
        });
    }
    backend::rebase_continue(repo)
}

/// Abort the current rebase
pub fn rebase_abort(repo: &Repository) -> Result<(), GitError> {
    if crate::native::active(repo) {
        crate::native::require_kind(repo, crate::native::journal::Kind::Rebase)?;
        return crate::native::sequencer::abort(repo);
    }
    backend::rebase_abort(repo)
}

/// Skip the current commit during rebase
pub fn rebase_skip(repo: &Repository) -> Result<RebaseResult, GitError> {
    if crate::native::active(repo) {
        crate::native::require_kind(repo, crate::native::journal::Kind::Rebase)?;
        let success = crate::native::sequencer::skip(repo)?;
        return Ok(RebaseResult {
            success,
            message: if success {
                "变基完成"
            } else {
                "变基等待冲突处理或修改提交"
            }
            .into(),
        });
    }
    backend::rebase_skip(repo)
}

/// Get the current rebase status
pub fn get_rebase_status(repo: &Repository) -> Result<Option<RebaseStatus>, GitError> {
    if let Some(operation) = crate::native::sequencer::status(repo)? {
        if operation.kind != crate::native::journal::Kind::Rebase {
            return Ok(None);
        }
        let total = operation.steps.len() as u32;
        let current = if operation.phase == "edit" {
            operation.cursor
        } else {
            operation.cursor + 1
        }
        .min(operation.steps.len()) as u32;
        return Ok(Some(RebaseStatus {
            is_interactive: true,
            current_step: current,
            total_steps: total,
            progress: if total == 0 {
                1.0
            } else {
                current as f32 / total as f32
            },
        }));
    }
    let git_dir = git_dir(repo);

    // Check if we're in a rebase
    let rebase_merge = git_dir.join("rebase-merge");
    let rebase_apply = git_dir.join("rebase-apply");

    let is_rebasing = rebase_merge.exists() || rebase_apply.exists();

    if !is_rebasing {
        return Ok(None);
    }

    // Get the current step info
    let step_file = rebase_merge.join("msgnum");
    let last_file = rebase_merge.join("last");
    let end_file = rebase_merge.join("end");

    let total_file = if last_file.exists() {
        last_file
    } else {
        end_file
    };

    let (current, total) = if step_file.exists() && total_file.exists() {
        let step = std::fs::read_to_string(&step_file)
            .map(|s| s.trim().parse::<u32>().unwrap_or(1))
            .unwrap_or(1);
        let last = std::fs::read_to_string(&total_file)
            .map(|s| s.trim().parse::<u32>().unwrap_or(1))
            .unwrap_or(1);
        (step, last)
    } else {
        (0, 0)
    };

    Ok(Some(RebaseStatus {
        is_interactive: rebase_merge.exists(),
        current_step: current,
        total_steps: total,
        progress: if total > 0 {
            current as f32 / total as f32
        } else {
            0.0
        },
    }))
}

pub fn get_rebase_todo(repo: &Repository) -> Result<Vec<RebaseTodoEntry>, GitError> {
    if let Some(operation) = crate::native::sequencer::status(repo)? {
        if operation.kind != crate::native::journal::Kind::Rebase {
            return Ok(Vec::new());
        }
        return operation.steps[operation.cursor..]
            .iter()
            .map(|step| native_todo(repo, step))
            .collect();
    }
    let git_dir = git_dir(repo);
    let todo_path = git_dir.join("rebase-merge").join("git-rebase-todo");
    if !todo_path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&todo_path).map_err(|e| GitError::OperationFailed {
        operation: "get_rebase_todo".to_string(),
        details: format!("读取 git-rebase-todo 失败: {e}"),
    })?;

    Ok(contents.lines().filter_map(parse_todo_line).collect())
}

pub fn get_current_rebase_step(repo: &Repository) -> Result<Option<RebaseTodoEntry>, GitError> {
    if let Some(operation) = crate::native::sequencer::status(repo)? {
        if operation.kind != crate::native::journal::Kind::Rebase {
            return Ok(None);
        }
        let cursor = if operation.phase == "edit" {
            operation.cursor.saturating_sub(1)
        } else {
            operation.cursor
        };
        return operation
            .steps
            .get(cursor)
            .map(|step| native_todo(repo, step))
            .transpose();
    }
    let git_dir = git_dir(repo);
    read_last_todo_entry(&git_dir.join("rebase-merge").join("done"))
}

/// Check if there are rebase conflicts
pub fn has_rebase_conflicts(repo: &Repository) -> Result<bool, GitError> {
    if crate::native::active(repo) {
        return Ok(repo.inner.read().unwrap().index()?.has_conflicts());
    }
    backend::has_rebase_conflicts(repo)
}

/// Rebase status information
#[derive(Debug, Clone)]
pub struct RebaseStatus {
    pub is_interactive: bool,
    pub current_step: u32,
    pub total_steps: u32,
    pub progress: f32,
}

#[cfg(test)]
mod tests {
    use super::{
        RebaseTodoEntry, build_todo_contents, get_current_rebase_step, get_rebase_status,
        get_rebase_todo, is_rebase_in_progress, prepare_interactive_rebase_plan, rebase_abort,
        start_interactive_rebase,
    };
    use crate::error::GitError;
    use crate::repository::Repository;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(repo_path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_stdout(repo_path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn create_commit(repo_path: &Path, file_name: &str, contents: &str, message: &str) {
        fs::write(repo_path.join(file_name), contents).expect("write file");
        run_git(repo_path, &["add", file_name]);
        run_git(repo_path, &["commit", "-m", message]);
    }

    fn create_linear_history_repo() -> (Repository, TempDir, Vec<String>) {
        let temp_dir = TempDir::new().expect("temp dir");
        run_git(temp_dir.path(), &["init"]);
        run_git(temp_dir.path(), &["config", "user.name", "slio-git"]);
        run_git(
            temp_dir.path(),
            &["config", "user.email", "slio-git@example.com"],
        );

        create_commit(temp_dir.path(), "notes.txt", "one\n", "first");
        create_commit(temp_dir.path(), "notes.txt", "one\ntwo\n", "second");
        create_commit(temp_dir.path(), "notes.txt", "one\ntwo\nthree\n", "third");

        let repo = Repository::discover(temp_dir.path()).expect("discover repo");
        let commits = git_stdout(temp_dir.path(), &["rev-list", "--reverse", "HEAD"])
            .lines()
            .map(ToOwned::to_owned)
            .collect();

        (repo, temp_dir, commits)
    }

    #[test]
    fn build_todo_contents_translates_reword_and_blocks_invalid_first_actions() {
        let contents = build_todo_contents(&[RebaseTodoEntry {
            action: "reword".to_string(),
            commit: "abc123".to_string(),
            message: "rename commit".to_string(),
        }])
        .expect("build todo");

        assert_eq!(contents, "edit abc123 rename commit\n");

        let error = build_todo_contents(&[RebaseTodoEntry {
            action: "squash".to_string(),
            commit: "abc123".to_string(),
            message: "rename commit".to_string(),
        }])
        .expect_err("first squash should be rejected");

        match error {
            GitError::OperationFailed { details, .. } => {
                assert!(details.contains("首条 todo 不能直接使用 fixup 或 squash"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn prepare_interactive_rebase_plan_uses_selected_commit_as_tail_boundary() {
        let (repo, _temp_dir, commits) = create_linear_history_repo();
        let plan =
            prepare_interactive_rebase_plan(&repo, &commits[1]).expect("prepare interactive plan");

        assert_eq!(plan.base_ref.as_deref(), Some(commits[0].as_str()));
        assert_eq!(plan.start_commit, commits[1]);
        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].commit, commits[1]);
        assert_eq!(plan.entries[1].commit, commits[2]);
        assert!(plan.entries.iter().all(|entry| entry.action == "pick"));
    }

    #[test]
    fn start_interactive_rebase_exposes_current_step_and_remaining_todo() {
        let (repo, _temp_dir, commits) = create_linear_history_repo();
        let entries = vec![
            RebaseTodoEntry {
                action: "reword".to_string(),
                commit: commits[1].clone(),
                message: "second".to_string(),
            },
            RebaseTodoEntry {
                action: "pick".to_string(),
                commit: commits[2].clone(),
                message: "third".to_string(),
            },
        ];

        start_interactive_rebase(&repo, Some(&commits[0]), &entries)
            .expect("start interactive rebase");

        assert!(is_rebase_in_progress(&repo));

        let status = get_rebase_status(&repo)
            .expect("read rebase status")
            .expect("status should exist");
        assert!(status.is_interactive);
        assert_eq!(status.current_step, 1);
        assert_eq!(status.total_steps, 2);

        let current_step = get_current_rebase_step(&repo)
            .expect("read current step")
            .expect("current step should exist");
        assert_eq!(current_step.action, "edit");
        assert_eq!(current_step.commit, commits[1]);
        assert_eq!(current_step.message, "second");

        let remaining = get_rebase_todo(&repo).expect("read remaining todo");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].commit, commits[2]);
        assert_eq!(remaining[0].action, "pick");

        rebase_abort(&repo).expect("abort rebase");
        assert!(!is_rebase_in_progress(&repo));
    }
}

fn native_todo(
    repo: &Repository,
    step: &crate::native::journal::Step,
) -> Result<RebaseTodoEntry, GitError> {
    let commit = crate::commit::get_commit(repo, &step.commit)?;
    Ok(RebaseTodoEntry {
        action: if step.action == "reword" {
            "edit".into()
        } else {
            step.action.clone()
        },
        commit: step.commit.clone(),
        message: commit.message.lines().next().unwrap_or_default().into(),
    })
}

#[cfg(feature = "app-store")]
pub(crate) use backend::start_onto;
