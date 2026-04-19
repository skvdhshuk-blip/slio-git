//! Commit operations for git-core

use crate::error::GitError;
use crate::repository::Repository;
use log::info;
use std::fs;

/// A Git commit
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
    pub author_time: i64,
    pub committer_name: String,
    pub committer_email: String,
    pub committer_time: i64,
    pub parent_ids: Vec<String>,
}

/// File-level change status for a commit diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommitChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// A file changed by a specific commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: CommitChangeStatus,
}

/// Create a new commit
pub fn create_commit(
    repo: &Repository,
    message: &str,
    _author_name: &str,
    _author_email: &str,
) -> Result<String, GitError> {
    info!("Creating commit: {}", message);

    let repo_lock = repo.inner.write().unwrap();

    // Get the index
    let mut index = repo_lock.index().map_err(|e| GitError::OperationFailed {
        operation: "create_commit".to_string(),
        details: e.to_string(),
    })?;

    // Write the tree
    let tree_oid = index.write_tree().map_err(|e| GitError::OperationFailed {
        operation: "create_commit".to_string(),
        details: e.to_string(),
    })?;

    let tree = repo_lock
        .find_tree(tree_oid)
        .map_err(|e| GitError::OperationFailed {
            operation: "create_commit".to_string(),
            details: e.to_string(),
        })?;

    let repository_state = repo_lock.state();
    let mut parent_commits = Vec::new();
    if let Some(parent_commit) = repo_lock
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
    {
        parent_commits.push(parent_commit);
    }
    if repository_state == git2::RepositoryState::Merge {
        parent_commits.extend(load_merge_head_parents(&repo_lock)?);
    }
    let parent_refs: Vec<&git2::Commit<'_>> = parent_commits.iter().collect();

    // Create signature
    let signature = repo_lock
        .signature()
        .map_err(|e| GitError::OperationFailed {
            operation: "create_commit".to_string(),
            details: e.to_string(),
        })?;

    let commit_oid = repo_lock
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parent_refs,
        )
        .map_err(|e| GitError::OperationFailed {
        operation: "create_commit".to_string(),
        details: e.to_string(),
    })?;

    if repository_state == git2::RepositoryState::Merge {
        repo_lock
            .cleanup_state()
            .map_err(|e| GitError::OperationFailed {
                operation: "create_commit".to_string(),
                details: format!("Failed to finalize merge state: {e}"),
            })?;
    }

    info!("Commit created: {}", commit_oid);

    Ok(commit_oid.to_string())
}

fn load_merge_head_parents(
    repo: &git2::Repository,
) -> Result<Vec<git2::Commit<'_>>, GitError> {
    let merge_head_path = repo.path().join("MERGE_HEAD");
    let merge_head_contents = fs::read_to_string(&merge_head_path).map_err(|e| {
        GitError::OperationFailed {
            operation: "create_commit".to_string(),
            details: format!("Failed to read MERGE_HEAD: {e}"),
        }
    })?;

    let mut commits = Vec::new();
    for commit_id in merge_head_contents.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let oid = git2::Oid::from_str(commit_id).map_err(|e| GitError::OperationFailed {
            operation: "create_commit".to_string(),
            details: format!("Invalid MERGE_HEAD commit id '{commit_id}': {e}"),
        })?;
        let commit = repo.find_commit(oid).map_err(|e| GitError::OperationFailed {
            operation: "create_commit".to_string(),
            details: format!("Failed to load MERGE_HEAD commit '{commit_id}': {e}"),
        })?;
        commits.push(commit);
    }

    Ok(commits)
}

