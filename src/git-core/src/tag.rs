//! Tag operations for git-core

#[cfg_attr(feature = "app-store", path = "tag/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "tag/desktop.rs")]
mod backend;

use crate::error::GitError;
use crate::repository::Repository;
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
    backend::list_tags(repo)
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
    crate::native::reject_active(repo)?;
    backend::create_tag(repo, name, target, message, tagger_name, tagger_email)
}

/// Create a lightweight tag
pub fn create_lightweight_tag(
    repo: &Repository,
    name: &str,
    target: &str,
) -> Result<String, GitError> {
    backend::create_lightweight_tag(repo, name, target)
}

/// Delete a tag
pub fn delete_tag(repo: &Repository, name: &str) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::delete_tag(repo, name)
}

/// Push a tag to a remote
pub fn push_tag(repo: &Repository, tag_name: &str, remote: &str) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::push_tag(repo, tag_name, remote)
}

/// Delete a tag from a remote
pub fn delete_remote_tag(repo: &Repository, tag_name: &str, remote: &str) -> Result<(), GitError> {
    crate::native::reject_active(repo)?;
    backend::delete_remote_tag(repo, tag_name, remote)
}
