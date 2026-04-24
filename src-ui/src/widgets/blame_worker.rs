//! Background worker for computing git blame asynchronously.

use git_core::{BlameInfo, Repository};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BlameRequest {
    pub path: PathBuf,
    pub rev: String,
}

#[derive(Debug, Clone)]
pub struct BlameResult {
    pub path: PathBuf,
    pub rev: String,
    pub result: Result<Vec<BlameInfo>, String>,
}

/// Spawn a blame computation as an iced Task.
/// `repo_workdir` is the repository working directory (passed instead of Repository
/// because git2::Repository is not Send).
pub fn load_blame(repo_workdir: PathBuf, req: BlameRequest) -> iced::Task<BlameResult> {
    iced::Task::perform(
        async move {
            let path = req.path.clone();
            let rev = req.rev.clone();
            let result = tokio::task::spawn_blocking(move || {
                Repository::discover(&repo_workdir)
                    .and_then(|repo| git_core::blame_file(&repo, &path, Some(&rev)))
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            (req.path, req.rev, result)
        },
        |(path, rev, result)| BlameResult { path, rev, result },
    )
}
