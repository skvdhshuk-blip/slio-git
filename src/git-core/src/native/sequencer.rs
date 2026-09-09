//! Native history actions. Immutable Git objects are prepared first, journaled
//! worktree transitions follow, and the original branch is published last.

use super::journal::{
    self, FileChange, Head, IndexImage, Journal, Kind, Step, Transition, checked_relative, recovery,
};
use crate::{GitError, RebaseTodoEntry, Repository};
use git2::{Index, Oid};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn status(repo: &Repository) -> Result<Option<Journal>, GitError> {
    Journal::load(&repo.inner.read().unwrap())
}

fn oid(text: &str) -> Result<Oid, GitError> {
    Oid::from_str(text).map_err(Into::into)
}

fn tree<'a>(repo: &'a git2::Repository, tip: Option<&str>) -> Result<git2::Tree<'a>, GitError> {
    match tip {
        Some(tip) => Ok(repo.find_commit(Oid::from_str(tip)?)?.tree()?),
        None => Ok(repo.find_tree(repo.treebuilder(None)?.write()?)?),
    }
}

fn signature(journal: &Journal) -> Result<git2::Signature<'static>, GitError> {
    Ok(git2::Signature::new(
        &journal.committer_name,
        &journal.committer_email,
        &git2::Time::new(journal.committer_time, journal.committer_offset),
    )?)
}

fn recovery_ref(journal: &Journal) -> String {
    format!("refs/slio/recovery/{}", journal.id)
}

fn validate_steps(repo: &git2::Repository, steps: &[Step], kind: &Kind) -> Result<(), GitError> {
    let mut seen = BTreeSet::new();
    let mut retained = false;
    for step in steps {
        if !matches!(
            step.action.as_str(),
            "pick"
                | "edit"
                | "reword"
                | "fixup"
                | "squash"
                | "drop"
                | "merge"
                | "squash-merge"
                | "fast-forward"
        ) {
            return Err(GitError::InvalidInput {
                message: "unsupported native history action".into(),
            });
        }
        let commit = repo.find_commit(oid(&step.commit)?)?;
        if *kind != Kind::Merge && commit.parent_count() > 1 {
            return Err(GitError::InvalidInput {
                message: "select a linear range; merge commits require an explicit mainline".into(),
            });
        }
        if !seen.insert(commit.id()) {
            return Err(GitError::InvalidInput {
                message: "a history plan cannot contain duplicate commits".into(),
            });
        }
        if !retained && matches!(step.action.as_str(), "fixup" | "squash") {
            return Err(GitError::InvalidInput {
                message: "the first retained commit cannot be fixup or squash".into(),
            });
        }
        retained |= step.action != "drop";
    }
    if *kind != Kind::Rebase && steps.len() != 1 {
        return Err(GitError::InvalidInput {
            message: "invalid native history plan".into(),
        });
    }
    Ok(())
}

