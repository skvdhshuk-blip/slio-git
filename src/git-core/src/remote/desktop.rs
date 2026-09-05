//! Main channel transport, including its existing system Git fallback.
use super::*;
use crate::process::git_command;
use git2::{Cred, Error as Git2Error, FetchOptions, PushOptions as Git2PushOptions};

pub(super) fn fetch(
    repo: &Repository,
    remote_name: &str,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    info!("Fetching from remote '{}'", remote_name);

    let remote_url = remote_url(repo, remote_name)?;
    if remote_url_uses_ssh(&remote_url) {
        info!(
            "Using system git fetch for SSH remote '{}' ({})",
            remote_name, remote_url
        );
        return run_git_remote_command(repo, "fetch", remote_name, &["fetch", remote_name]);
    }

    let repo_lock = repo.inner.write().unwrap();
    let config = repo_lock.config().map_err(|e| GitError::RemoteFailed {
        remote: remote_name.to_string(),
        details: e.to_string(),
    })?;
    let mut remote = repo_lock
        .find_remote(remote_name)
        .map_err(|e| GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: e.to_string(),
        })?;

    let mut callbacks = build_remote_callbacks(config, credentials);

    callbacks.transfer_progress(|progress| {
        info!(
            "Fetch progress: {}/{} objects",
            progress.received_objects(),
            progress.total_objects()
        );
        true
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let refspecs = ["refs/heads/*:refs/remotes/*"];
    remote
        .fetch(&refspecs, Some(&mut fetch_options), None)
        .map_err(|e| GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: e.to_string(),
        })?;

    info!("Fetch completed successfully");
    Ok(())
}

pub(super) fn push_with_options(
    repo: &Repository,
    remote_name: &str,
    branch_name: &str,
    options: PushOptions<'_>,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    info!(
        "Pushing branch '{}' to remote '{}'",
        branch_name, remote_name
    );

    let target_branch = normalize_target_branch(branch_name, options);
    let refspec = build_push_refspec(branch_name, target_branch);
    let should_set_upstream =
        options.set_upstream || should_auto_set_upstream(repo, branch_name, target_branch);
    let requires_system_git = should_use_system_git_for_push(repo, credentials)
        || should_set_upstream
        || options.force_with_lease
        || options.push_tags
        || branch_name != target_branch
        || is_explicit_refspec(branch_name);

    if requires_system_git {
        info!("Using system git for push with options {:?}", options);
        return run_git_remote_command_with_owned_args(
            repo,
            "push",
            remote_name,
            build_push_args(repo, remote_name, branch_name, options),
        );
    }

    // Try libgit2 first (handles SSH keys from agent + ~/.ssh/ + credential helpers)
    let libgit2_result = (|| -> Result<(), GitError> {
        let repo_lock = repo.inner.write().unwrap();
        let config = repo_lock.config().map_err(|e| GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: e.to_string(),
        })?;
        let mut remote =
            repo_lock
                .find_remote(remote_name)
                .map_err(|e| GitError::RemoteFailed {
                    remote: remote_name.to_string(),
                    details: e.to_string(),
                })?;

        let mut callbacks = build_remote_callbacks(config, credentials);
        callbacks.push_update_reference(|refname, msg| {
            info!("Push update: {} - {:?}", refname, msg);
            Ok(())
        });

        let mut push_options = Git2PushOptions::new();
        push_options.remote_callbacks(callbacks);

        remote
            .push(&[&refspec], Some(&mut push_options))
            .map_err(|e| GitError::RemoteFailed {
                remote: remote_name.to_string(),
                details: e.to_string(),
            })?;

        info!("Push completed successfully via libgit2");
        Ok(())
    })();

    if libgit2_result.is_ok() {
        return libgit2_result;
    }

    // Fallback to system git (handles edge cases libgit2 can't)
    info!(
        "libgit2 push failed ({}), falling back to system git",
        libgit2_result.as_ref().unwrap_err()
    );
    run_git_remote_command_with_owned_args(
        repo,
        "push",
        remote_name,
        build_push_args(repo, remote_name, branch_name, options),
    )
}

