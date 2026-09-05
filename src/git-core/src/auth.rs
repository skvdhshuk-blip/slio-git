//! Remote and commit identity used when the process cannot see ~/.ssh or ~/.gitconfig.

use crate::GitError;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Native commit identity never falls through to the machine's global config.
/// Read repository-level values first, then the application's explicit identity.
pub(crate) fn native_signature(
    repo: &git2::Repository,
) -> Result<git2::Signature<'static>, GitError> {
    let settings = context();
    let local = repo.config()?.open_level(git2::ConfigLevel::Local)?;
    let name = local
        .get_string("user.name")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or(settings.user_name)
        .filter(|value| !value.trim().is_empty());
    let email = local
        .get_string("user.email")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or(settings.user_email)
        .filter(|value| !value.trim().is_empty());
    match (name, email) {
        (Some(name), Some(email)) => Ok(git2::Signature::now(&name, &email)?),
        _ => Err(GitError::InvalidInput {
            message: "Set the commit name and email in repository config or application settings"
                .into(),
        }),
    }
}

pub(crate) fn signature(repo: &git2::Repository) -> Result<git2::Signature<'static>, GitError> {
    #[cfg(feature = "app-store")]
    {
        native_signature(repo)
    }
    #[cfg(not(feature = "app-store"))]
    {
        Ok(repo.signature()?)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    pub imported_ssh_key: Option<PathBuf>,
    pub imported_ssh_pub: Option<PathBuf>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
}

struct AuthState {
    context: AuthContext,
    #[cfg_attr(not(feature = "app-store"), allow(dead_code))]
    access: Option<Arc<dyn Send + Sync>>,
}

static AUTH: RwLock<AuthState> = RwLock::new(AuthState {
    context: AuthContext {
        imported_ssh_key: None,
        imported_ssh_pub: None,
        user_name: None,
        user_email: None,
    },
    access: None,
});

pub fn set_context(ctx: AuthContext) {
    set_context_with_access(ctx, None);
}

/// Keep the selected key's permission in the same snapshot as its path. A
/// queued operation cannot accidentally combine a new key with an old lease.
pub fn set_context_with_access(ctx: AuthContext, access: Option<Arc<dyn Send + Sync>>) {
    if let Ok(mut guard) = AUTH.write() {
        *guard = AuthState {
            context: ctx,
            access,
        };
    }
}

pub fn context() -> AuthContext {
    AUTH.read()
        .map(|guard| guard.context.clone())
        .unwrap_or_default()
}

#[cfg(feature = "app-store")]
pub(crate) fn network_context() -> (AuthContext, Option<Arc<dyn Send + Sync>>) {
    let guard = AUTH.read().unwrap();
    (guard.context.clone(), guard.access.clone())
}

pub fn configured_identity() -> Option<(String, String)> {
    let ctx = context();
    let name = ctx.user_name.filter(|s| !s.trim().is_empty())?;
    let email = ctx.user_email.filter(|s| !s.trim().is_empty())?;
    Some((name, email))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_requires_both_fields() {
        set_context(AuthContext {
            user_name: Some("Ada".into()),
            user_email: None,
            ..AuthContext::default()
        });
        assert!(configured_identity().is_none());

        set_context(AuthContext {
            user_name: Some("Ada".into()),
            user_email: Some("ada@example.com".into()),
            ..AuthContext::default()
        });
        assert_eq!(
            configured_identity(),
            Some(("Ada".into(), "ada@example.com".into()))
        );

        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        assert_eq!(native_signature(&repo).unwrap().name(), Some("Ada"));
        repo.config()
            .unwrap()
            .set_str("user.name", "Repository Author")
            .unwrap();
        repo.config().unwrap().set_str("user.email", "").unwrap();
        let signature = native_signature(&repo).unwrap();
        assert_eq!(signature.name(), Some("Repository Author"));
        assert_eq!(signature.email(), Some("ada@example.com"));

        set_context(AuthContext::default());
        assert!(native_signature(&repo).is_err());

        #[cfg(feature = "app-store")]
        {
            let lease = Arc::new(());
            let weak = Arc::downgrade(&lease);
            set_context_with_access(AuthContext::default(), Some(lease));
            let callbacks =
                crate::remote::build_remote_callbacks(git2::Config::new().unwrap(), None);
            set_context(AuthContext::default());
            assert!(
                weak.upgrade().is_some(),
                "changing settings must not revoke the running operation's access"
            );
            drop(callbacks);
            assert!(
                weak.upgrade().is_none(),
                "the last operation must release the access lease"
            );
        }
    }
}