pub(crate) fn start(
    repo: &Repository,
    kind: Kind,
    base: Option<String>,
    entries: &[RebaseTodoEntry],
    force_autocrlf: bool,
) -> Result<bool, GitError> {
    let guard = repo.inner.write().unwrap();
    let _lock = journal::lock(&guard)?;
    let (_settings, repo) = journal::durable_repository(&guard)?;
    if Journal::load(&repo)?.is_some() {
        return Err(recovery("finish or abort the active slio operation first"));
    }
    if repo.state() != git2::RepositoryState::Clean {
        return Err(recovery(
            "finish the existing Git operation in the tool that started it",
        ));
    }
    if repo.is_bare() {
        return Err(GitError::InvalidInput {
            message: "history operations need a working tree".into(),
        });
    }
    let original_head = Head::read(&repo)?;
    let original_index = IndexImage::read(&repo.index()?);
    let steps: Vec<_> = entries
        .iter()
        .map(|entry| Step {
            action: entry.action.trim().to_ascii_lowercase(),
            commit: entry.commit.clone(),
        })
        .collect();
    validate_steps(&repo, &steps, &kind)?;
    if kind == Kind::Rebase && base.is_none() && steps.iter().all(|step| step.action == "drop") {
        return Err(GitError::InvalidInput {
            message: "cannot drop every root commit".into(),
        });
    }
    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_untracked(true)
        .recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut status_opts))?;
    if kind == Kind::Rebase && !statuses.is_empty() {
        return Err(GitError::DirtyWorkingTree);
    }
    if statuses.iter().any(|entry| {
        entry.status().intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE
                | git2::Status::CONFLICTED,
        )
    }) {
        return Err(GitError::DirtyWorkingTree);
    }
    let dirty_paths = statuses
        .iter()
        .map(|entry| checked_relative(entry.path_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    drop(statuses);
    // Fast-forward reuses an existing commit and must work before the user has
    // configured a commit identity. These journal fields are unused for writing
    // commits in this action, so retain the existing commit's identity.
    let committer = if kind == Kind::Merge && steps[0].action == "fast-forward" {
        repo.find_commit(oid(&steps[0].commit)?)?
            .committer()
            .to_owned()
    } else {
        crate::auth::native_signature(&repo)?
    };
    let mut journal = Journal {
        version: 1,
        id: format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ),
        kind,
        original_head: original_head.clone(),
        original_index: original_index.clone(),
        expected_index: original_index,
        original_files: BTreeMap::new(),
        expected_files: BTreeMap::new(),
        dirty_paths,
        base: base.clone(),
        tip: base,
        expected_head: original_head,
        steps,
        cursor: 0,
        message: None,
        generated: Vec::new(),
        phase: "starting".into(),
        transition: None,
        force_autocrlf,
        committer_name: committer.name().unwrap_or_default().into(),
        committer_email: committer.email().unwrap_or_default().into(),
        committer_time: committer.when().seconds(),
        committer_offset: committer.when().offset_minutes(),
    };
    // Reject overlap before saving a journal or detaching HEAD.
    if journal.kind != Kind::Rebase {
        let desired = replay_index(&repo, &journal)?;
        let touched = IndexImage::read(&desired).paths_changed_from(&journal.original_index)?;
        if touched
            .iter()
            .any(|path| journal.dirty_paths.contains(path))
        {
            return Err(GitError::DirtyWorkingTree);
        }
    }
    repo.reference(
        &recovery_ref(&journal),
        oid(&journal.original_head.oid)?,
        false,
        "slio native operation recovery",
    )?;
    journal.save(&repo)?;
    advance(&repo, &mut journal)
}

fn replay_index(repo: &git2::Repository, journal: &Journal) -> Result<Index, GitError> {
    let step = &journal.steps[journal.cursor];
    let source = repo.find_commit(oid(&step.commit)?)?;
    if journal.kind == Kind::Merge {
        let ours = repo.find_commit(oid(journal.tip.as_deref().unwrap())?)?;
        return Ok(repo.merge_commits(&ours, &source, None)?);
    }
    let parent = source.parent_id(0).ok().map(|id| id.to_string());
    let parent_tree = tree(repo, parent.as_deref())?;
    let source_tree = source.tree()?;
    let current_tree = tree(repo, journal.tip.as_deref())?;
    let (base, theirs) = if journal.kind == Kind::Revert {
        (&source_tree, &parent_tree)
    } else {
        (&parent_tree, &source_tree)
    };
    if base.id() == current_tree.id() {
        let mut index = Index::new()?;
        index.read_tree(theirs)?;
        return Ok(index);
    }
    Ok(repo.merge_trees(base, &current_tree, theirs, None)?)
}

fn validate_ready(repo: &git2::Repository, journal: &Journal) -> Result<(), GitError> {
    journal.validate_identity(repo)?;
    if Head::read(repo)? != journal.expected_head {
        return Err(recovery("HEAD changed outside the active operation"));
    }
    let mut index = repo.index()?;
    index.read(true)?;
    if IndexImage::read(&index) != journal.expected_index {
        return Err(recovery(
            "the index changed; amend or resolve the current step before continuing",
        ));
    }
    let workdir = repo.workdir().ok_or_else(|| recovery("missing worktree"))?;
    let paths = journal.expected_files.keys().map(String::as_str).collect();
    for (path, expected) in &journal.expected_files {
        if journal::transition_image(workdir, path, &paths, None)? != *expected {
            return Err(recovery(format!("{path} changed outside the active step")));
        }
    }
    Ok(())
}

