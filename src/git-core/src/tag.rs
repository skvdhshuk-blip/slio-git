//! Tag operations for git-core

use crate::error::GitError;
use crate::repository::Repository;
use git2::PushOptions as Git2PushOptions;
use log::info;

/// A Git tag
#[derive(Debug, Clone, Default)]
pub struct TagInfo {
    pub name: String,
    pub target: String,
    pub message: Option<String>,
    pub tagger_name: Option<String>,
    pub tagger_email: Option<String>,
    pub tagged_time: Option<i64>,
}

/// List all tags with full metadata.
pub fn list_tags(repo: &Repository) -> Result<Vec<TagInfo>, GitError> {
    info!("Listing all tags");
    let repo_lock = repo.inner.read().unwrap();
    let names = repo_lock.tag_names(None).map_err(|e| GitError::OperationFailed {
        operation: "list_tags".to_string(),
        details: e.to_string(),
    })?;

    let mut tags = Vec::new();
    for name in names.iter().flatten() {
        let object = match repo_lock.revparse_single(&format!("refs/tags/{name}")) {
            Ok(object) => object,
            Err(_) => continue,
        };
        let mut info = TagInfo {
            name: name.to_string(),
            target: object.id().to_string(),
            ..TagInfo::default()
        };
        if let Ok(tag) = object.peel_to_tag() {
            info.target = tag
                .target_id()
                .to_string()
                .chars()
                .take(12)
                .collect();
            let message = tag.message().unwrap_or("").trim();
            if !message.is_empty() {
                info.message = Some(message.to_string());
            }
            let tagger = tag.tagger();
            if let Some(tagger) = tagger {
                info.tagger_name = Some(tagger.name().unwrap_or("").to_string());
                info.tagger_email = Some(tagger.email().unwrap_or("").to_string());
                info.tagged_time = Some(tagger.when().seconds());
            }
        } else if let Ok(commit) = object.peel_to_commit() {
            info.target = commit.id().to_string().chars().take(12).collect();
        }
        tags.push(info);
    }
    Ok(tags)
}

/// Create an annotated tag
pub fn create_tag(
    repo: &Repository,
    name: &str,
    target: &str,
    message: &str,
    tagger_name: &str,
    tagger_email: &str,
) -> Result<String, GitError> {
    info!("Creating annotated tag '{}' at {}", name, target);
    let repo_lock = repo.inner.write().unwrap();
    let object = repo_lock.revparse_single(target).map_err(|e| GitError::OperationFailed {
        operation: "create_tag".to_string(),
        details: e.to_string(),
    })?;
    let tagger = git2::Signature::now(tagger_name, tagger_email).map_err(|e| {
        GitError::OperationFailed {
            operation: "create_tag".to_string(),
            details: e.to_string(),
        }
    })?;
    repo_lock
        .tag(name, &object, &tagger, message, false)
        .map_err(|e| GitError::OperationFailed {
            operation: "create_tag".to_string(),
            details: e.to_string(),
        })?;
    Ok(name.to_string())
}

/// Create a lightweight tag
pub fn create_lightweight_tag(
    repo: &Repository,
    name: &str,
    target: &str,
) -> Result<String, GitError> {
    info!("Creating lightweight tag '{}' at {}", name, target);
    let repo_lock = repo.inner.write().unwrap();
    let object = repo_lock.revparse_single(target).map_err(|e| GitError::OperationFailed {
        operation: "create_lightweight_tag".to_string(),
        details: e.to_string(),
    })?;
    repo_lock
        .tag_lightweight(name, &object, false)
        .map_err(|e| GitError::OperationFailed {
            operation: "create_lightweight_tag".to_string(),
            details: e.to_string(),
        })?;
    Ok(name.to_string())
}

/// Delete a tag
pub fn delete_tag(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!("Deleting tag '{}'", name);
    let repo_lock = repo.inner.write().unwrap();
    repo_lock.tag_delete(name).map_err(|e| GitError::OperationFailed {
        operation: "delete_tag".to_string(),
        details: e.to_string(),
    })?;
    Ok(())
}

/// Push a tag to a remote
pub fn push_tag(repo: &Repository, tag_name: &str, remote: &str) -> Result<(), GitError> {
    info!("Pushing tag '{}' to remote '{}'", tag_name, remote);
    let repo_lock = repo.inner.write().unwrap();
    let config = repo_lock.config().map_err(|e| GitError::RemoteFailed {
        remote: remote.to_string(),
        details: e.to_string(),
    })?;
    let mut remote_obj = repo_lock.find_remote(remote).map_err(|e| GitError::RemoteFailed {
        remote: remote.to_string(),
        details: e.to_string(),
    })?;
    let callbacks = crate::remote::build_remote_callbacks(config, None);
    let mut options = Git2PushOptions::new();
    options.remote_callbacks(callbacks);
    let refspec = format!("refs/tags/{tag_name}");
    remote_obj
        .push(&[&refspec], Some(&mut options))
        .map_err(|e| GitError::RemoteFailed {
            remote: remote.to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

/// Delete a tag from a remote
pub fn delete_remote_tag(repo: &Repository, tag_name: &str, remote: &str) -> Result<(), GitError> {
    info!("Deleting tag '{}' from remote '{}'", tag_name, remote);
    let repo_lock = repo.inner.write().unwrap();
    let config = repo_lock.config().map_err(|e| GitError::RemoteFailed {
        remote: remote.to_string(),
        details: e.to_string(),
    })?;
    let mut remote_obj = repo_lock.find_remote(remote).map_err(|e| GitError::RemoteFailed {
        remote: remote.to_string(),
        details: e.to_string(),
    })?;
    let callbacks = crate::remote::build_remote_callbacks(config, None);
    let mut options = Git2PushOptions::new();
    options.remote_callbacks(callbacks);
    let refspec = format!(":refs/tags/{tag_name}");
    remote_obj
        .push(&[&refspec], Some(&mut options))
        .map_err(|e| GitError::RemoteFailed {
            remote: remote.to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}
