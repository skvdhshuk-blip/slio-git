//! Remote operations for git-core

use crate::error::GitError;
#[cfg_attr(feature = "app-store", path = "remote/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "remote/desktop.rs")]
mod backend;
use crate::repository::Repository;
use git2::{Config, RemoteCallbacks};
use log::info;

/// A Git remote
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PullOptions<'a> {
    pub branch_name: Option<&'a str>,
    pub rebase: bool,
    pub ff_only: bool,
    pub no_ff: bool,
    pub squash: bool,
    pub force_autocrlf_true: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushOptions<'a> {
    pub target_branch: Option<&'a str>,
    pub force_with_lease: bool,
    pub push_tags: bool,
    pub set_upstream: bool,
}

#[cfg(any(test, not(feature = "app-store")))]
fn remote_url_uses_ssh(url: &str) -> bool {
    if url.starts_with("ssh://") {
        return true;
    }

    if url.contains("://") {
        return false;
    }

    let mut parts = url.splitn(2, ':');
    let Some(left) = parts.next() else {
        return false;
    };

    parts.next().is_some() && left.contains('@')
}

fn resolve_auth_username(
    config: &Config,
    url: &str,
    explicit_username: Option<&str>,
    username_from_url: Option<&str>,
) -> Option<String> {
    explicit_username
        .filter(|username| !username.is_empty())
        .map(str::to_string)
        .or_else(|| username_from_url.map(str::to_string))
        .or_else(|| {
            #[cfg(not(feature = "app-store"))]
            {
                let mut helper = git2::CredentialHelper::new(url);
                helper.config(config);
                helper.username.clone()
            }
            #[cfg(feature = "app-store")]
            {
                config
                    .get_string(&format!("credential.{url}.username"))
                    .ok()
                    .or_else(|| config.get_string("credential.username").ok())
            }
        })
}

pub(crate) fn build_remote_callbacks(
    config: Config,
    credentials: Option<(&str, &str)>,
    url: &str,
) -> RemoteCallbacks<'static> {
    #[cfg(feature = "app-store")]
    {
        backend::build_remote_callbacks(config, credentials, url)
    }
    #[cfg(not(feature = "app-store"))]
    {
        let _ = url;
        backend::build_remote_callbacks(config, credentials)
    }
}

/// libgit2 1.8 replaces a rejected SSH certificate callback's message. Keep
/// the failure actionable without ever treating an unknown host as trusted.
pub(crate) fn transport_error(error: &git2::Error) -> String {
    #[cfg(feature = "app-store")]
    if error.class() == git2::ErrorClass::Ssh && error.message().contains("hostkey") {
        return "SSH server identity could not be verified (unknown or changed host key). Verify the server and import its trusted known_hosts file in Settings.".into();
    }
    error.to_string()
}

#[cfg(feature = "app-store")]
pub(crate) fn push_reference(
    repo: &Repository,
    remote: &str,
    refspec: String,
) -> Result<(), GitError> {
    let raw = repo.inner.write().unwrap();
    backend::push_refspecs(&raw, remote, &[refspec], false, None, None)
}

#[cfg(not(feature = "app-store"))]
fn remote_url(repo: &Repository, remote_name: &str) -> Result<String, GitError> {
    let repo_lock = repo.inner.read().unwrap();
    let remote = repo_lock
        .find_remote(remote_name)
        .map_err(|e| GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: e.to_string(),
        })?;

    Ok(remote.url().unwrap_or("").to_string())
}

#[cfg(any(test, not(feature = "app-store")))]
fn has_explicit_credentials(credentials: Option<(&str, &str)>) -> bool {
    credentials.is_some_and(|(username, password)| {
        !username.trim().is_empty() || !password.trim().is_empty()
    })
}

#[cfg(any(test, not(feature = "app-store")))]
fn should_use_system_git_for_push(repo: &Repository, credentials: Option<(&str, &str)>) -> bool {
    repo.is_worktree() && !has_explicit_credentials(credentials)
}