fn prepare(
    repo: &git2::Repository,
    journal: &Journal,
    index: &mut Index,
    head: Head,
) -> Result<Transition, GitError> {
    if !journal.force_autocrlf {
        return Transition::prepare(repo, index, head, &journal::directory(repo.path()));
    }
    // A private repository handle and an application-level config overlay keep
    // "-c core.autocrlf=true" scoped to this checkout. No user's config is written.
    let isolated = git2::Repository::open(repo.path())?;
    let storage = journal::directory(repo.path());
    let config = storage.join("checkout-config");
    journal::atomic_write(&config, b"[core]\n\tautocrlf = true\n")?;
    isolated
        .config()?
        .add_file(&config, git2::ConfigLevel::App, true)?;
    Transition::prepare(&isolated, index, head, &storage)
}

fn advance(repo: &git2::Repository, journal: &mut Journal) -> Result<bool, GitError> {
    loop {
        journal.validate_identity(repo)?;
        match journal.phase.as_str() {
            "transition" => journal.finish_transition(repo)?,
            "starting" => {
                validate_ready(repo, journal)?;
                let mut desired = Index::new()?;
                desired.read_tree(&tree(repo, journal.base.as_deref())?)?;
                let head = Head::detached(
                    journal
                        .base
                        .clone()
                        .unwrap_or_else(|| journal.original_head.oid.clone()),
                );
                let mut transition = prepare(repo, journal, &mut desired, head)?;
                transition.next_tip = journal.base.clone();
                journal.begin_transition(repo, transition)?;
            }
            "ready" => {
                validate_ready(repo, journal)?;
                if journal.cursor == journal.steps.len() {
                    journal.phase = "publishing".into();
                    journal.save(repo)?;
                    continue;
                }
                if journal.steps[journal.cursor].action == "drop" {
                    journal.cursor += 1;
                    journal.save(repo)?;
                    continue;
                }
                let mut desired = replay_index(repo, journal)
                    .map_err(|error| recovery(format!("prepare replay: {error}")))?;
                if desired.has_conflicts() {
                    let mut transition =
                        prepare(repo, journal, &mut desired, journal.expected_head.clone())?;
                    transition.next_phase = "conflict".into();
                    transition.next_cursor = journal.cursor;
                    transition.next_tip = journal.tip.clone();
                    journal.begin_transition(repo, transition)?;
                    return Ok(false);
                }
                if journal.kind == Kind::Merge
                    && journal.steps[journal.cursor].action == "squash-merge"
                {
                    let mut transition =
                        prepare(repo, journal, &mut desired, journal.expected_head.clone())?;
                    transition.next_phase = "merge-ready".into();
                    transition.next_cursor = journal.cursor;
                    transition.next_tip = journal.tip.clone();
                    journal.begin_transition(repo, transition)?;
                    return Ok(false);
                }
                commit_step(repo, journal, &mut desired)?;
            }
            "conflict" | "edit" | "empty" | "merge-ready" => return Ok(false),
            "publishing" => {
                publish(repo, journal)?;
                return Ok(true);
            }
            "completed" | "aborted" => {
                cleanup(repo, journal)?;
                return Ok(true);
            }
            phase => return Err(recovery(format!("unknown operation phase: {phase}"))),
        }
    }
}

