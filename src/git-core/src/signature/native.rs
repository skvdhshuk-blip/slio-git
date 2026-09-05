use super::*;

pub(super) fn verify_commit_signature(
    repo: &Repository,
    oid: git2::Oid,
) -> Result<SignatureStatus, GitError> {
    let raw = repo.inner.read().unwrap();
    match raw.extract_signature(&oid, None) {
        Ok(_) => Ok(SignatureStatus::NotVerified {
            reason: VerificationFailureReason::CannotVerify,
        }),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(SignatureStatus::NoSignature),
        Err(error) => Err(error.into()),
    }
}