/// Amend a commit with a new message
pub fn amend_commit(repo: &Repository, commit_id: &str, message: &str) -> Result<String, GitError> {
    info!("Amending commit: {}", commit_id);

    let repo_lock = repo.inner.write().unwrap();

    let oid = git2::Oid::from_str(commit_id).map_err(|_| GitError::CommitNotFound {
        id: commit_id.to_string(),
    })?;

    let commit = repo_lock
        .find_commit(oid)
        .map_err(|_| GitError::CommitNotFound {
            id: commit_id.to_string(),
        })?;

    // Get the tree from the commit
    let tree = commit.tree().map_err(|e| GitError::OperationFailed {
        operation: "amend_commit".to_string(),
        details: e.to_string(),
    })?;

    // Create signature
    let signature = repo_lock
        .signature()
        .map_err(|e| GitError::OperationFailed {
            operation: "amend_commit".to_string(),
            details: e.to_string(),
        })?;

    // Amend the commit using the commit object
    let amend_oid = commit
        .amend(
            Some("HEAD"),
            Some(&signature),
            Some(&signature),
            None,
            Some(message),
            Some(&tree),
        )
        .map_err(|e| GitError::OperationFailed {
            operation: "amend_commit".to_string(),
            details: e.to_string(),
        })?;

    info!("Commit amended: {}", amend_oid);
    Ok(amend_oid.to_string())
}

/// Create a signature for commits
pub fn create_signature(
    _repo: &Repository,
    name: &str,
    email: &str,
) -> Result<git2::Signature<'static>, GitError> {
    git2::Signature::now(name, email).map_err(|e| GitError::OperationFailed {
        operation: "create_signature".to_string(),
        details: e.to_string(),
    })
}

/// Get the default signature (user's git config)
pub fn get_default_signature(repo: &Repository) -> Result<git2::Signature<'static>, GitError> {
    let repo_lock = repo.inner.read().unwrap();

    repo_lock
        .signature()
        .map_err(|e| GitError::OperationFailed {
            operation: "get_default_signature".to_string(),
            details: e.to_string(),
        })
}

/// Validate a commit reference (hash, branch name, tag, etc.)
/// Returns the resolved full hash and first line of commit message if valid.
pub fn validate_commit_ref(
    repo: &Repository,
    reference: &str,
) -> Result<(String, String), GitError> {
    let repo_lock = repo.inner.read().unwrap();
    let object = repo_lock
        .revparse_single(reference)
        .map_err(|_| GitError::CommitNotFound {
            id: reference.to_string(),
        })?;
    let commit = object
        .peel_to_commit()
        .map_err(|_| GitError::CommitNotFound {
            id: reference.to_string(),
        })?;
    let hash = commit.id().to_string();
    let summary = commit.summary().unwrap_or("").to_string();
    Ok((hash, summary))
}

/// Get commit information
pub fn get_commit(repo: &Repository, commit_id: &str) -> Result<CommitInfo, GitError> {
    let repo_lock = repo.inner.read().unwrap();

    let oid = git2::Oid::from_str(commit_id).map_err(|_| GitError::CommitNotFound {
        id: commit_id.to_string(),
    })?;

    let commit = repo_lock
        .find_commit(oid)
        .map_err(|_| GitError::CommitNotFound {
            id: commit_id.to_string(),
        })?;

    let author = commit.author();
    let committer = commit.committer();

    Ok(CommitInfo {
        id: commit_id.to_string(),
        message: commit.message().unwrap_or("").to_string(),
        author_name: author.name().unwrap_or("").to_string(),
        author_email: author.email().unwrap_or("").to_string(),
        author_time: commit.time().seconds(),
        committer_name: committer.name().unwrap_or("").to_string(),
        committer_email: committer.email().unwrap_or("").to_string(),
        committer_time: committer.when().seconds(),
        parent_ids: commit.parents().map(|p| p.id().to_string()).collect(),
    })
}