fn commit_step(
    repo: &git2::Repository,
    journal: &mut Journal,
    index: &mut Index,
) -> Result<(), GitError> {
    let step = journal.steps[journal.cursor].clone();
    let source = repo.find_commit(oid(&step.commit)?)?;
    let current = journal
        .tip
        .as_deref()
        .map(|tip| repo.find_commit(Oid::from_str(tip)?))
        .transpose()?;
    let result_tree = repo.find_tree(
        index
            .write_tree_to(repo)
            .map_err(|error| recovery(format!("write replay tree: {error}")))?,
    )?;
    let source_parent = source.parent_id(0).ok().map(|id| id.to_string());
    let originally_empty = source.tree_id() == tree(repo, source_parent.as_deref())?.id();
    let now_empty = result_tree.id() == tree(repo, journal.tip.as_deref())?.id();
    if now_empty && !originally_empty && step.action == "pick" && journal.kind == Kind::Rebase {
        journal.cursor += 1;
        journal.save(repo)?;
        return Ok(());
    }
    let combine = matches!(step.action.as_str(), "fixup" | "squash");
    let mut parents = if combine {
        current
            .as_ref()
            .ok_or_else(|| recovery("missing squash predecessor"))?
            .parents()
            .collect::<Vec<_>>()
    } else {
        current.into_iter().collect::<Vec<_>>()
    };
    if journal.kind == Kind::Merge && step.action != "squash-merge" {
        parents.push(repo.find_commit(source.id())?);
    }
    let predecessor = journal
        .tip
        .as_deref()
        .map(|tip| repo.find_commit(Oid::from_str(tip)?))
        .transpose()?;
    let author = if combine {
        predecessor.as_ref().unwrap().author()
    } else if matches!(journal.kind, Kind::Revert | Kind::Merge) {
        signature(journal)?
    } else {
        source.author()
    };
    let message = if let Some(message) = &journal.message {
        message.clone()
    } else {
        match step.action.as_str() {
            "merge" => format!("Merge commit '{}'\n", source.id()),
            "squash-merge" => format!("Squash commit '{}'\n", source.id()),
            "fixup" => predecessor
                .as_ref()
                .unwrap()
                .message()
                .unwrap_or_default()
                .to_string(),
            "squash" => format!(
                "{}\n\n{}",
                predecessor
                    .as_ref()
                    .unwrap()
                    .message()
                    .unwrap_or_default()
                    .trim_end(),
                source.message().unwrap_or_default()
            ),
            _ if journal.kind == Kind::Revert => format!(
                "Revert \"{}\"\n\nThis reverts commit {}.\n",
                source.summary().unwrap_or_default(),
                source.id()
            ),
            _ => source.message().unwrap_or_default().to_string(),
        }
    };
    let parent_refs: Vec<_> = parents.iter().collect();
    let candidate = if step.action == "fast-forward" {
        source.id().to_string()
    } else {
        repo.commit(
            None,
            &author,
            &signature(journal)?,
            &message,
            &result_tree,
            &parent_refs,
        )?
        .to_string()
    };
    journal.generated.push(candidate.clone());
    let mut transition = prepare(repo, journal, index, Head::detached(candidate.clone()))
        .map_err(|error| recovery(format!("prepare replay checkout: {error}")))?;
    if journal.phase == "conflict" {
        let paths: BTreeSet<_> = journal
            .expected_index
            .0
            .iter()
            .filter(|entry| entry.stage != 0)
            .map(|entry| checked_relative(&entry.path))
            .collect::<Result<_, _>>()?;
        for path in paths {
            if !transition.files.iter().any(|file| file.path == path) {
                let image = journal::image(
                    &repo.workdir().unwrap().join(&path),
                    Some(&journal::directory(repo.path())),
                )?;
                transition.files.push(FileChange {
                    path,
                    before: image.clone(),
                    after: image,
                });
            }
        }
    }
    transition.next_cursor = journal.cursor + 1;
    transition.next_tip = Some(candidate);
    transition.next_phase = if matches!(step.action.as_str(), "edit" | "reword") {
        "edit"
    } else {
        "ready"
    }
    .into();
    journal.begin_transition(repo, transition)
}

