//! Platform-owned file and URL opening; no command interpreter on macOS.
use std::path::Path;

#[cfg(target_os = "macos")]
fn open(url: &objc2_foundation::NSURL) -> Result<(), String> {
    if objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(url) {
        Ok(())
    } else {
        Err("macOS could not open this file or URL".into())
    }
}

pub fn open_file(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        open(&objc2_foundation::NSURL::fileURLWithPath(
            &objc2_foundation::NSString::from_str(&path.to_string_lossy()),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        open::that(path).map_err(|error| error.to_string())
    }
}

pub fn open_url(value: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url =
            objc2_foundation::NSURL::URLWithString(&objc2_foundation::NSString::from_str(value))
                .ok_or("Invalid URL")?;
        open(&url)
    }
    #[cfg(not(target_os = "macos"))]
    {
        open::that(value).map_err(|error| error.to_string())
    }
}
