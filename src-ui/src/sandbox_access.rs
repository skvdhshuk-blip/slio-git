//! Folder access for the App Store sandbox.
//!
//! Persistence (path + bookmark blob) is the truth. On desktop builds the
//! bookmark is optional and restore falls back to the raw path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use git_core::requires_bookmarks;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderGrant {
    pub path: PathBuf,
    pub bookmark: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    NeedsReselect { path: PathBuf },
    Failed { path: PathBuf, details: String },
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessError::NeedsReselect { path } => {
                write!(f, "reselect the folder to open {}", path.display())
            }
            AccessError::Failed { path, details } => {
                write!(f, "cannot open {}: {details}", path.display())
            }
        }
    }
}

static ACTIVE: LazyLock<Mutex<HashMap<PathBuf, ActiveAccess>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
struct ActiveAccess {
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    url: macos::ScopedUrl,
}

pub fn remember_folder(path: PathBuf) -> FolderGrant {
    remember_path(path, true)
}

pub fn remember_file(path: PathBuf) -> FolderGrant {
    remember_path(path, false)
}

fn remember_path(path: PathBuf, is_directory: bool) -> FolderGrant {
    let bookmark = create_bookmark(&path, is_directory);
    FolderGrant { path, bookmark }
}

pub fn restore_folder(grant: &FolderGrant) -> Result<PathBuf, AccessError> {
    if !requires_bookmarks() {
        return Ok(grant.path.clone());
    }

    let Some(blob) = grant.bookmark.as_deref() else {
        return Err(AccessError::NeedsReselect {
            path: grant.path.clone(),
        });
    };

    match resolve_bookmark(blob) {
        Ok(path) => {
            start_accessing(&path)?;
            Ok(path)
        }
        Err(details) => Err(AccessError::Failed {
            path: grant.path.clone(),
            details,
        }),
    }
}

pub fn start_accessing(path: &Path) -> Result<(), AccessError> {
    if !requires_bookmarks() {
        return Ok(());
    }
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    {
        if let Ok(guard) = ACTIVE.lock() {
            if guard.contains_key(path) {
                return Ok(());
            }
        }
        let url = macos::url_from_path(path, path.is_dir()).map_err(|details| AccessError::Failed {
            path: path.to_path_buf(),
            details,
        })?;
        macos::start(&url).map_err(|details| AccessError::Failed {
            path: path.to_path_buf(),
            details,
        })?;
        if let Ok(mut guard) = ACTIVE.lock() {
            guard.insert(path.to_path_buf(), ActiveAccess { url });
        }
        return Ok(());
    }
    #[cfg(not(all(target_os = "macos", feature = "app-store")))]
    {
        let _ = path;
        Ok(())
    }
}

pub fn stop_accessing(path: &Path) {
    if let Ok(mut guard) = ACTIVE.lock() {
        if let Some(access) = guard.remove(path) {
            #[cfg(all(target_os = "macos", feature = "app-store"))]
            macos::stop(&access.url);
            let _ = access;
        }
    }
}

fn create_bookmark(path: &Path, is_directory: bool) -> Option<Vec<u8>> {
    if !requires_bookmarks() {
        return None;
    }
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    {
        return macos::create_bookmark(path, is_directory).ok();
    }
    #[cfg(not(all(target_os = "macos", feature = "app-store")))]
    {
        let _ = (path, is_directory);
        None
    }
}

fn resolve_bookmark(blob: &[u8]) -> Result<PathBuf, String> {
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    {
        return macos::resolve_bookmark(blob);
    }
    #[cfg(not(all(target_os = "macos", feature = "app-store")))]
    {
        let _ = blob;
        Err("bookmarks are only available in the Mac App Store build".to_string())
    }
}