fn publish(repo: &git2::Repository, journal: &mut Journal) -> Result<(), GitError> {
    journal.validate_identity(repo)?;
    let tip = journal
        .tip
        .as_deref()
        .ok_or_else(|| recovery("cannot publish an empty branch"))?;
    let mut index = repo.index()?;
    index.read(true)?;
    if IndexImage::read(&index) != journal.expected_index {
        return Err(recovery("index changed before publication"));
    }
    let paths = journal.expected_files.keys().map(String::as_str).collect();
    for (path, expected) in &journal.expected_files {
        if journal::transition_image(repo.workdir().unwrap(), path, &paths, None)? != *expected {
            return Err(recovery(format!("{path} changed before publication")));
        }
    }
    let current_head = Head::read(repo)?;
    let published_head = Head {
        reference: journal.original_head.reference.clone(),
        oid: tip.into(),
    };
    if current_head != journal.expected_head && current_head != published_head {
        return Err(recovery("HEAD changed before publication"));
    }
    if let Some(reference) = &journal.original_head.reference {
        let actual = repo.find_reference(reference)?.target();
        if actual != Some(oid(tip)?) {
            repo.reference_matching(
                reference,
                oid(tip)?,
                true,
                oid(&journal.original_head.oid)?,
                "slio native history",
            )?;
        }
        journal::checkpoint("publish-ref")?;
        repo.set_head(reference)?;
    } else {
        repo.set_head_detached(oid(tip)?)?;
    }
    journal::checkpoint("publish-head")?;
    journal.phase = "completed".into();
    journal.save(repo)?;
    cleanup(repo, journal)
}