/// Get the list of files changed by a commit compared with its first parent.
pub fn get_commit_changed_files(
    repo: &Repository,
    commit_id: &str,
) -> Result<Vec<CommitChangedFile>, GitError> {
    let repo_lock = repo.inner.read().unwrap();

    let oid = git2::Oid::from_str(commit_id).map_err(|_| GitError::CommitNotFound {
        id: commit_id.to_string(),
    })?;

    let commit = repo_lock
        .find_commit(oid)
        .map_err(|_| GitError::CommitNotFound {
            id: commit_id.to_string(),
        })?;

    let commit_tree = commit.tree().map_err(|e| GitError::OperationFailed {
        operation: "get_commit_changed_files".to_string(),
        details: e.to_string(),
    })?;

    let parent_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(
            commit
                .parent(0)
                .and_then(|parent| parent.tree())
                .map_err(|e| GitError::OperationFailed {
                    operation: "get_commit_changed_files".to_string(),
                    details: e.to_string(),
                })?,
        )
    };

    let mut diff = repo_lock
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        .map_err(|e| GitError::OperationFailed {
            operation: "get_commit_changed_files".to_string(),
            details: e.to_string(),
        })?;

    let mut find_options = git2::DiffFindOptions::new();
    find_options.renames(true);
    diff.find_similar(Some(&mut find_options))
        .map_err(|e| GitError::OperationFailed {
            operation: "get_commit_changed_files".to_string(),
            details: e.to_string(),
        })?;

    let mut changed_files = diff
        .deltas()
        .map(|delta| {
            let status = match delta.status() {
                git2::Delta::Added => CommitChangeStatus::Added,
                git2::Delta::Deleted => CommitChangeStatus::Deleted,
                git2::Delta::Renamed => CommitChangeStatus::Renamed,
                _ => CommitChangeStatus::Modified,
            };

            let old_path = delta
                .old_file()
                .path()
                .map(|path| path.to_string_lossy().to_string());
            let new_path = delta
                .new_file()
                .path()
                .map(|path| path.to_string_lossy().to_string());

            CommitChangedFile {
                path: new_path
                    .clone()
                    .or_else(|| old_path.clone())
                    .unwrap_or_default(),
                old_path: (status == CommitChangeStatus::Renamed).then_some(old_path).flatten(),
                status,
            }
        })
        .collect::<Vec<_>>();

    changed_files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.old_path.cmp(&right.old_path))
            .then(left.status.cmp(&right.status))
    });

    Ok(changed_files)
}

// --- Commit message history persistence ---

use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_RECENT_MESSAGES: usize = 10;

fn config_dir() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("slio-git"))
}

fn history_file_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("commit-messages.json"))
}

/// Load recent commit messages for a specific repository path.
/// Returns up to 10 messages, newest first.
pub fn load_recent_messages(repo_path: &Path) -> Vec<String> {
    let Some(file_path) = history_file_path() else {
        return Vec::new();
    };

    let Ok(content) = std::fs::read_to_string(&file_path) else {
        return Vec::new();
    };

    let key = repo_path.to_string_lossy().to_string();
    let map: HashMap<String, Vec<String>> = serde_json::from_str(&content).unwrap_or_default();

    map.get(&key).cloned().unwrap_or_default()
}

/// Save a commit message to the recent history for a repository.
/// Keeps the last MAX_RECENT_MESSAGES messages, newest first.
pub fn save_recent_message(repo_path: &Path, message: &str) {
    if message.trim().is_empty() {
        return;
    }

    let Some(file_path) = history_file_path() else {
        return;
    };

    // Ensure config directory exists
    if let Some(dir) = file_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let key = repo_path.to_string_lossy().to_string();

    // Load existing
    let mut map: HashMap<String, Vec<String>> = std::fs::read_to_string(&file_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    let messages = map.entry(key).or_default();

    // Remove duplicate if exists
    messages.retain(|m| m != message);

    // Insert at front
    messages.insert(0, message.to_string());

    // Trim to max
    messages.truncate(MAX_RECENT_MESSAGES);

    // Save
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(&file_path, json);
    }

    info!("Saved commit message to history");
}

#[cfg(test)]
mod tests {
    use super::{get_commit_changed_files, CommitChangeStatus};
    use crate::commit;
    use crate::diff::{resolve_conflict, ConflictResolution};
    use crate::index;
    use crate::repository::{Repository, RepositoryState};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn configure_signature(repo: &Repository) {
        let repo_lock = repo.inner.read().expect("repo lock");
        let mut config = repo_lock.config().expect("config");
        config
            .set_str("user.name", "slio-git tests")
            .expect("user.name");
        config
            .set_str("user.email", "tests@slio-git.local")
            .expect("user.email");
    }