pub fn encode_bookmark(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn decode_bookmark(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || text.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let chars: Vec<char> = text.chars().collect();
    for chunk in chars.chunks(2) {
        let hex = format!("{}{}", chunk[0], chunk[1]);
        out.push(u8::from_str_radix(&hex, 16).ok()?);
    }
    Some(out)
}

#[cfg(all(target_os = "macos", feature = "app-store"))]
mod macos {
    use std::ffi::{CStr, c_void};
    use std::path::{Path, PathBuf};

    #[repr(C)]
    struct __CFURL(c_void);
    type CFURLRef = *const __CFURL;
    type CFDataRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFErrorRef = *mut c_void;
    type CFIndex = isize;

    const K_CF_URL_BOOKMARK_CREATION_WITH_SECURITY_SCOPE: usize = 1 << 11;
    const K_CF_URL_BOOKMARK_RESOLUTION_WITH_SECURITY_SCOPE: usize = 1 << 10;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFAllocatorRef,
            buffer: *const u8,
            buf_len: CFIndex,
            is_directory: u8,
        ) -> CFURLRef;
        fn CFURLCreateBookmarkData(
            allocator: CFAllocatorRef,
            url: CFURLRef,
            options: usize,
            resource_properties_to_include: *const c_void,
            relative_to_url: CFURLRef,
            error: *mut CFErrorRef,
        ) -> CFDataRef;
        fn CFURLCreateByResolvingBookmarkData(
            allocator: CFAllocatorRef,
            bookmark: CFDataRef,
            options: usize,
            relative_to_url: CFURLRef,
            resource_properties_to_include: *const c_void,
            is_stale: *mut u8,
            error: *mut CFErrorRef,
        ) -> CFURLRef;
        fn CFURLGetFileSystemRepresentation(
            url: CFURLRef,
            resolve_against_base: u8,
            buffer: *mut u8,
            max_buf_len: CFIndex,
        ) -> u8;
        fn CFURLStartAccessingSecurityScopedResource(url: CFURLRef) -> u8;
        fn CFURLStopAccessingSecurityScopedResource(url: CFURLRef);
        fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
        fn CFDataGetLength(data: CFDataRef) -> CFIndex;
        fn CFDataCreate(allocator: CFAllocatorRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
        fn CFRelease(cf: *const c_void);
    }

    pub struct ScopedUrl(CFURLRef);

    unsafe impl Send for ScopedUrl {}
    unsafe impl Sync for ScopedUrl {}

    impl std::fmt::Debug for ScopedUrl {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("ScopedUrl").finish()
        }
    }

    impl Drop for ScopedUrl {
        fn drop(&mut self) {
            unsafe {
                if !self.0.is_null() {
                    CFRelease(self.0 as *const c_void);
                }
            }
        }
    }

    pub fn url_from_path(path: &Path, is_directory: bool) -> Result<ScopedUrl, String> {
        let bytes = path.to_string_lossy().into_owned().into_bytes();
        let url = unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as CFIndex,
                if is_directory { 1 } else { 0 },
            )
        };
        if url.is_null() {
            return Err("failed to create file URL".to_string());
        }
        Ok(ScopedUrl(url))
    }

    pub fn create_bookmark(path: &Path, is_directory: bool) -> Result<Vec<u8>, String> {
        let url = url_from_path(path, is_directory)?;
        let mut error: CFErrorRef = std::ptr::null_mut();
        let data = unsafe {
            CFURLCreateBookmarkData(
                std::ptr::null(),
                url.0,
                K_CF_URL_BOOKMARK_CREATION_WITH_SECURITY_SCOPE,
                std::ptr::null(),
                std::ptr::null(),
                &mut error,
            )
        };
        if data.is_null() {
            return Err("failed to create security-scoped bookmark".to_string());
        }
        let len = unsafe { CFDataGetLength(data) } as usize;
        let ptr = unsafe { CFDataGetBytePtr(data) };
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        unsafe { CFRelease(data) };
        Ok(bytes)
    }

    pub fn resolve_bookmark(blob: &[u8]) -> Result<PathBuf, String> {
        let data = unsafe {
            CFDataCreate(
                std::ptr::null(),
                blob.as_ptr(),
                blob.len() as CFIndex,
            )
        };
        if data.is_null() {
            return Err("invalid bookmark data".to_string());
        }
        let mut stale = 0u8;
        let mut error: CFErrorRef = std::ptr::null_mut();
        let url = unsafe {
            CFURLCreateByResolvingBookmarkData(
                std::ptr::null(),
                data,
                K_CF_URL_BOOKMARK_RESOLUTION_WITH_SECURITY_SCOPE,
                std::ptr::null(),
                std::ptr::null(),
                &mut stale,
                &mut error,
            )
        };
        unsafe { CFRelease(data) };
        if url.is_null() {
            return Err("bookmark is stale; reselect the folder".to_string());
        }
        let mut buffer = [0u8; 4096];
        let ok = unsafe {
            CFURLGetFileSystemRepresentation(url, 1, buffer.as_mut_ptr(), buffer.len() as CFIndex)
        };
        unsafe { CFRelease(url as *const c_void) };
        if ok == 0 {
            return Err("failed to resolve bookmark path".to_string());
        }
        let cstr = unsafe { CStr::from_ptr(buffer.as_ptr() as *const i8) };
        Ok(PathBuf::from(cstr.to_string_lossy().into_owned()))
    }

    pub fn start(url: &ScopedUrl) -> Result<(), String> {
        let ok = unsafe { CFURLStartAccessingSecurityScopedResource(url.0) };
        if ok == 0 {
            Err("startAccessingSecurityScopedResource failed".to_string())
        } else {
            Ok(())
        }
    }

    pub fn stop(url: &ScopedUrl) {
        unsafe { CFURLStopAccessingSecurityScopedResource(url.0) };
    }

    #[allow(dead_code)]
    fn _cfstring_unused(_: CFStringRef, _: CFAllocatorRef) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmark_hex_roundtrip() {
        let raw = vec![0xde, 0xad, 0xbe, 0xef];
        let encoded = encode_bookmark(&raw);
        assert_eq!(encoded, "deadbeef");
        assert_eq!(decode_bookmark(&encoded), Some(raw));
        assert_eq!(decode_bookmark("zz"), None);
    }

    #[test]
    fn desktop_restore_uses_raw_path() {
        if requires_bookmarks() {
            return;
        }
        let grant = FolderGrant {
            path: PathBuf::from("/tmp/repo"),
            bookmark: None,
        };
        assert_eq!(restore_folder(&grant).unwrap(), PathBuf::from("/tmp/repo"));
    }

    #[test]
    fn store_restore_without_bookmark_asks_to_reselect() {
        if !requires_bookmarks() {
            return;
        }
        let grant = FolderGrant {
            path: PathBuf::from("/tmp/repo"),
            bookmark: None,
        };
        assert!(matches!(
            restore_folder(&grant),
            Err(AccessError::NeedsReselect { .. })
        ));
    }
}