fn cleanup(repo: &git2::Repository, journal: &Journal) -> Result<(), GitError> {
    match repo.find_reference(&recovery_ref(journal)) {
        Ok(mut reference) => reference.delete()?,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::remove_dir_all(journal::directory(repo.path()))?;
    journal::sync_directory(repo.path())?;
    Ok(())
}

pub(crate) fn continue_operation(repo: &Repository) -> Result<bool, GitError> {
    let guard = repo.inner.write().unwrap();
    let _lock = journal::lock(&guard)?;
    let (_settings, repo) = journal::durable_repository(&guard)?;
    let mut journal =
        Journal::load(&repo)?.ok_or_else(|| recovery("no native operation to continue"))?;
    journal.validate_identity(&repo)?;
    if matches!(journal.phase.as_str(), "conflict" | "empty" | "merge-ready") {
        if Head::read(&repo)? != journal.expected_head {
            return Err(recovery("HEAD changed outside the active step"));
        }
        let conflicts: BTreeSet<_> = journal
            .expected_index
            .0
            .iter()
            .filter(|entry| entry.stage != 0)
            .map(|entry| entry.path.clone())
            .collect();
        let resolving = git2::Repository::open(repo.path())?;
        let mut index = resolving.index()?;
        index.read(true)?;
        let actual = IndexImage::read(&index);
        if actual
            .0
            .iter()
            .filter(|e| !conflicts.contains(&e.path))
            .collect::<Vec<_>>()
            != journal
                .expected_index
                .0
                .iter()
                .filter(|e| !conflicts.contains(&e.path))
                .collect::<Vec<_>>()
        {
            return Err(recovery(
                "unrelated index entries changed during conflict resolution",
            ));
        }
        for path in conflicts {
            let relative = checked_relative(&path)?;
            let full = repo.workdir().unwrap().join(&relative);
            if fs::symlink_metadata(&full).is_ok() {
                if fs::read(&full).is_ok_and(|contents| {
                    contents
                        .split(|byte| *byte == b'\n')
                        .any(|line| line.starts_with(b"<<<<<<< "))
                }) {
                    return Err(GitError::MergeConflict);
                }
                index.add_path(Path::new(&relative))?;
            } else {
                index.remove_path(Path::new(&relative))?;
            }
        }
        if index.has_conflicts() {
            return Err(GitError::MergeConflict);
        }
        // commit_step journals the current on-disk index as the before image;
        // the resolved index is not published until that transition is durable.
        commit_step(&repo, &mut journal, &mut index)?;
    } else if journal.phase == "edit" {
        validate_ready(&repo, &journal)?;
        journal.phase = "ready".into();
        journal.save(&repo)?;
    }
    advance(&repo, &mut journal)
}

pub(crate) fn skip(repo: &Repository) -> Result<bool, GitError> {
    let guard = repo.inner.write().unwrap();
    let _lock = journal::lock(&guard)?;
    let (_settings, repo) = journal::durable_repository(&guard)?;
    let mut journal =
        Journal::load(&repo)?.ok_or_else(|| recovery("no native operation to skip"))?;
    journal.validate_identity(&repo)?;
    if journal.kind != Kind::Rebase || !matches!(journal.phase.as_str(), "conflict" | "empty") {
        return Err(recovery("only a conflicting rebase step can be skipped"));
    }
    if Head::read(&repo)? != journal.expected_head {
        return Err(recovery("HEAD changed outside the active operation"));
    }
    let mut current_index = repo.index()?;
    current_index.read(true)?;
    let owned: BTreeSet<_> = journal
        .expected_index
        .0
        .iter()
        .filter(|entry| entry.stage != 0)
        .map(|entry| &entry.path)
        .collect();
    if IndexImage::read(&current_index)
        .0
        .iter()
        .filter(|e| !owned.contains(&e.path))
        .collect::<Vec<_>>()
        != journal
            .expected_index
            .0
            .iter()
            .filter(|e| !owned.contains(&e.path))
            .collect::<Vec<_>>()
    {
        return Err(recovery(
            "unrelated index changes must be preserved before skipping",
        ));
    }
    let mut desired = Index::new()?;
    desired.read_tree(&tree(&repo, journal.tip.as_deref())?)?;
    let mut transition = prepare(&repo, &journal, &mut desired, journal.expected_head.clone())?;
    transition.next_cursor = journal.cursor + 1;
    transition.next_tip = journal.tip.clone();
    journal.begin_transition(&repo, transition)?;
    advance(&repo, &mut journal)
}

pub(crate) fn abort(repo: &Repository) -> Result<(), GitError> {
    let guard = repo.inner.write().unwrap();
    let _lock = journal::lock(&guard)?;
    let (_settings, repo) = journal::durable_repository(&guard)?;
    let mut journal =
        Journal::load(&repo)?.ok_or_else(|| recovery("no native operation to abort"))?;
    journal.validate_identity(&repo)?;
    if journal.phase == "transition" {
        journal.finish_transition(&repo)?;
    }
    if journal.phase == "aborted" {
        return cleanup(&repo, &journal);
    }
    if matches!(journal.phase.as_str(), "publishing" | "completed") {
        return Err(recovery(
            "history was published; finish recovery before using reset to undo it",
        ));
    }
    if Head::read(&repo)? != journal.expected_head {
        return Err(recovery("HEAD changed outside the active operation"));
    }
    let current_index = IndexImage::read(&repo.index()?);
    let owned = |entry: &&journal::Entry| {
        std::str::from_utf8(&entry.path)
            .ok()
            .is_some_and(|path| journal.original_files.contains_key(path))
    };
    if current_index
        .0
        .iter()
        .filter(|e| !owned(e))
        .collect::<Vec<_>>()
        != journal
            .original_index
            .0
            .iter()
            .filter(|e| !owned(e))
            .collect::<Vec<_>>()
    {
        return Err(recovery(
            "unrelated staged changes must be preserved before aborting",
        ));
    }
    if !matches!(
        journal.phase.as_str(),
        "conflict" | "edit" | "empty" | "merge-ready"
    ) {
        validate_ready(&repo, &journal)?;
    }
    let storage = journal::directory(repo.path());
    let workdir = repo.workdir().unwrap();
    let mut files = Vec::new();
    let paths = journal.original_files.keys().map(String::as_str).collect();
    for (path, original) in &journal.original_files {
        files.push(FileChange {
            path: path.clone(),
            before: journal::transition_image(workdir, path, &paths, Some(&storage))?,
            after: original.clone(),
        });
    }
    let transition = Transition {
        before_head: journal.expected_head.clone(),
        after_head: journal.original_head.clone(),
        before_index: current_index,
        after_index: journal.original_index.clone(),
        files,
        next_phase: "aborted".into(),
        next_cursor: journal.cursor,
        next_tip: Some(journal.original_head.oid.clone()),
    };
    journal.begin_transition(&repo, transition)?;
    cleanup(&repo, &journal)
}

pub(crate) fn amend(repo: &Repository, commit_id: &str, message: &str) -> Result<String, GitError> {
    let guard = repo.inner.write().unwrap();
    let _lock = journal::lock(&guard)?;
    let (_settings, repo) = journal::durable_repository(&guard)?;
    let mut journal = Journal::load(&repo)?.ok_or_else(|| recovery("no native edit is active"))?;
    journal.validate_identity(&repo)?;
    if journal.phase != "edit" || journal.tip.as_deref() != Some(commit_id) {
        return Err(recovery(
            "only the paused native edit commit can be amended",
        ));
    }
    if Head::read(&repo)? != journal.expected_head {
        return Err(recovery("HEAD changed outside the active edit"));
    }
    let current = repo.find_commit(oid(commit_id)?)?;
    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Err(GitError::MergeConflict);
    }
    let result_tree = repo.find_tree(index.write_tree()?)?;
    let candidate = current
        .amend(
            None,
            None,
            Some(&signature(&journal)?),
            None,
            Some(message),
            Some(&result_tree),
        )?
        .to_string();
    let mut transition = prepare(
        &repo,
        &journal,
        &mut index,
        Head::detached(candidate.clone()),
    )?;
    let changed = IndexImage::read(&index).paths_changed_from(&journal.expected_index)?;
    for path in changed {
        if !transition.files.iter().any(|file| file.path == path) {
            let image = journal::image(
                &repo.workdir().unwrap().join(&path),
                Some(&journal::directory(repo.path())),
            )?;
            transition.files.push(FileChange {
                path,
                before: image.clone(),
                after: image,
            });
        }
    }
    journal.generated.push(candidate.clone());
    transition.next_phase = "edit".into();
    transition.next_cursor = journal.cursor;
    transition.next_tip = Some(candidate.clone());
    journal.begin_transition(&repo, transition)?;
    Ok(candidate)
}