pub(super) fn pull_with_options(
    repo: &Repository,
    remote_name: &str,
    options: PullOptions<'_>,
    _credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    info!(
        "Pulling from remote '{}' with options {:?}",
        remote_name, options
    );

    let repo_path = repo.command_cwd();
    let args = build_pull_args(repo, remote_name, options)?;
    let output = git_command()
        .args(&args)
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "pull".to_string(),
            details: format!("Failed to execute git pull: {}", e),
        })?;

    if !output.status.success() {
        return Err(GitError::OperationFailed {
            operation: "pull".to_string(),
            details: format!(
                "git pull failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    info!("Pull completed successfully");
    Ok(())
}

pub(super) fn build_remote_callbacks(
    config: Config,
    credentials: Option<(&str, &str)>,
) -> RemoteCallbacks<'static> {
    let explicit_username = credentials
        .map(|(username, _)| username.trim().to_string())
        .filter(|username| !username.is_empty());
    let explicit_password =
        credentials.and_then(|(_, password)| (!password.is_empty()).then(|| password.to_string()));

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |url, username_from_url, allowed_types| {
        if allowed_types.is_user_pass_plaintext() {
            if let (Some(username), Some(password)) =
                (explicit_username.as_deref(), explicit_password.as_deref())
            {
                return Cred::userpass_plaintext(username, password);
            }
        }

        let auth_username =
            resolve_auth_username(&config, url, explicit_username.as_deref(), username_from_url);

        if allowed_types.is_username() {
            if let Some(username) = auth_username.as_deref() {
                return Cred::username(username);
            }
        }

        if allowed_types.is_ssh_key() {
            if let Some(username) = auth_username.as_deref() {
                // 1. Try SSH agent first
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }

                // 2. Try common SSH key files from ~/.ssh/
                let ssh_dir = dirs_next::home_dir()
                    .map(|h| h.join(".ssh"))
                    .unwrap_or_default();
                let key_names = [
                    "id_ed25519",
                    "id_rsa",
                    "id_ecdsa",
                    "id_dsa",
                ];
                for key_name in &key_names {
                    let private_key = ssh_dir.join(key_name);
                    if private_key.exists() {
                        let public_key = ssh_dir.join(format!("{key_name}.pub"));
                        let pub_path = public_key.exists().then_some(public_key.as_path());
                        if let Ok(cred) =
                            Cred::ssh_key(username, pub_path, &private_key, None)
                        {
                            return Ok(cred);
                        }
                    }
                }
            }
        }

        if allowed_types.is_user_pass_plaintext() {
            if let Ok(cred) =
                Cred::credential_helper(&config, url, explicit_username.as_deref().or(username_from_url))
            {
                return Ok(cred);
            }
        }

        if allowed_types.is_default() {
            if let Ok(cred) = Cred::default() {
                return Ok(cred);
            }
        }

        Err(Git2Error::from_str(
            "failed to resolve remote credentials from manual input, ssh-agent, or git credential helper",
        ))
    });

    callbacks
}

pub(super) fn run_git_remote_command(
    repo: &Repository,
    operation: &str,
    remote_name: &str,
    args: &[&str],
) -> Result<(), GitError> {
    let output = git_command()
        .args(args)
        .current_dir(repo.command_cwd())
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: format!("Failed to execute git {operation}: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };

        return Err(GitError::RemoteFailed {
            remote: remote_name.to_string(),
            details: format!("git {operation} failed: {details}"),
        });
    }

    Ok(())
}

pub(super) fn run_git_remote_command_with_owned_args(
    repo: &Repository,
    operation: &str,
    remote_name: &str,
    args: Vec<String>,
) -> Result<(), GitError> {
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_git_remote_command(repo, operation, remote_name, &arg_refs)
}
