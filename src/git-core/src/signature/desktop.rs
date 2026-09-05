use super::*;
use crate::process::git_command;
use log::info;

pub(super) fn verify_commit_signature(
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
