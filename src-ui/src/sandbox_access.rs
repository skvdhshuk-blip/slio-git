//! Security-scoped access is an owned lease. The bookmark-resolved URL stays
//! alive until the last repository, watcher or background task releases it.
#[cfg(test)]
use git_core::requires_bookmarks;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderGrant {
    pub path: PathBuf,
    pub bookmark: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    NeedsReselect { path: PathBuf },
    #[cfg_attr(not(all(target_os = "macos", feature = "app-store")), allow(dead_code))]
    Failed { path: PathBuf, details: String },
}
impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsReselect { path } => write!(f, "请重新选择并授权：{}", path.display()),
            Self::Failed { path, details } => write!(f, "无法访问 {}：{details}", path.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccessLease(Arc<Access>);
#[derive(Debug)]
struct Access {
    requested: PathBuf,
    grant: FolderGrant,
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    url: Option<macos::ScopedUrl>,
}
impl Drop for Access {
    fn drop(&mut self) {
        #[cfg(all(target_os = "macos", feature = "app-store"))]
        if let Some(url) = &self.url {
            macos::stop(url);
        }
    }
}
impl AccessLease {
    pub fn path(&self) -> &Path {
        &self.0.requested
    }
    pub fn grant(&self) -> &FolderGrant {
        &self.0.grant
    }
}

// This weak registry owns no permissions. Dispatch captures strong leases before
// queueing work, so navigation cannot release the URL while a worker uses it.
static ACTIVE: LazyLock<Mutex<Vec<Weak<Access>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
pub fn snapshot() -> Vec<AccessLease> {
    let mut active = ACTIVE.lock().unwrap();
    active.retain(|lease| lease.strong_count() > 0);
    active
        .iter()
        .filter_map(Weak::upgrade)
        .map(AccessLease)
        .collect()
}

pub fn remember_folder(path: PathBuf) -> Result<FolderGrant, AccessError> {
    remember_path(path, true)
}
pub fn remember_file(path: PathBuf) -> Result<FolderGrant, AccessError> {
    remember_path(path, false)
}

pub fn prepare_output(path: &Path) -> Result<(), String> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn remember_path(path: PathBuf, is_directory: bool) -> Result<FolderGrant, AccessError> {
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    let bookmark = Some(
        macos::create_bookmark(&path, is_directory).map_err(|details| AccessError::Failed {
            path: path.clone(),
            details,
        })?,
    );
    #[cfg(not(all(target_os = "macos", feature = "app-store")))]
    let bookmark = {
        let _ = is_directory;
        None
    };
    Ok(FolderGrant { path, bookmark })
}

pub fn absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn requested_under_root(
    grant: &FolderGrant,
    requested: &Path,
    resolved_root: &Path,
) -> Result<PathBuf, AccessError> {
    let suffix = requested
        .strip_prefix(&grant.path)
        .map_err(|_| AccessError::NeedsReselect {
            path: requested.into(),
        })?;
    if suffix
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(AccessError::NeedsReselect {
            path: requested.into(),
        });
    }
    Ok(resolved_root.join(suffix))
}

pub fn acquire(grant: &FolderGrant, requested: &Path) -> Result<AccessLease, AccessError> {
    #[cfg(all(target_os = "macos", feature = "app-store"))]
    let access = {
        let blob = grant
            .bookmark
            .as_deref()
            .ok_or_else(|| AccessError::NeedsReselect {
                path: requested.into(),
            })?;
        let (url, root, stale) =
            macos::resolve_bookmark(blob).map_err(|_| AccessError::NeedsReselect {
                path: requested.into(),
            })?;
        let path = requested_under_root(grant, requested, &root)?;
        macos::start(&url).map_err(|_| AccessError::NeedsReselect {
            path: requested.into(),
        })?;
        // Construct the owner before refreshing, so errors also stop access.
        let mut access = Access {
            requested: path,
            grant: FolderGrant {
                path: root,
                bookmark: grant.bookmark.clone(),
            },
            url: Some(url),
        };
        if stale {
            access.grant.bookmark = Some(
                macos::bookmark_for_url(access.url.as_ref().unwrap()).map_err(|details| {
                    AccessError::Failed {
                        path: requested.into(),
                        details,
                    }
                })?,
            );
        }
        access
    };
    #[cfg(not(all(target_os = "macos", feature = "app-store")))]
    let access = Access {
        requested: requested_under_root(grant, requested, &grant.path)?,
        grant: grant.clone(),
    };
    let lease = Arc::new(access);
    ACTIVE.lock().unwrap().push(Arc::downgrade(&lease));
    Ok(AccessLease(lease))
}

