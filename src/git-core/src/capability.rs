//! Single source for App Store sandbox capability gates.

/// `true` only when the crate is built with `--features app-store`.
pub fn is_app_store_build() -> bool {
    cfg!(feature = "app-store")
}

/// Spawn `git` from PATH. Off in the store build.
pub fn system_git() -> bool {
    !is_app_store_build()
}

/// Check GitHub Releases for updates. Off in the store build.
pub fn github_updater() -> bool {
    !is_app_store_build()
}

/// Read `~/.ssh` and ssh-agent. Off in the store build.
pub fn implicit_home_ssh() -> bool {
    !is_app_store_build()
}

/// Persist and restore user-selected folders with security-scoped bookmarks.
pub fn requires_bookmarks() -> bool {
    is_app_store_build() && cfg!(target_os = "macos")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_build_keeps_desktop_capabilities() {
        assert_eq!(is_app_store_build(), cfg!(feature = "app-store"));
        assert_eq!(system_git(), !is_app_store_build());
        assert_eq!(github_updater(), !is_app_store_build());
        assert_eq!(implicit_home_ssh(), !is_app_store_build());
        if is_app_store_build() && cfg!(target_os = "macos") {
            assert!(requires_bookmarks());
        } else {
            assert!(!requires_bookmarks());
        }
    }
}