fn configured_upstream_branch(repo: &Repository, remote_name: &str) -> Option<String> {
    let branch_name = repo.current_branch().ok().flatten()?;
    let repo_lock = repo.inner.read().ok()?;
    let config = repo_lock.config().ok()?;
    let configured_remote = config
        .get_string(&format!("branch.{branch_name}.remote"))
        .ok()?;

    if configured_remote != remote_name {
        return None;
    }

    let merge_ref = config
        .get_string(&format!("branch.{branch_name}.merge"))
        .ok()?;
    let upstream_branch = merge_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(merge_ref.as_str());

    (!upstream_branch.is_empty()).then(|| upstream_branch.to_string())
}

fn current_branch(repo: &Repository, operation: &str) -> Result<String, GitError> {
    repo.current_branch()
        .map_err(|error| GitError::OperationFailed {
            operation: operation.to_string(),
            details: error.to_string(),
        })?
        .ok_or_else(|| GitError::OperationFailed {
            operation: operation.to_string(),
            details: "Detached HEAD, cannot pull.".to_string(),
        })
}

#[cfg(any(test, not(feature = "app-store")))]
fn build_pull_args(
    repo: &Repository,
    remote_name: &str,
    options: PullOptions<'_>,
) -> Result<Vec<String>, GitError> {
    let mut args = Vec::new();
    if options.force_autocrlf_true {
        args.push("-c".to_string());
        args.push("core.autocrlf=true".to_string());
    }
    args.push("pull".to_string());

    if options.rebase {
        args.push("--rebase".to_string());
    } else {
        // Match GUI clients like TortoiseGit by explicitly disabling rebase for
        // the default merge-based pull path instead of inheriting ambient config.
        args.push("--no-rebase".to_string());
    }
    if options.ff_only {
        args.push("--ff-only".to_string());
    }
    if options.no_ff {
        args.push("--no-ff".to_string());
    }
    if options.squash {
        args.push("--squash".to_string());
    }

    let explicit_branch = options
        .branch_name
        .map(str::trim)
        .filter(|branch| !branch.is_empty());
    if let Some(branch_name) = explicit_branch {
        args.push(remote_name.to_string());
        args.push(branch_name.to_string());
        return Ok(args);
    }

    if configured_upstream_branch(repo, remote_name).is_some() {
        args.push(remote_name.to_string());
        return Ok(args);
    }

    let branch_name = current_branch(repo, "pull")?;

    args.push(remote_name.to_string());
    args.push(branch_name);
    Ok(args)
}

fn is_explicit_refspec(value: &str) -> bool {
    value.contains(':') || value.starts_with("refs/")
}

fn normalize_target_branch<'a>(branch_name: &'a str, options: PushOptions<'a>) -> &'a str {
    options
        .target_branch
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .unwrap_or(branch_name)
}

fn build_push_refspec(branch_name: &str, target_branch: &str) -> String {
    if is_explicit_refspec(branch_name) {
        return branch_name.to_string();
    }

    format!("refs/heads/{branch_name}:refs/heads/{target_branch}")
}

fn should_auto_set_upstream(repo: &Repository, branch_name: &str, target_branch: &str) -> bool {
    if is_explicit_refspec(branch_name) || branch_name != target_branch {
        return false;
    }

    let Ok(Some(current_branch)) = repo.current_branch() else {
        return false;
    };

    current_branch == branch_name && repo.current_upstream_ref().is_none()
}

#[cfg(any(test, not(feature = "app-store")))]
fn build_push_args(
    repo: &Repository,
    remote_name: &str,
    branch_name: &str,
    options: PushOptions<'_>,
) -> Vec<String> {
    let target_branch = normalize_target_branch(branch_name, options);
    let refspec = build_push_refspec(branch_name, target_branch);
    let should_set_upstream =
        options.set_upstream || should_auto_set_upstream(repo, branch_name, target_branch);

    let mut args = vec!["push".to_string()];
    if options.force_with_lease {
        args.push("--force-with-lease".to_string());
    }
    if should_set_upstream {
        args.push("--set-upstream".to_string());
    }
    if options.push_tags {
        args.push("--tags".to_string());
    }
    args.push(remote_name.to_string());
    args.push(refspec);
    args
}

