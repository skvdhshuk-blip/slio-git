//! GitHub release auto-update checker.

use log::info;
use serde::Deserialize;

const GITHUB_REPO: &str = "sk-wang/slio-git";
const GITHUB_API: &str = "https://api.github.com";

/// Information about an available update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub download_url: Option<String>,
    pub release_notes: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Check GitHub for a newer release than `current_version`.
/// Returns `Some(UpdateInfo)` if a newer version exists.
pub async fn check_for_update(current_version: String) -> Result<Option<UpdateInfo>, String> {
    if !crate::capability::github_updater() {
        return Ok(None);
    }
    let current_version = current_version.as_str();
    let url = format!("{}/repos/{}/releases/latest", GITHUB_API, GITHUB_REPO);

    let client = reqwest::Client::builder()
        .user_agent("slio-git-updater")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to check for updates: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned {}", response.status()));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {e}"))?;

    let latest = release.tag_name.trim_start_matches('v');
    let current = current_version.trim_start_matches('v');

    info!("Update check: current={current}, latest={latest}");

    if !is_newer(latest, current) {
        return Ok(None);
    }

    let dmg_asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(".dmg"))
        .map(|a| a.browser_download_url.clone());

    Ok(Some(UpdateInfo {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        release_url: release.html_url,
        download_url: dmg_asset,
        release_notes: release.body.unwrap_or_default(),
    }))
}

/// Simple semver comparison: returns true if `latest` > `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .filter_map(|part| part.parse::<u32>().ok())
            .collect()
    };

    let l = parse(latest);
    let c = parse(current);

    for i in 0..l.len().max(c.len()) {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.0.8", "0.0.7"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.0.7", "0.0.7"));
        assert!(!is_newer("0.0.6", "0.0.7"));
        assert!(is_newer("0.0.10", "0.0.9"));
    }
}