    fn write_file(root: &Path, path: &str, content: &str) {
        let file_path = root.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(file_path, content).expect("write file");
    }

    fn commit_all(repo: &Repository, message: &str) -> String {
        index::stage_all(repo).expect("stage all");
        commit::create_commit(repo, message, "", "").expect("create commit")
    }

    fn remove_from_index(repo: &Repository, path: &str) {
        let repo_lock = repo.inner.write().expect("repo lock");
        let mut index = repo_lock.index().expect("index");
        index
            .remove_path(Path::new(path))
            .expect("remove path from index");
        index.write().expect("write index");
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn try_run_git(root: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git")
            .status
            .success()
    }

    fn head_oid(root: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .expect("git rev-parse");
        assert!(
            output.status.success(),
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn get_commit_changed_files_reports_root_commit_as_added_files() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo = Repository::init(temp_dir.path()).expect("init repo");
        configure_signature(&repo);

        write_file(temp_dir.path(), "src/main.rs", "fn main() {}\n");
        let commit_id = commit_all(&repo, "initial");

        let files = get_commit_changed_files(&repo, &commit_id).expect("load changed files");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].old_path, None);
        assert_eq!(files[0].status, CommitChangeStatus::Added);
    }

    #[test]
    fn get_commit_changed_files_reports_modify_delete_and_rename_statuses() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo = Repository::init(temp_dir.path()).expect("init repo");
        configure_signature(&repo);

        write_file(temp_dir.path(), "modified.txt", "before\n");
        write_file(temp_dir.path(), "deleted.txt", "remove me\n");
        write_file(temp_dir.path(), "old-name.txt", "rename me\n");
        commit_all(&repo, "baseline");

        write_file(temp_dir.path(), "modified.txt", "after\n");
        fs::remove_file(temp_dir.path().join("deleted.txt")).expect("remove deleted file");
        fs::rename(
            temp_dir.path().join("old-name.txt"),
            temp_dir.path().join("new-name.txt"),
        )
        .expect("rename file");
        index::stage_file(&repo, Path::new("modified.txt")).expect("stage modified");
        index::stage_file(&repo, Path::new("new-name.txt")).expect("stage renamed target");
        remove_from_index(&repo, "deleted.txt");
        remove_from_index(&repo, "old-name.txt");

        let commit_id = commit::create_commit(&repo, "change set", "", "").expect("create commit");
        let files = get_commit_changed_files(&repo, &commit_id).expect("load changed files");

        assert_eq!(files.len(), 3);

        let deleted = files
            .iter()
            .find(|file| file.path == "deleted.txt")
            .expect("deleted entry");
        assert_eq!(deleted.status, CommitChangeStatus::Deleted);
        assert_eq!(deleted.old_path, None);

        let modified = files
            .iter()
            .find(|file| file.path == "modified.txt")
            .expect("modified entry");
        assert_eq!(modified.status, CommitChangeStatus::Modified);
        assert_eq!(modified.old_path, None);