/// List all remotes
pub fn list_remotes(repo: &Repository) -> Result<Vec<RemoteInfo>, GitError> {
    let repo_lock = repo.inner.read().unwrap();
    let mut remotes = Vec::new();

    let remote_names = repo_lock.remotes().map_err(|e| GitError::OperationFailed {
        operation: "list_remotes".to_string(),
        details: e.to_string(),
    })?;

    for i in 0..remote_names.len() {
        if let Some(name) = remote_names.get(i) {
            if let Ok(remote) = repo_lock.find_remote(name) {
                let url = remote.url().unwrap_or("").to_string();
                remotes.push(RemoteInfo {
                    name: name.to_string(),
                    url,
                });
            }
        }
    }

    Ok(remotes)
}

/// List remotes that are relevant to the current branch workflow.
///
/// If the current branch already tracks an upstream remote, keep the result
/// focused on that remote so the UI can stay anchored to the mainline sync
/// target. When no upstream is configured, fall back to all remotes.
pub fn list_branch_scoped_remotes(repo: &Repository) -> Result<Vec<RemoteInfo>, GitError> {
    let remotes = list_remotes(repo)?;
    let Some(preferred_remote) = repo.current_upstream_remote() else {
        return Ok(remotes);
    };

    let filtered = remotes
        .iter()
        .filter(|remote| remote.name == preferred_remote)
        .cloned()
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        Ok(remotes)
    } else {
        Ok(filtered)
    }
}

/// Fetch from a remote
pub fn fetch(
    repo: &Repository,
    remote_name: &str,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    backend::fetch(repo, remote_name, credentials)
}

/// Push to a remote
pub fn push(
    repo: &Repository,
    remote_name: &str,
    branch_name: &str,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    push_with_options(
        repo,
        remote_name,
        branch_name,
        PushOptions::default(),
        credentials,
    )
}

pub fn push_with_options(
    repo: &Repository,
    remote_name: &str,
    branch_name: &str,
    options: PushOptions<'_>,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::push_with_options(repo, remote_name, branch_name, options, credentials)
}

