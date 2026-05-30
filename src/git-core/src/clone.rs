//! Clone operations for git-core

use crate::error::GitError;
use crate::remote::build_remote_callbacks;
use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{Config, FetchOptions};
use log::info;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Options for cloning a repository.
#[derive(Debug, Clone)]
pub struct CloneOptions {
    /// The URL of the remote repository.
    pub url: String,
    /// The parent directory where the clone will be created.
    pub parent_dir: PathBuf,
    /// The name of the directory for the cloned repository.
    pub directory_name: String,
    /// Optional depth for a shallow clone (0 means full clone).
    pub depth: Option<u32>,
    /// Optional branch to checkout after cloning.
    pub branch: Option<String>,
}

/// Progress events emitted during a clone operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloneProgress {
    /// Receiving objects from the remote.
    ReceivingObjects { pct: u8 },
    /// Resolving deltas.
    ResolvingDeltas { pct: u8 },
    /// Checking out files in the working tree.
    CheckingOutFiles { pct: u8 },
}

impl CloneProgress {
    /// Returns the overall progress as a fraction in `[0.0, 1.0]`.
    pub fn fraction(&self) -> f64 {
        match self {
            CloneProgress::ReceivingObjects { pct }
            | CloneProgress::ResolvingDeltas { pct }
            | CloneProgress::CheckingOutFiles { pct } => f64::from(*pct) / 100.0,
        }
    }
}

/// Clone a remote repository.
///
/// Returns the path to the newly cloned repository on success.
pub fn clone(
    options: &CloneOptions,
    progress_callback: Option<Box<dyn FnMut(CloneProgress) + Send>>,
    credentials: Option<(&str, &str)>,
) -> Result<PathBuf, GitError> {
    let dest = options.parent_dir.join(&options.directory_name);
    info!(
        "Cloning '{}' into '{}'",
        options.url,
        dest.display()
    );

    let config = Config::open_default().map_err(|e| GitError::CloneFailed {
        url: options.url.clone(),
        details: format!("failed to open git config: {e}"),
    })?;

    let mut callbacks = build_remote_callbacks(config, credentials);

    // Wrap the progress callback in Arc<Mutex<..>> so closures can share it
    // while satisfying the 'static lifetime required by RemoteCallbacks.
    let progress_cb: Option<Arc<Mutex<Box<dyn FnMut(CloneProgress) + Send>>>> =
        progress_callback.map(|cb| Arc::new(Mutex::new(cb)));

    callbacks.transfer_progress({
        let progress_cb = progress_cb.clone();
        move |stats| {
            if let Some(ref cb) = progress_cb {
                let total = stats.total_objects();
                if total > 0 {
                    let received = stats.received_objects();
                    let pct = ((received as f64 / total as f64) * 100.0) as u8;
                    if let Ok(mut cb) = cb.lock() {
                        cb(CloneProgress::ReceivingObjects { pct });
                    }
                }
            }
            true
        }
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    if let Some(depth) = options.depth {
        if depth > 0 {
            fetch_options.depth(depth as i32);
        }
    }

    let mut checkout_builder = CheckoutBuilder::new();
    if let Some(ref progress_cb) = progress_cb {
        let progress_cb = progress_cb.clone();
        checkout_builder.progress(move |_path, current, total| {
            if total > 0 {
                let pct = ((current as f64 / total as f64) * 100.0) as u8;
                if let Ok(mut cb) = progress_cb.lock() {
                    cb(CloneProgress::CheckingOutFiles { pct });
                }
            }
        });
    }

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch_options);
    builder.with_checkout(checkout_builder);

    if let Some(ref branch) = options.branch {
        builder.branch(branch);
    }

    let repo = builder.clone(&options.url, &dest).map_err(|e| {
        GitError::CloneFailed {
            url: options.url.clone(),
            details: e.to_string(),
        }
    })?;

    // Drop the repo handle to release the lock before returning the path.
    drop(repo);

    info!("Clone completed at '{}'", dest.display());
    Ok(dest)
}

/// Validate a clone URL.
///
/// Returns `Ok(())` if the URL is valid, or a descriptive `GitError` otherwise.
pub fn validate_clone_url(url: &str) -> Result<(), GitError> {
    let trimmed = url.trim();

    if trimmed.is_empty() {
        return Err(GitError::InvalidInput {
            message: "clone URL must not be empty".to_string(),
        });
    }

    // SSH pattern: [user@]host:path
    if is_ssh_url(trimmed) {
        return Ok(());
    }

    // HTTP / HTTPS
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(());
    }

    // SSH:// and git:// scheme URLs (ssh:// already handled by is_ssh_url above)
    if trimmed.starts_with("ssh://") || trimmed.starts_with("git://") {
        return Ok(());
    }

    // Local path that exists on disk
    if Path::new(trimmed).exists() {
        return Ok(());
    }

    Err(GitError::InvalidInput {
        message: format!("'{trimmed}' is not a valid clone URL"),
    })
}

