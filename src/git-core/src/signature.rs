//! GPG/SSH signature verification for git-core

use crate::error::GitError;
use crate::process::git_command;
use crate::repository::Repository;
use log::info;
use std::collections::HashMap;
use std::sync::RwLock;

/// Reason a signature failed to verify
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerificationFailureReason {
    Unknown,
    Expired,
    ExpiredKey,
    RevokedKey,
    CannotVerify,
}

/// GPG/SSH signature verification result for a commit (sealed enum, 4 states)
#[derive(Debug, Clone, PartialEq)]
pub enum SignatureStatus {
    /// No signature present
    NoSignature,
    /// Signature verified successfully
    Verified { user: String, fingerprint: String },
    /// Signature present but verification failed for a typed reason
    NotVerified { reason: VerificationFailureReason },
    /// Signature is cryptographically bad
    Bad,
}

/// Cache for signature verification results (commit Oid → status)
pub struct SignatureCache {
    cache: RwLock<HashMap<git2::Oid, SignatureStatus>>,
}

impl SignatureCache {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn get(&self, oid: git2::Oid) -> Option<SignatureStatus> {
        self.cache.read().unwrap().get(&oid).cloned()
    }

    pub fn insert(&self, oid: git2::Oid, status: SignatureStatus) {
        self.cache.write().unwrap().insert(oid, status);
    }

    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }
}

impl Default for SignatureCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract and verify the signature of a commit using `git log --format=%G?%n%GS%n%GF`.
///
/// Mirrors IDEA's GitCommitSignatureLoaderBase approach: one subprocess call yields
/// the status code, signer name, and fingerprint together, avoiding fragile stderr
/// regex on `verify-commit --raw`.
pub fn verify_commit_signature(
    repo: &Repository,
    oid: git2::Oid,
) -> Result<SignatureStatus, GitError> {
    info!("Verifying signature for commit: {}", oid);

    let repo_path = repo.command_cwd();
    let commit_id = oid.to_string();

    let output = git_command()
        .args(["log", "-1", "--format=%G?%n%GS%n%GF", &commit_id])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| GitError::OperationFailed {
            operation: "verify_commit_signature".to_string(),
            details: e.to_string(),
        })?;

    if !output.status.success() {
        return Ok(SignatureStatus::NotVerified {
            reason: VerificationFailureReason::CannotVerify,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();

    let status_code = lines.next().unwrap_or("").trim();
    let signer = lines.next().unwrap_or("").trim().to_string();
    let fingerprint = lines.next().unwrap_or("").trim().to_string();

    let result = match status_code {
        "G" => SignatureStatus::Verified {
            user: signer,
            fingerprint,
        },
        "U" => SignatureStatus::NotVerified {
            reason: VerificationFailureReason::Unknown,
        },
        "X" => SignatureStatus::NotVerified {
            reason: VerificationFailureReason::Expired,
        },
        "Y" => SignatureStatus::NotVerified {
            reason: VerificationFailureReason::ExpiredKey,
        },
        "R" => SignatureStatus::NotVerified {
            reason: VerificationFailureReason::RevokedKey,
        },
        "E" => SignatureStatus::NotVerified {
            reason: VerificationFailureReason::CannotVerify,
        },
        "B" => SignatureStatus::Bad,
        _ => SignatureStatus::NoSignature,
    };

    info!("Signature verification for {}: {:?}", commit_id, result);
    Ok(result)
}