        let renamed = files
            .iter()
            .find(|file| file.path == "new-name.txt")
            .expect("renamed entry");
        assert_eq!(renamed.status, CommitChangeStatus::Renamed);
        assert_eq!(renamed.old_path.as_deref(), Some("old-name.txt"));
    }

    #[test]
    fn get_commit_changed_files_compares_merge_commit_with_first_parent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo = Repository::init(temp_dir.path()).expect("init repo");
        configure_signature(&repo);

        write_file(temp_dir.path(), "base.txt", "base\n");
        commit_all(&repo, "baseline");

        let base_branch = repo
            .current_branch()
            .expect("current branch")
            .expect("branch name");

        run_git(temp_dir.path(), &["checkout", "-b", "feature"]);
        write_file(temp_dir.path(), "feature.txt", "feature\n");
        commit_all(&repo, "feature commit");

        run_git(temp_dir.path(), &["checkout", &base_branch]);
        write_file(temp_dir.path(), "main.txt", "main\n");
        commit_all(&repo, "main commit");

        run_git(
            temp_dir.path(),
            &["merge", "feature", "--no-ff", "-m", "merge feature"],
        );

        let merge_commit_id = head_oid(temp_dir.path());
        let files = get_commit_changed_files(&repo, &merge_commit_id).expect("load changed files");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "feature.txt");
        assert_eq!(files[0].status, CommitChangeStatus::Added);
    }

    #[test]
    fn create_commit_finalizes_merge_state_and_creates_merge_commit() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo = Repository::init(temp_dir.path()).expect("init repo");
        configure_signature(&repo);

        write_file(temp_dir.path(), "shared.txt", "base\n");
        commit_all(&repo, "baseline");

        let base_branch = repo
            .current_branch()
            .expect("current branch")
            .expect("branch name");

        run_git(temp_dir.path(), &["checkout", "-b", "feature"]);
        write_file(temp_dir.path(), "shared.txt", "feature change\n");
        commit_all(&repo, "feature change");

        run_git(temp_dir.path(), &["checkout", &base_branch]);
        write_file(temp_dir.path(), "shared.txt", "main change\n");
        commit_all(&repo, "main change");

        assert!(
            !try_run_git(temp_dir.path(), &["merge", "feature", "--no-edit"]),
            "merge should stop on conflict"
        );

        let repo = Repository::discover(temp_dir.path()).expect("discover conflicted repo");
        assert_eq!(repo.get_state(), RepositoryState::Merging);

        resolve_conflict(
            &repo,
            Path::new("shared.txt"),
            ConflictResolution::Ours,
        )
        .expect("resolve conflict");

        let commit_id =
            commit::create_commit(&repo, "merge resolved", "", "").expect("create merge commit");

        let refreshed_repo = Repository::discover(temp_dir.path()).expect("refresh repo");
        assert_eq!(refreshed_repo.get_state(), RepositoryState::Clean);

        let merge_commit = commit::get_commit(&refreshed_repo, &commit_id).expect("load commit");
        assert_eq!(merge_commit.parent_ids.len(), 2);
    }

    #[test]
    fn create_commit_creates_merge_commit_even_with_stale_repository_handle() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo = Repository::init(temp_dir.path()).expect("init repo");
        configure_signature(&repo);

        write_file(temp_dir.path(), "shared.txt", "base\n");
        commit_all(&repo, "baseline");

        let base_branch = repo
            .current_branch()
            .expect("current branch")
            .expect("branch name");

        run_git(temp_dir.path(), &["checkout", "-b", "feature"]);
        write_file(temp_dir.path(), "shared.txt", "feature change\n");
        commit_all(&repo, "feature change");

        run_git(temp_dir.path(), &["checkout", &base_branch]);
        write_file(temp_dir.path(), "shared.txt", "main change\n");
        commit_all(&repo, "main change");

        let stale_repo = Repository::discover(temp_dir.path()).expect("open repo before merge");
        assert_eq!(stale_repo.get_state(), RepositoryState::Clean);

        assert!(
            !try_run_git(temp_dir.path(), &["merge", "feature", "--no-edit"]),
            "merge should stop on conflict"
        );

        resolve_conflict(
            &stale_repo,
            Path::new("shared.txt"),
            ConflictResolution::Ours,
        )
        .expect("resolve conflict");

        let commit_id =
            commit::create_commit(&stale_repo, "merge resolved", "", "").expect("create commit");

        let refreshed_repo = Repository::discover(temp_dir.path()).expect("refresh repo");
        assert_eq!(refreshed_repo.get_state(), RepositoryState::Clean);

        let merge_commit = commit::get_commit(&refreshed_repo, &commit_id).expect("load commit");
        assert_eq!(merge_commit.parent_ids.len(), 2);
    }
}
