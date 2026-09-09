//! Interactive replay keeps the original branch intact until the complete plan
//! is published. A checkpoint in the Git directory survives closing the app.
use super::{RebaseTodoEntry, signature_from_locked};
use crate::{GitError, Repository};
use git2::Oid;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize)]
struct Session {
    original: String,
    reference: String,
    tip: Option<String>,
    next: usize,
    phase: String,
    entries: Vec<RebaseTodoEntry>,
}

fn directory(raw: &git2::Repository) -> PathBuf {
    raw.path().join("rebase-merge")
}
pub(super) fn active(repo: &Repository) -> bool {
    repo.path.join("rebase-merge/slio-state.json").exists()
}
fn error(message: &str) -> GitError {
    GitError::OperationFailed {
        operation: "interactive_rebase".into(),
        details: message.into(),
    }
}
fn load(raw: &git2::Repository) -> Result<Session, GitError> {
    serde_json::from_slice(&fs::read(directory(raw).join("slio-state.json"))?)
        .map_err(|e| error(&e.to_string()))
}
fn save(raw: &git2::Repository, session: &Session) -> Result<(), GitError> {
    use std::io::Write;
    let dir = directory(raw);
    let bytes = serde_json::to_vec(session).map_err(|e| error(&e.to_string()))?;
    let mut file = fs::File::create(dir.join("slio-state.tmp"))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(dir.join("slio-state.tmp"), dir.join("slio-state.json"))?;
    let current = session.next + usize::from(session.phase == "conflict");
    fs::write(dir.join("msgnum"), current.to_string())?;
    fs::write(dir.join("end"), session.entries.len().to_string())?;
    let done = session.entries[..current]
        .iter()
        .map(todo_line)
        .collect::<String>();
    let todo = session.entries[current..]
        .iter()
        .map(todo_line)
        .collect::<String>();
    fs::write(dir.join("done"), done)?;
    fs::write(dir.join("git-rebase-todo"), todo)?;
    Ok(())
}
fn todo_line(entry: &RebaseTodoEntry) -> String {
    let action = if entry.action == "reword" {
        "edit"
    } else {
        &entry.action
    };
    format!(
        "{action} {} {}\n",
        entry.commit,
        entry.message.lines().next().unwrap_or_default()
    )
}
fn tip<'a>(
    raw: &'a git2::Repository,
    session: &Session,
) -> Result<Option<git2::Commit<'a>>, GitError> {
    session
        .tip
        .as_deref()
        .map(|id| Ok(raw.find_commit(Oid::from_str(id)?)?))
        .transpose()
}
fn check_head(raw: &git2::Repository, session: &Session) -> Result<(), GitError> {
    if raw.find_reference(&session.reference)?.target() != Some(Oid::from_str(&session.original)?) {
        return Err(error("原分支已被其他操作更新，保留恢复现场"));
    }
    let head = raw.head()?;
    if head.is_branch() {
        return Err(error("当前分支已切换，保留恢复现场"));
    }
    if let Some(expected) = &session.tip {
        let current = head.peel_to_commit()?;
        let previous = raw.find_commit(Oid::from_str(expected)?)?;
        let amended = session.phase == "edit" && current.parent_ids().eq(previous.parent_ids());
        if current.id() != previous.id() && !amended {
            return Err(error("HEAD 已被其他操作更新，保留恢复现场"));
        }
    }
    Ok(())
}

pub(super) fn start(
    repo: &Repository,
    base: Option<&str>,
    entries: &[RebaseTodoEntry],
) -> Result<String, GitError> {
    let raw = repo.inner.write().unwrap();
    let head = raw.head()?;
    let reference = head
        .name()
        .filter(|_| head.is_branch())
        .ok_or_else(|| error("需要当前本地分支"))?
        .to_owned();
    let original = head.peel_to_commit()?.id();
    let base = base
        .map(|id| {
            raw.revparse_single(id)
                .and_then(|obj| obj.peel_to_commit())
                .map(|c| c.id().to_string())
        })
        .transpose()?;
    // Resolve identity before creating a recovery state or touching the worktree.
    signature_from_locked(&raw)?;
    let mut session = Session {
        original: original.to_string(),
        reference,
        tip: base,
        next: 0,
        phase: "ready".into(),
        entries: entries.to_vec(),
    };
    fs::create_dir(directory(&raw))?;
    save(&raw, &session)?;
    raw.set_head_detached(original)?;
    if let Some(base) = &session.tip {
        install(&raw, Oid::from_str(base)?)?;
    }
    run(&raw, &mut session)
}

fn replay_index(
    raw: &git2::Repository,
    source: &git2::Commit<'_>,
    parent: Option<&git2::Commit<'_>>,
) -> Result<git2::Index, GitError> {
    let empty = raw.find_tree(raw.treebuilder(None)?.write()?)?;
    let base = if source.parent_count() == 0 {
        None
    } else {
        Some(source.parent(0)?.tree()?)
    };
    let ours = parent.map(|parent| parent.tree()).transpose()?;
    Ok(raw.merge_trees(
        base.as_ref().unwrap_or(&empty),
        ours.as_ref().unwrap_or(&empty),
        &source.tree()?,
        None,
    )?)
}

