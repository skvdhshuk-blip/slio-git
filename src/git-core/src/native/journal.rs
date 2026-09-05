//! Durable, replayable changes to HEAD, the index, and individual worktree files.

use crate::GitError;
use fs2::FileExt;
use git2::{Index, IndexEntry, IndexTime, ObjectType, Oid, Repository};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub(crate) fn recovery(reason: impl Into<String>) -> GitError {
    GitError::RecoveryRequired {
        reason: reason.into(),
    }
}

pub(crate) fn directory(git_dir: &Path) -> PathBuf {
    git_dir.join("slio-operation")
}

pub(crate) struct OperationLock(File);
impl Drop for OperationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub(crate) fn lock(repo: &Repository) -> Result<OperationLock, GitError> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(repo.path().join("slio-operation.lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| recovery("another slio operation is running for this worktree"))?;
    Ok(OperationLock(lock))
}

/// Use a fresh handle so the object and reference backends see the durability
/// option before they are initialized. This overlay never modifies user config.
pub(crate) fn durable_repository(
    repo: &Repository,
) -> Result<(tempfile::NamedTempFile, Repository), GitError> {
    let mut settings = tempfile::NamedTempFile::new_in(repo.path())?;
    settings.write_all(b"[core]\n\tfsyncObjectFiles = true\n")?;
    let durable = Repository::open(repo.path())?;
    durable
        .config()?
        .add_file(settings.path(), git2::ConfigLevel::App, true)?;
    Ok((settings, durable))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Head {
    pub reference: Option<String>,
    pub oid: String,
}

impl Head {
    pub fn read(repo: &Repository) -> Result<Self, GitError> {
        let reference = repo.find_reference("HEAD")?;
        let oid = reference.resolve()?.peel_to_commit()?.id().to_string();
        Ok(Self {
            reference: reference.symbolic_target().map(str::to_owned),
            oid,
        })
    }

    pub fn detached(oid: String) -> Self {
        Self {
            reference: None,
            oid,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub path: Vec<u8>,
    pub oid: String,
    pub mode: u32,
    pub stage: u16,
    pub flags_extended: u16,
    pub assume_valid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IndexImage(pub Vec<Entry>);

impl IndexImage {
    pub fn read(index: &Index) -> Self {
        Self(
            index
                .iter()
                .map(|e| Entry {
                    path: e.path,
                    oid: e.id.to_string(),
                    mode: e.mode,
                    stage: e.flags & 0x3000,
                    flags_extended: e.flags_extended & 0x6000,
                    assume_valid: e.flags & 0x8000 != 0,
                })
                .collect(),
        )
    }

    pub fn install(&self, index: &mut Index) -> Result<(), GitError> {
        index.clear()?;
        for entry in &self.0 {
            checked_relative(&entry.path)?;
            index.add(&IndexEntry {
                path: entry.path.clone(),
                id: Oid::from_str(&entry.oid)?,
                mode: entry.mode,
                flags: entry.stage | if entry.assume_valid { 0x8000 } else { 0 },
                flags_extended: entry.flags_extended,
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                uid: 0,
                gid: 0,
                file_size: 0,
            })?;
        }
        Ok(())
    }

    pub fn paths_changed_from(&self, before: &Self) -> Result<Vec<String>, GitError> {
        let group = |image: &Self| {
            let mut map = BTreeMap::<Vec<u8>, Vec<Entry>>::new();
            for entry in &image.0 {
                map.entry(entry.path.clone())
                    .or_default()
                    .push(entry.clone());
            }
            map
        };
        let after = group(self);
        let before = group(before);
        let paths: BTreeSet<_> = after.keys().chain(before.keys()).cloned().collect();
        paths
            .into_iter()
            .filter(|path| after.get(path) != before.get(path))
            .map(|path| checked_relative(&path))
            .collect()
    }
}

pub(crate) fn checked_relative(bytes: &[u8]) -> Result<String, GitError> {
    let path = std::str::from_utf8(bytes)
        .map_err(|_| recovery("native history requires UTF-8 file names"))?;
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        || path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return Err(recovery("invalid path in native operation"));
    }
    Ok(path.into())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileImage {
    pub blob: String,
    pub mode: u32,
}

fn bytes_for_symlink(path: &Path) -> Result<Vec<u8>, GitError> {
    let target = fs::read_link(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(target.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        Ok(target.to_string_lossy().as_bytes().to_vec())
    }
}

fn read_file(path: &Path) -> Result<Option<(Vec<u8>, u32)>, GitError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(Some((bytes_for_symlink(path)?, 0o120000)));
    }
    if metadata.is_dir() {
        return Ok(Some((Vec::new(), 0o040000)));
    }
    if !metadata.is_file() {
        return Err(recovery(
            "a directory or special file occupies an operation path",
        ));
    }
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    Ok(Some((
        fs::read(path)?,
        if executable { 0o100755 } else { 0o100644 },
    )))
}

pub(crate) fn image(path: &Path, storage: Option<&Path>) -> Result<Option<FileImage>, GitError> {
    let Some((bytes, mode)) = read_file(path)? else {
        return Ok(None);
    };
    let blob = Oid::hash_object(ObjectType::Blob, &bytes)?.to_string();
    if let Some(storage) = storage {
        let destination = storage.join("images").join(&blob);
        if !destination.exists() {
            fs::create_dir_all(destination.parent().unwrap())?;
            atomic_write(&destination, &bytes)?;
        }
    }
    Ok(Some(FileImage { blob, mode }))
}

fn check_parents(root: &Path, relative: &str) -> Result<(), GitError> {
    checked_relative(relative.as_bytes())?;
    let mut path = root.to_path_buf();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for part in &parts[..parts.len() - 1] {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(recovery(
                    "an operation path has a non-directory or symlink parent",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), GitError> {
    atomic_write_mode(path, bytes, None)
}

fn atomic_write_mode(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), GitError> {
    let parent = path
        .parent()
        .ok_or_else(|| recovery("missing file parent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    let _ = mode;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    sync_directory(parent)?;
    Ok(())
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), GitError> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    let _ = path;
    Ok(())
}

fn install_file(
    root: &Path,
    relative: &str,
    desired: &Option<FileImage>,
    storage: &Path,
) -> Result<(), GitError> {
    let path = root.join(relative);
    if image(&path, None)? == *desired {
        return Ok(());
    }
    check_parents(root, relative)?;
    let Some(desired) = desired else {
        if fs::symlink_metadata(&path).is_ok() {
            if path.is_dir() && !fs::symlink_metadata(&path)?.file_type().is_symlink() {
                remove_empty_directories(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
            sync_directory(path.parent().unwrap())?;
        }
        return Ok(());
    };
    if desired.mode == 0o040000 {
        if fs::symlink_metadata(&path).is_ok() {
            fs::remove_file(&path)?;
            checkpoint("directory-removed")?;
        }
        fs::create_dir_all(&path)?;
        sync_directory(path.parent().unwrap())?;
        return Ok(());
    }
    let oid = Oid::from_str(&desired.blob)?;
    let bytes = fs::read(storage.join("images").join(oid.to_string()))?;
    if Oid::hash_object(ObjectType::Blob, &bytes)? != oid {
        return Err(recovery("operation file snapshot is corrupt"));
    }
    fs::create_dir_all(path.parent().unwrap())?;
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
        remove_empty_directories(&path)?;
        checkpoint("directory-removed")?;
    }
    if desired.mode == 0o120000 {
        let staging = tempfile::tempdir_in(path.parent().unwrap())?;
        let link = staging.path().join("link");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            std::os::unix::fs::symlink(std::ffi::OsString::from_vec(bytes), &link)?;
        }
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(
            String::from_utf8(bytes).map_err(|_| recovery("invalid symlink"))?,
            &link,
        )?;
        fs::rename(link, &path)?;
    } else {
        atomic_write_mode(
            &path,
            &bytes,
            Some(if desired.mode == 0o100755 {
                0o755
            } else {
                0o644
            }),
        )?;
    }
    sync_directory(path.parent().unwrap())?;
    Ok(())
}

fn remove_empty_directories(path: &Path) -> Result<(), GitError> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            return Err(recovery("an unrelated file prevents directory replacement"));
        }
        remove_empty_directories(&entry.path())?;
    }
    fs::remove_dir(path)?;
    Ok(())
}

// Reading a child of a file/symlink must never follow it outside the worktree.
// Such a child is absent only when the parent is in this transition's write set.
pub(crate) fn transition_image(
    root: &Path,
    relative: &str,
    paths: &BTreeSet<&str>,
    storage: Option<&Path>,
) -> Result<Option<FileImage>, GitError> {
    let mut prefix = PathBuf::new();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for part in &parts[..parts.len() - 1] {
        prefix.push(part);
        match fs::symlink_metadata(root.join(&prefix)) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) if paths.contains(prefix.to_str().unwrap_or_default()) => return Ok(None),
            Ok(_) => {
                return Err(recovery(
                    "an operation path has an unrelated file or symlink parent",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    image(&root.join(relative), storage)
}

fn validate_directory_replacement(
    root: &Path,
    relative: &Path,
    files: &[FileChange],
) -> Result<(), GitError> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            validate_directory_replacement(root, &path, files)?;
        } else if !files
            .iter()
            .any(|change| Path::new(&change.path) == path && change.after.is_none())
        {
            return Err(recovery("an unrelated file prevents directory replacement"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FileChange {
    pub path: String,
    pub before: Option<FileImage>,
    pub after: Option<FileImage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Transition {
    pub before_head: Head,
    pub after_head: Head,
    pub before_index: IndexImage,
    pub after_index: IndexImage,
    pub files: Vec<FileChange>,
    pub next_phase: String,
    pub next_cursor: usize,
    pub next_tip: Option<String>,
}

impl Transition {
    pub fn prepare(
        repo: &Repository,
        desired: &mut Index,
        after_head: Head,
        storage: &Path,
    ) -> Result<Self, GitError> {
        let before_index = IndexImage::read(&repo.index()?);
        let after_index = IndexImage::read(desired);
        let paths = after_index.paths_changed_from(&before_index)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| recovery("operation needs a worktree"))?;
        let preview = tempfile::tempdir_in(storage)?;
        if !paths.is_empty() {
            let mut checkout = git2::build::CheckoutBuilder::new();
            checkout
                .force()
                .allow_conflicts(true)
                .conflict_style_merge(true)
                .update_index(false)
                .target_dir(preview.path());
            for path in &paths {
                checkout.path(path);
            }
            // A preview has an empty baseline, including for file/directory
            // replacements. Reusing the live HEAD here makes libgit2 compare
            // against paths that do not exist in this empty directory.
            let preview_repo = Repository::init(preview.path())?;
            preview_repo.set_odb(&repo.odb()?)?;
            let source_config = repo.config()?;
            let mut preview_config = preview_repo.config()?;
            for key in [
                "core.autocrlf",
                "core.eol",
                "core.safecrlf",
                "core.symlinks",
                "core.attributesfile",
            ] {
                match source_config.get_string(key) {
                    Ok(value) => preview_config.set_str(key, &value)?,
                    Err(error) if error.code() == git2::ErrorCode::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let mut preview_index = Index::new()?;
            after_index.install(&mut preview_index)?;
            preview_repo.checkout_index(Some(&mut preview_index), Some(&mut checkout))?;
        }
        let mut files = Vec::new();
        let path_set = paths.iter().map(String::as_str).collect();
        for path in &paths {
            files.push(FileChange {
                before: transition_image(workdir, path, &path_set, Some(storage))?,
                after: transition_image(preview.path(), path, &path_set, Some(storage))?,
                path: path.clone(),
            });
        }
        Ok(Self {
            before_head: Head::read(repo)?,
            after_head,
            before_index,
            after_index,
            files,
            next_phase: "ready".into(),
            next_cursor: 0,
            next_tip: None,
        })
    }

    pub fn apply(&self, repo: &Repository, storage: &Path) -> Result<(), GitError> {
        let current_head = Head::read(repo)?;
        if current_head != self.before_head && current_head != self.after_head {
            return Err(recovery("HEAD changed outside the active operation"));
        }
        let mut index = repo.index()?;
        index.read(true)?;
        let current_index = IndexImage::read(&index);
        if current_index != self.before_index && current_index != self.after_index {
            return Err(recovery("the index changed outside the active operation"));
        }
        let workdir = repo
            .workdir()
            .ok_or_else(|| recovery("operation needs a worktree"))?;
        // Validate the entire write set before changing any file.
        let paths = self
            .files
            .iter()
            .map(|change| change.path.as_str())
            .collect();
        for change in &self.files {
            let actual = transition_image(workdir, &change.path, &paths, None)?;
            let replacing_directory = change
                .before
                .as_ref()
                .is_some_and(|image| image.mode == 0o040000)
                || change
                    .after
                    .as_ref()
                    .is_some_and(|image| image.mode == 0o040000);
            if actual != change.before
                && actual != change.after
                && !(actual.is_none() && replacing_directory)
            {
                return Err(recovery(format!(
                    "{} changed outside the active operation",
                    change.path
                )));
            }
            if actual.as_ref().is_some_and(|image| image.mode == 0o040000)
                && !change
                    .after
                    .as_ref()
                    .is_some_and(|image| image.mode == 0o040000)
            {
                validate_directory_replacement(workdir, Path::new(&change.path), &self.files)?;
            }
        }
        let mut ordered: Vec<_> = self.files.iter().collect();
        ordered.sort_by_key(|change| {
            let depth = change.path.matches('/').count();
            match &change.after {
                None => (0, usize::MAX - depth),
                Some(image) if image.mode == 0o040000 => (1, depth),
                _ => (2, depth),
            }
        });
        for change in ordered {
            if transition_image(workdir, &change.path, &paths, None)? == change.after {
                continue;
            }
            install_file(workdir, &change.path, &change.after, storage)?;
            checkpoint("file")?;
        }
        if current_index != self.after_index {
            self.after_index.install(&mut index)?;
            index.write()?;
            if let Some(path) = index.path() {
                File::open(path)?.sync_all()?;
                sync_directory(path.parent().unwrap())?;
            }
        }
        checkpoint("index")?;
        if current_head != self.after_head {
            if let Some(reference) = &self.after_head.reference {
                repo.set_head(reference)?;
            } else {
                repo.set_head_detached(Oid::from_str(&self.after_head.oid)?)?;
            }
        }
        checkpoint("head")?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Kind {
    Rebase,
    CherryPick,
    Revert,
    Merge,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Step {
    pub action: String,
    pub commit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub version: u32,
    pub id: String,
    pub kind: Kind,
    pub original_head: Head,
    pub original_index: IndexImage,
    pub original_files: BTreeMap<String, Option<FileImage>>,
    pub expected_files: BTreeMap<String, Option<FileImage>>,
    pub dirty_paths: Vec<String>,
    pub base: Option<String>,
    pub tip: Option<String>,
    pub expected_head: Head,
    pub steps: Vec<Step>,
    pub cursor: usize,
    pub message: Option<String>,
    pub generated: Vec<String>,
    pub phase: String,
    pub transition: Option<Transition>,
    pub expected_index: IndexImage,
    pub force_autocrlf: bool,
    pub committer_name: String,
    pub committer_email: String,
    pub committer_time: i64,
    pub committer_offset: i32,
}

impl Journal {
    pub fn load(repo: &Repository) -> Result<Option<Self>, GitError> {
        let path = directory(repo.path()).join("state.json");
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let journal: Self = serde_json::from_slice(&bytes)
            .map_err(|_| recovery("native operation journal is corrupt; keep it for recovery"))?;
        if journal.version != 1 || journal.cursor > journal.steps.len() {
            return Err(recovery(
                "unsupported native operation journal version or position",
            ));
        }
        Ok(Some(journal))
    }

    pub fn save(&self, repo: &Repository) -> Result<(), GitError> {
        let storage = directory(repo.path());
        fs::create_dir_all(&storage)?;
        sync_directory(repo.path())?;
        atomic_write(
            &storage.join("state.json"),
            &serde_json::to_vec_pretty(self).map_err(|e| recovery(e.to_string()))?,
        )?;
        checkpoint("journal")
    }

    pub fn validate_identity(&self, repo: &Repository) -> Result<(), GitError> {
        if let Some(reference) = &self.original_head.reference {
            let actual = repo
                .find_reference(reference)?
                .target()
                .map(|id| id.to_string());
            if actual.as_deref() != Some(&self.original_head.oid)
                && !(matches!(self.phase.as_str(), "publishing" | "completed")
                    && actual == self.tip)
            {
                return Err(recovery(
                    "the original branch changed outside the active operation",
                ));
            }
        }
        Ok(())
    }

    pub fn begin_transition(
        &mut self,
        repo: &Repository,
        transition: Transition,
    ) -> Result<(), GitError> {
        for change in &transition.files {
            if self.dirty_paths.iter().any(|dirty| {
                dirty == &change.path
                    || dirty.starts_with(&format!("{}/", change.path))
                    || change.path.starts_with(&format!("{dirty}/"))
            }) {
                return Err(GitError::DirtyWorkingTree);
            }
            self.original_files
                .entry(change.path.clone())
                .or_insert_with(|| change.before.clone());
        }
        self.transition = Some(transition);
        self.phase = "transition".into();
        self.save(repo)?;
        self.finish_transition(repo)
    }

    pub fn finish_transition(&mut self, repo: &Repository) -> Result<(), GitError> {
        self.validate_identity(repo)?;
        let transition = self
            .transition
            .as_ref()
            .ok_or_else(|| recovery("operation is missing its transition"))?;
        transition.apply(repo, &directory(repo.path()))?;
        self.phase = transition.next_phase.clone();
        self.cursor = transition.next_cursor;
        self.tip = transition.next_tip.clone();
        self.expected_head = transition.after_head.clone();
        self.expected_index = transition.after_index.clone();
        for file in &transition.files {
            self.expected_files
                .insert(file.path.clone(), file.after.clone());
        }
        self.transition = None;
        self.save(repo)
    }
}

#[cfg(test)]
thread_local! {
    static FAILPOINT: std::cell::RefCell<Option<(&'static str, usize)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn fail_after(point: &'static str, hits: usize) {
    FAILPOINT.with(|value| *value.borrow_mut() = Some((point, hits)));
}

pub(crate) fn checkpoint(point: &str) -> Result<(), GitError> {
    #[cfg(test)]
    {
        let fail = FAILPOINT.with(|value| {
            let mut value = value.borrow_mut();
            if let Some((name, hits)) = value.as_mut() {
                if *name == point {
                    if *hits == 0 {
                        *value = None;
                        return true;
                    }
                    *hits -= 1;
                }
            }
            false
        });
        if fail {
            // Only the isolated crash-test child sets this path. Keep the
            // live lock and stack intact until the parent sends SIGKILL.
            if let Some(marker) = std::env::var_os("SLIO_TEST_KILL_CHECKPOINT") {
                std::fs::write(marker, point)?;
                loop {
                    std::thread::park();
                }
            }
            return Err(recovery(format!("simulated interruption after {point}")));
        }
    }
    let _ = point;
    Ok(())
}
