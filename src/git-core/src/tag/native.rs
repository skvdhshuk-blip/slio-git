//! Native execution for tag.

use super::*;

pub(super) fn list_tags(repo: &Repository) -> Result<Vec<TagInfo>, GitError> {
    info!("Listing all tags");
    let repo_lock = repo.inner.read().unwrap();
    let names = repo_lock
        .tag_names(None)
        .map_err(|e| GitError::OperationFailed {
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
            info.target = tag.target_id().to_string().chars().take(12).collect();
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

pub(super) fn create_tag(
    repo: &Repository,
    name: &str,
    target: &str,
    message: &str,
    tagger_name: &str,
    tagger_email: &str,
) -> Result<String, GitError> {
    info!("Creating annotated tag '{}' at {}", name, target);
    let repo_lock = repo.inner.write().unwrap();
    let object = repo_lock
        .revparse_single(target)
        .map_err(|e| GitError::OperationFailed {
            operation: "create_tag".to_string(),
            details: e.to_string(),
        })?;
    let tagger =
        git2::Signature::now(tagger_name, tagger_email).map_err(|e| GitError::OperationFailed {
            operation: "create_tag".to_string(),
            details: e.to_string(),
        })?;
    repo_lock
        .tag(name, &object, &tagger, message, false)
        .map_err(|e| GitError::OperationFailed {
            operation: "create_tag".to_string(),
            details: e.to_string(),
        })?;
    Ok(name.to_string())
}

pub(super) fn create_lightweight_tag(
    repo: &Repository,
    name: &str,
    target: &str,
) -> Result<String, GitError> {
    info!("Creating lightweight tag '{}' at {}", name, target);
    let repo_lock = repo.inner.write().unwrap();
    let object = repo_lock
        .revparse_single(target)
        .map_err(|e| GitError::OperationFailed {
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

pub(super) fn delete_tag(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!("Deleting tag '{}'", name);
    let repo_lock = repo.inner.write().unwrap();
    repo_lock
        .tag_delete(name)
        .map_err(|e| GitError::OperationFailed {
            operation: "delete_tag".to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

pub(super) fn push_tag(repo: &Repository, tag_name: &str, remote: &str) -> Result<(), GitError> {
    info!("Pushing tag '{}' to remote '{}'", tag_name, remote);
    crate::remote::push_reference(
        repo,
        remote,
        format!("refs/tags/{tag_name}:refs/tags/{tag_name}"),
    )
}

pub(super) fn delete_remote_tag(
    repo: &Repository,
    tag_name: &str,
    remote: &str,
) -> Result<(), GitError> {
    info!("Deleting tag '{}' from remote '{}'", tag_name, remote);
    crate::remote::push_reference(repo, remote, format!(":refs/tags/{tag_name}"))
}