fn commit_index(
    raw: &git2::Repository,
    session: &Session,
    mut index: git2::Index,
) -> Result<Oid, GitError> {
    let entry = &session.entries[session.next];
    let source = raw.find_commit(Oid::from_str(&entry.commit)?)?;
    let parent = tip(raw, session)?;
    let combined = matches!(entry.action.as_str(), "fixup" | "squash");
    let parents: Vec<_> = if combined {
        parent
            .as_ref()
            .ok_or_else(|| error("没有可合并的前一提交"))?
            .parents()
            .collect()
    } else {
        parent.into_iter().collect()
    };
    let parents: Vec<_> = parents.iter().collect();
    let previous = if combined { tip(raw, session)? } else { None };
    let message = match entry.action.as_str() {
        "fixup" => previous
            .as_ref()
            .unwrap()
            .message()
            .unwrap_or_default()
            .to_owned(),
        "squash" => format!(
            "{}\n\n{}",
            previous
                .as_ref()
                .unwrap()
                .message()
                .unwrap_or_default()
                .trim_end(),
            source.message().unwrap_or_default().trim()
        ),
        _ => source.message().unwrap_or_default().to_owned(),
    };
    let author = previous.as_ref().unwrap_or(&source).author();
    let tree = raw.find_tree(index.write_tree_to(raw)?)?;
    Ok(raw.commit(
        None,
        &author,
        &signature_from_locked(raw)?,
        &message,
        &tree,
        &parents,
    )?)
}

fn install(raw: &git2::Repository, id: Oid) -> Result<(), GitError> {
    let commit = raw.find_commit(id)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    raw.checkout_tree(commit.as_object(), Some(&mut checkout))?;
    raw.set_head_detached(id)?;
    Ok(())
}

fn run(raw: &git2::Repository, session: &mut Session) -> Result<String, GitError> {
    while session.next < session.entries.len() {
        let entry = &session.entries[session.next];
        if entry.action == "drop" {
            session.next += 1;
            save(raw, session)?;
            continue;
        }
        let source = raw.find_commit(Oid::from_str(&entry.commit)?)?;
        let parent = tip(raw, session)?;
        let index = replay_index(raw, &source, parent.as_ref())?;
        if index.has_conflicts() {
            session.phase = "conflict".into();
            save(raw, session)?;
            // A merge result is an in-memory index. Copy its staged entries into
            // the repository index before writing or checking out conflict files.
            let mut disk = raw.index()?;
            disk.clear()?;
            for entry in index.iter() {
                disk.add(&entry)?;
            }
            disk.write()?;
            let mut checkout = git2::build::CheckoutBuilder::new();
            checkout
                .force()
                .allow_conflicts(true)
                .conflict_style_merge(true);
            raw.checkout_index(Some(&mut disk), Some(&mut checkout))?;
            return Ok("存在冲突，请解决后继续或中止".into());
        }
        let id = commit_index(raw, session, index)?;
        install(raw, id)?;
        session.tip = Some(id.to_string());
        session.next += 1;
        session.phase = if matches!(entry.action.as_str(), "edit" | "reword") {
            "edit"
        } else {
            "ready"
        }
        .into();
        save(raw, session)?;
        if session.phase == "edit" {
            return Ok("已停在目标提交，编辑后继续".into());
        }
    }
    let id = session
        .tip
        .as_ref()
        .ok_or_else(|| error("不能删除整个分支的全部历史"))?;
    // All-drop plans still need to install their base.
    install(raw, Oid::from_str(id)?)?;
    raw.reference_matching(
        &session.reference,
        Oid::from_str(id)?,
        true,
        Oid::from_str(&session.original)?,
        "interactive rebase",
    )?;
    raw.set_head(&session.reference)?;
    fs::remove_dir_all(directory(raw))?;
    Ok("交互式变基已完成".into())
}

pub(super) fn resume(repo: &Repository, skip: bool) -> Result<String, GitError> {
    let raw = repo.inner.write().unwrap();
    let mut session = load(&raw)?;
    check_head(&raw, &session)?;
    if skip {
        let id = session.tip.as_deref().unwrap_or(&session.original);
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force();
        raw.checkout_tree(
            raw.find_commit(Oid::from_str(id)?)?.as_object(),
            Some(&mut checkout),
        )?;
        if session.phase == "conflict" {
            session.next += 1;
        }
    } else if session.phase == "conflict" {
        crate::commit_actions::stage_resolved_conflicts(&raw)?;
        let index = raw.index()?;
        if index.has_conflicts() {
            return Err(GitError::MergeConflict);
        }
        let id = commit_index(&raw, &session, index)?;
        raw.set_head_detached(id)?;
        session.tip = Some(id.to_string());
        let edit = matches!(
            session.entries[session.next].action.as_str(),
            "edit" | "reword"
        );
        session.next += 1;
        if edit {
            session.phase = "edit".into();
            save(&raw, &session)?;
            return Ok("已停在目标提交，编辑后继续".into());
        }
    } else if session.phase == "edit" {
        if raw
            .statuses(None)?
            .iter()
            .any(|entry| entry.status() != git2::Status::WT_NEW)
        {
            return Err(GitError::DirtyWorkingTree);
        }
        session.tip = Some(raw.head()?.peel_to_commit()?.id().to_string());
    }
    session.phase = "ready".into();
    save(&raw, &session)?;
    run(&raw, &mut session)
}

pub(super) fn abort(repo: &Repository) -> Result<(), GitError> {
    let raw = repo.inner.write().unwrap();
    let session = load(&raw)?;
    check_head(&raw, &session)?;
    let original = raw.find_commit(Oid::from_str(&session.original)?)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    raw.checkout_tree(original.as_object(), Some(&mut checkout))?;
    raw.set_head(&session.reference)?;
    fs::remove_dir_all(directory(&raw))?;
    Ok(())
}