pub fn encode_bookmark(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
        bookmark_for_url(&url_from_path(path, is_directory)?)
    }

    pub fn bookmark_for_url(url: &ScopedUrl) -> Result<Vec<u8>, String> {
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
        if !error.is_null() {
            unsafe { CFRelease(error) };
        }
        if data.is_null() {
            return Err("failed to create security-scoped bookmark".to_string());
        }
        let len = unsafe { CFDataGetLength(data) } as usize;
        let ptr = unsafe { CFDataGetBytePtr(data) };
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        unsafe { CFRelease(data) };
        Ok(bytes)
    }

    pub fn resolve_bookmark(blob: &[u8]) -> Result<(ScopedUrl, PathBuf, bool), String> {
        let data = unsafe { CFDataCreate(std::ptr::null(), blob.as_ptr(), blob.len() as CFIndex) };
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
        if !error.is_null() {
            unsafe { CFRelease(error) };
        }
        if url.is_null() {
            return Err("bookmark is stale; reselect the folder".to_string());
        }
        let mut buffer = [0u8; 4096];
        let ok = unsafe {
            CFURLGetFileSystemRepresentation(url, 1, buffer.as_mut_ptr(), buffer.len() as CFIndex)
        };
        let scoped = ScopedUrl(url);
        if ok == 0 {
            return Err("failed to resolve bookmark path".to_string());
        }
        let cstr = unsafe { CStr::from_ptr(buffer.as_ptr() as *const i8) };
        Ok((
            scoped,
            PathBuf::from(cstr.to_string_lossy().into_owned()),
            stale != 0,
        ))
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
        let data = vec![0xde, 0xad, 0xbe, 0xef];
        assert_eq!(decode_bookmark(&encode_bookmark(&data)), Some(data));
        assert_eq!(decode_bookmark("zz"), None);
    }
    #[test]
    fn ancestor_grant_preserves_requested_child_after_moving_the_root() {
        let grant = FolderGrant {
            path: "/old/projects".into(),
            bookmark: None,
        };
        assert_eq!(
            requested_under_root(
                &grant,
                Path::new("/old/projects/repo"),
                Path::new("/new/projects")
            )
            .unwrap(),
            PathBuf::from("/new/projects/repo")
        );
        assert!(
            requested_under_root(
                &grant,
                Path::new("/old/projects/../private"),
                Path::new("/new/projects")
            )
            .is_err()
        );
    }
    #[test]
    fn missing_bookmark_requires_reselection_in_mas() {
        if requires_bookmarks() {
            let grant = FolderGrant {
                path: "/tmp/repo".into(),
                bookmark: None,
            };
            assert!(matches!(
                acquire(&grant, &grant.path),
                Err(AccessError::NeedsReselect { .. })
            ));
        }
    }
    #[test]
    fn lease_is_retained_until_the_last_task_finishes() {
        let access = Arc::new(Access {
            requested: "/test".into(),
            grant: FolderGrant {
                path: "/test".into(),
                bookmark: None,
            },
            #[cfg(all(target_os = "macos", feature = "app-store"))]
            url: None,
        });
        let weak = Arc::downgrade(&access);
        let session = AccessLease(access);
        let worker = session.clone();
        drop(session);
        assert!(weak.upgrade().is_some());
        drop(worker);
        assert!(weak.upgrade().is_none());
    }
}
