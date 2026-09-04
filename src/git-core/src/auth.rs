//! Remote and commit identity used when the process cannot see ~/.ssh or ~/.gitconfig.

use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    pub imported_ssh_key: Option<PathBuf>,
    pub imported_ssh_pub: Option<PathBuf>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
}

static AUTH: RwLock<AuthContext> = RwLock::new(AuthContext {
    imported_ssh_key: None,
    imported_ssh_pub: None,
    user_name: None,
    user_email: None,
});

pub fn set_context(ctx: AuthContext) {
    if let Ok(mut guard) = AUTH.write() {
        *guard = ctx;
    }
}

pub fn context() -> AuthContext {
    AUTH.read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
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

        set_context(AuthContext::default());
    }
}