fn set_branch_upstream(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
    target_branch: &str,
) -> Result<(), GitError> {
    let repo_lock = repo.inner.write().unwrap();
    let mut config = repo_lock.config().map_err(|e| GitError::RemoteFailed {
        remote: remote_name.to_string(),
        details: e.to_string(),
    })?;
    config
        .set_str(&format!("branch.{branch_name}.remote"), remote_name)
        .map_err(|e| GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: e.to_string(),
        })?;
    config
        .set_str(
            &format!("branch.{branch_name}.merge"),
            &format!("refs/heads/{target_branch}"),
        )
        .map_err(|e| GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

/// Force push with --force-with-lease semantics
pub fn force_push(repo: &Repository, remote_name: &str, branch_name: &str) -> Result<(), GitError> {
    info!(
        "Force pushing branch '{}' to remote '{}' (--force-with-lease)",
        branch_name, remote_name
    );

    push_with_options(
        repo,
        remote_name,
        branch_name,
        PushOptions {
            force_with_lease: true,
            ..PushOptions::default()
        },
        None,
    )
}

/// Pull from a remote.
pub fn pull(
    repo: &Repository,
    remote_name: &str,
    branch_name: &str,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    pull_with_options(
        repo,
        remote_name,
        PullOptions {
            branch_name: Some(branch_name),
            ..PullOptions::default()
        },
        credentials,
    )
}

/// Pull from a remote using system Git semantics.
pub fn pull_with_options(
    repo: &Repository,
    remote_name: &str,
    options: PullOptions<'_>,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::pull_with_options(repo, remote_name, options, credentials)
}

#[cfg(test)]
mod tests {
    use super::{
        PullOptions, PushOptions, build_pull_args, build_push_args, push, remote_url_uses_ssh,
        resolve_auth_username, should_use_system_git_for_push,
    };
    use crate::repository::Repository;
    use git2::Config;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::{TempDir, tempdir};

    fn config_with_username(username: &str) -> (TempDir, Config) {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("gitconfig");
        fs::write(
            &config_path,
            format!("[credential]\n\tusername = {username}\n"),
        )
        .unwrap();

        let config = Config::open(&config_path).unwrap();
        (temp_dir, config)
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
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

    fn create_committed_repo() -> (TempDir, String) {
        let repo_dir = tempdir().unwrap();
        git(repo_dir.path(), &["init"]);
        git(
            repo_dir.path(),
            &["config", "user.email", "tests@example.com"],
        );
        git(repo_dir.path(), &["config", "user.name", "Test User"]);
        fs::write(repo_dir.path().join("tracked.txt"), "base\n").unwrap();
        git(repo_dir.path(), &["add", "tracked.txt"]);
        git(repo_dir.path(), &["commit", "-m", "base"]);
        let branch_name = git(repo_dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
        (repo_dir, branch_name)
    }

    fn create_linked_worktree_repo() -> (TempDir, Repository) {
        let (repo_dir, _) = create_committed_repo();
        let worktree_path = repo_dir.path().join("linked-worktree");

        git(
            repo_dir.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature/push-path",
                &worktree_path.display().to_string(),
            ],
        );

        let worktree_repo = Repository::open(&worktree_path).unwrap();
        (repo_dir, worktree_repo)
    }

    #[test]
    fn resolve_auth_username_prefers_explicit_username() {
        let (_temp_dir, config) = config_with_username("saved-user");

        let username = resolve_auth_username(
            &config,
            "https://example.com/repo.git",
            Some("manual-user"),
            Some("remote-user"),
        );

        assert_eq!(username.as_deref(), Some("manual-user"));
    }

    #[test]
    fn resolve_auth_username_falls_back_to_git_credential_username() {
        let (_temp_dir, config) = config_with_username("saved-user");

        let username = resolve_auth_username(&config, "https://example.com/repo.git", None, None);

        assert_eq!(username.as_deref(), Some("saved-user"));
    }

    #[test]
    fn build_pull_args_uses_configured_upstream_when_branch_is_empty() {
        let (repo_dir, branch_name) = create_committed_repo();
        let remote_dir = tempdir().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        git(
            repo_dir.path(),
            &[
                "remote",
                "add",
                "origin",
                &remote_dir.path().display().to_string(),
            ],
        );
        git(repo_dir.path(), &["push", "-u", "origin", &branch_name]);

        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_pull_args(&repo, "origin", PullOptions::default()).unwrap();

        assert_eq!(args, vec!["pull", "--no-rebase", "origin"]);
    }

    #[test]
    fn build_pull_args_falls_back_to_current_branch_without_upstream() {
        let (repo_dir, branch_name) = create_committed_repo();
        let remote_dir = tempdir().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        git(
            repo_dir.path(),
            &[
                "remote",
                "add",
                "origin",
                &remote_dir.path().display().to_string(),
            ],
        );

        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_pull_args(&repo, "origin", PullOptions::default()).unwrap();

        assert_eq!(args, vec!["pull", "--no-rebase", "origin", &branch_name]);
    }

    #[test]
    fn build_pull_args_passes_explicit_branch_and_strategy_flags() {
        let (repo_dir, _) = create_committed_repo();
        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_pull_args(
            &repo,
            "origin",
            PullOptions {
                branch_name: Some("release/main"),
                rebase: true,
                ..PullOptions::default()
            },
        )
        .unwrap();

        assert_eq!(args, vec!["pull", "--rebase", "origin", "release/main"]);
    }

    #[test]
    fn build_pull_args_includes_autocrlf_true_override_when_requested() {
        let (repo_dir, branch_name) = create_committed_repo();
        let remote_dir = tempdir().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        git(
            repo_dir.path(),
            &[
                "remote",
                "add",
                "origin",
                &remote_dir.path().display().to_string(),
            ],
        );

        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_pull_args(
            &repo,
            "origin",
            PullOptions {
                force_autocrlf_true: true,
                ..PullOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "-c",
                "core.autocrlf=true",
                "pull",
                "--no-rebase",
                "origin",
                &branch_name
            ]
        );
    }

    #[test]
    fn build_push_args_sets_upstream_when_branch_has_no_upstream() {
        let (repo_dir, branch_name) = create_committed_repo();
        let remote_dir = tempdir().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        git(
            repo_dir.path(),
            &[
                "remote",
                "add",
                "origin",
                &remote_dir.path().display().to_string(),
            ],
        );

        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_push_args(&repo, "origin", &branch_name, PushOptions::default());

        assert_eq!(
            args,
            vec![
                "push".to_string(),
                "--set-upstream".to_string(),
                "origin".to_string(),
                format!("refs/heads/{branch_name}:refs/heads/{branch_name}")
            ]
        );
    }

    #[test]
    fn build_push_args_honors_target_branch_and_manual_upstream() {
        let (repo_dir, branch_name) = create_committed_repo();
        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_push_args(
            &repo,
            "origin",
            &branch_name,
            PushOptions {
                target_branch: Some("review/main"),
                set_upstream: true,
                ..PushOptions::default()
            },
        );

        assert_eq!(
            args,
            vec![
                "push".to_string(),
                "--set-upstream".to_string(),
                "origin".to_string(),
                format!("refs/heads/{branch_name}:refs/heads/review/main")
            ]
        );
    }

    #[test]
    fn build_push_args_preserves_explicit_refspecs() {
        let (repo_dir, _) = create_committed_repo();
        let repo = Repository::open(repo_dir.path()).unwrap();
        let args = build_push_args(
            &repo,
            "origin",
            "abc123:refs/heads/main",
            PushOptions::default(),
        );

        assert_eq!(
            args,
            vec![
                "push".to_string(),
                "origin".to_string(),
                "abc123:refs/heads/main".to_string()
            ]
        );
    }

    #[test]
    fn push_creates_branch_on_empty_remote_and_sets_upstream() {
        let (repo_dir, branch_name) = create_committed_repo();
        let remote_dir = tempdir().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        git(
            repo_dir.path(),
            &[
                "remote",
                "add",
                "origin",
                &remote_dir.path().display().to_string(),
            ],
        );

        let repo = Repository::open(repo_dir.path()).unwrap();
        push(&repo, "origin", &branch_name, None).unwrap();

        let upstream = git(
            repo_dir.path(),
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        );
        assert_eq!(upstream, format!("origin/{branch_name}"));
    }

    #[test]
    fn push_from_worktree_with_explicit_credentials_keeps_libgit2_path() {
        let (_repo_dir, worktree_repo) = create_linked_worktree_repo();

        assert!(worktree_repo.is_worktree());
        assert!(!should_use_system_git_for_push(
            &worktree_repo,
            Some(("manual-user", "manual-password")),
        ));
    }

    #[test]
    fn push_from_worktree_without_credentials_uses_system_git_path() {
        let (_repo_dir, worktree_repo) = create_linked_worktree_repo();

        assert!(worktree_repo.is_worktree());
        assert!(should_use_system_git_for_push(&worktree_repo, None));
    }

    #[test]
    fn remote_url_uses_ssh_detects_common_ssh_forms() {
        assert!(remote_url_uses_ssh("git@codeup.aliyun.com:group/repo.git"));
        assert!(remote_url_uses_ssh(
            "ssh://git@codeup.aliyun.com/group/repo.git"
        ));
        assert!(!remote_url_uses_ssh(
            "https://codeup.aliyun.com/group/repo.git"
        ));
    }
}

#[cfg(feature = "app-store")]
pub(crate) use backend::push_selected_commit;