/// Detect whether a URL uses the SCP-style SSH syntax (`[user@]host:path`).
fn is_ssh_url(url: &str) -> bool {
    if url.starts_with("ssh://") {
        return true;
    }
    // If it contains `://` it is a scheme-based URL, not SCP-style.
    if url.contains("://") {
        return false;
    }
    let mut parts = url.splitn(2, ':');
    let Some(left) = parts.next() else {
        return false;
    };
    parts.next().is_some() && !left.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── URL validation tests ──────────────────────────────────────────

    #[test]
    fn validate_rejects_empty_url() {
        assert!(validate_clone_url("").is_err());
        assert!(validate_clone_url("  ").is_err());
    }

    #[test]
    fn validate_accepts_https_url() {
        assert!(validate_clone_url("https://github.com/user/repo.git").is_ok());
    }

    #[test]
    fn validate_accepts_http_url() {
        assert!(validate_clone_url("http://example.com/repo.git").is_ok());
    }

    #[test]
    fn validate_accepts_ssh_url() {
        assert!(validate_clone_url("git@github.com:user/repo.git").is_ok());
    }

    #[test]
    fn validate_accepts_ssh_url_with_user() {
        assert!(validate_clone_url("admin@gitlab.com:group/project.git").is_ok());
    }

    #[test]
    fn validate_accepts_ssh_scheme_url() {
        assert!(validate_clone_url("ssh://git@github.com/user/repo.git").is_ok());
    }

    #[test]
    fn validate_accepts_git_scheme_url() {
        assert!(validate_clone_url("git://github.com/user/repo.git").is_ok());
    }

    #[test]
    fn validate_accepts_existing_local_path() {
        let dir = tempfile::tempdir().unwrap();
        // The directory exists but is not a git repo -- still valid as a URL.
        assert!(validate_clone_url(dir.path().to_str().unwrap()).is_ok());
    }

    #[test]
    fn validate_rejects_nonexistent_local_path() {
        assert!(validate_clone_url("/no/such/path/at/all").is_err());
    }

    // ── SSH pattern detection tests ───────────────────────────────────

    #[test]
    fn ssh_detects_standard_form() {
        assert!(is_ssh_url("git@github.com:user/repo.git"));
    }

    #[test]
    fn ssh_detects_custom_user() {
        assert!(is_ssh_url("deploy@server:repo.git"));
    }

    #[test]
    fn ssh_detects_ssh_scheme() {
        assert!(is_ssh_url("ssh://git@github.com/user/repo.git"));
    }

    #[test]
    fn ssh_rejects_https() {
        assert!(!is_ssh_url("https://github.com/user/repo.git"));
    }

    #[test]
    fn ssh_rejects_http() {
        assert!(!is_ssh_url("http://example.com/repo.git"));
    }

    #[test]
    fn ssh_rejects_local_path_with_colon() {
        // "/tmp/something:else" has a colon but left side contains '/'
        assert!(!is_ssh_url("/tmp/something:else"));
    }

    // ── CloneProgress::fraction tests ─────────────────────────────────

    #[test]
    fn fraction_zero_percent() {
        let p = CloneProgress::ReceivingObjects { pct: 0 };
        assert!((p.fraction() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn fraction_fifty_percent() {
        let p = CloneProgress::ResolvingDeltas { pct: 50 };
        assert!((p.fraction() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn fraction_hundred_percent() {
        let p = CloneProgress::CheckingOutFiles { pct: 100 };
        assert!((p.fraction() - 1.0).abs() < f64::EPSILON);
    }
}