pub(crate) fn finish_merge(repo: &Repository, message: &str) -> Result<String, GitError> {
    {
        let raw = repo.inner.write().unwrap();
        let _lock = journal::lock(&raw)?;
        let mut operation = Journal::load(&raw)?.ok_or_else(|| recovery("no native merge"))?;
        if operation.kind != Kind::Merge {
            return Err(recovery(
                "finish the active history operation with its recovery controls",
            ));
        }
        operation.message = Some(message.into());
        operation.save(&raw)?;
    }
    if !continue_operation(repo)? {
        return Err(GitError::MergeConflict);
    }
    Ok(repo
        .inner
        .read()
        .unwrap()
        .head()?
        .peel_to_commit()?
        .id()
        .to_string())
}

pub(crate) fn quit_merge(repo: &Repository) -> Result<(), GitError> {
    let guard = repo.inner.write().unwrap();
    let _lock = journal::lock(&guard)?;
    let (_settings, raw) = journal::durable_repository(&guard)?;
    let mut operation = Journal::load(&raw)?.ok_or_else(|| recovery("no native merge"))?;
    operation.validate_identity(&raw)?;
    if operation.kind != Kind::Merge
        || !matches!(operation.phase.as_str(), "conflict" | "merge-ready")
    {
        return Err(recovery("this operation cannot quit merge"));
    }
    if Head::read(&raw)? != operation.expected_head {
        return Err(recovery("HEAD changed outside merge"));
    }
    let index = IndexImage::read(&raw.index()?);
    let transition = Transition {
        before_head: operation.expected_head.clone(),
        after_head: operation.original_head.clone(),
        before_index: index.clone(),
        after_index: index,
        files: Vec::new(),
        next_phase: "aborted".into(),
        next_cursor: operation.cursor,
        next_tip: Some(operation.original_head.oid.clone()),
    };
    operation.begin_transition(&raw, transition)?;
    cleanup(&raw, &operation)
}
