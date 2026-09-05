//! In-process MAS transport. Authentication is frozen before connecting; no
//! helper, shell command, home-directory key probing or retry is available here.
use super::*;
use git2::{Cred, FetchOptions, Oid, PushOptions as Git2PushOptions};
use std::cell::RefCell;
use std::collections::BTreeMap;

pub(super) fn build_remote_callbacks(
    mut config: Config,
    credentials: Option<(&str, &str)>,
) -> RemoteCallbacks<'static> {
    let username = credentials
        .map(|pair| pair.0.trim().to_string())
        .filter(|name| !name.is_empty());
    let password = credentials
        .map(|pair| pair.1.to_string())
        .filter(|password| !password.is_empty());
    let (identity, access) = crate::auth::network_context();
    let config = config.snapshot();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |url, from_url, allowed| {
        let _access = &access;
        let config = config
            .as_ref()
            .map_err(|error| git2::Error::from_str(&error.to_string()))?;
        let user = resolve_auth_username(config, url, username.as_deref(), from_url);
        if allowed.is_user_pass_plaintext() {
            if let (Some(user), Some(password)) = (user.as_deref(), password.as_deref()) {
                return Cred::userpass_plaintext(user, password);
            }
        }
        if allowed.is_username() {
            if let Some(user) = user.as_deref() {
                return Cred::username(user);
            }
        }
        if allowed.is_ssh_key() {
            if let (Some(user), Some(key)) = (user.as_deref(), identity.imported_ssh_key.as_deref())
            {
                return Cred::ssh_key(user, identity.imported_ssh_pub.as_deref(), key, None);
            }
        }
        Err(git2::Error::from_str(
            "请提供账号凭据或重新授权 SSH 私钥；商店版不调用外部凭据助手",
        ))
    });
    callbacks
}

pub(super) fn fetch(
    repo: &Repository,
    remote_name: &str,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    let raw = repo.inner.write().unwrap();
    let mut remote = raw.find_remote(remote_name)?;
    let mut options = FetchOptions::new();
    options.remote_callbacks(build_remote_callbacks(raw.config()?, credentials));
    // Empty refspecs use this remote's configured mapping, including its name.
    remote.fetch::<&str>(&[], Some(&mut options), None)?;
    Ok(())
}

fn expected_remote(
    raw: &git2::Repository,
    remote: &git2::Remote<'_>,
    destination: &str,
) -> Result<Oid, GitError> {
    // Resolve through the actual fetch mapping, not a guessed remote name.
    for refspec in remote
        .refspecs()
        .filter(|spec| spec.direction() == git2::Direction::Fetch)
    {
        if refspec.src_matches(destination) {
            let local = refspec.transform(destination)?;
            let name = local.as_str().ok_or_else(|| GitError::InvalidInput {
                message: "non-UTF8 remote reference".into(),
            })?;
            return match raw.refname_to_id(name) {
                Ok(id) => Ok(id),
                Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(Oid::zero()),
                Err(error) => Err(error.into()),
            };
        }
    }
    // No observation means only creating an absent reference is safe.
    Ok(Oid::zero())
}

fn push_refspecs(
    raw: &git2::Repository,
    remote_name: &str,
    refspecs: &[String],
    force: bool,
    frozen: Option<(&str, Oid)>,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    let mut remote = raw.find_remote(remote_name)?;
    let mut expected = BTreeMap::new();
    if force {
        for spec in refspecs {
            let (_, destination) =
                spec.trim_start_matches('+')
                    .split_once(':')
                    .ok_or_else(|| GitError::InvalidInput {
                        message: "explicit source and destination required".into(),
                    })?;
            let id = if let Some((reference, id)) = frozen.filter(|pair| pair.0 == destination) {
                let _ = reference;
                id
            } else {
                expected_remote(raw, &remote, destination)?
            };
            expected.insert(destination.to_string(), id);
        }
    }
    let lease_error = RefCell::new(None);
    let outcomes = RefCell::new(BTreeMap::new());
    let negotiated = RefCell::new(std::collections::BTreeSet::new());
    let mut callbacks = build_remote_callbacks(raw.config()?, credentials);
    callbacks.push_negotiation(|updates| {
        for update in updates {
            let reference = update
                .dst_refname()
                .ok_or_else(|| git2::Error::from_str("invalid destination reference"))?;
            if update.src() == update.dst() {
                continue;
            }
            negotiated.borrow_mut().insert(reference.to_owned());
            if let Some(expected) = expected.get(reference) {
                if update.src() != *expected {
                    *lease_error.borrow_mut() = Some(GitError::RemoteRefChanged {
                        reference: reference.into(),
                        expected: expected.to_string(),
                        actual: update.src().to_string(),
                    });
                    return Err(git2::Error::from_str(
                        "remote changed; force-with-lease refused before upload",
                    ));
                }
            }
        }
        Ok(())
    });
    callbacks.push_update_reference(|reference, status| {
        outcomes
            .borrow_mut()
            .insert(reference.to_string(), status.map(str::to_owned));
        Ok(())
    });
    let mut options = Git2PushOptions::new();
    options.remote_callbacks(callbacks);
    let result = remote.push(refspecs, Some(&mut options));
    drop(options);
    if let Some(error) = lease_error.into_inner() {
        return Err(error);
    }
    let results = outcomes.into_inner();
    let rejected: Vec<_> = results
        .iter()
        .filter_map(|(reference, status)| {
            status
                .as_ref()
                .map(|message| format!("{reference}: {message}"))
        })
        .collect();
    if !rejected.is_empty() {
        return Err(GitError::RemoteFailed {
            remote: remote_name.into(),
            details: format!("部分引用可能已上传；服务端拒绝：{}", rejected.join("; ")),
        });
    }
    result.map_err(|error| GitError::RemoteFailed {
        remote: remote_name.into(),
        details: error.to_string(),
    })?;
    if negotiated
        .into_inner()
        .iter()
        .any(|reference| !results.contains_key(reference))
    {
        return Err(GitError::RemoteFailed {
            remote: remote_name.into(),
            details: "上传结果不完整；请刷新远端状态后确认，不能自动重试".into(),
        });
    }
    Ok(())
}

pub(super) fn push_with_options(
    repo: &Repository,
    remote_name: &str,
    branch_name: &str,
    options: PushOptions<'_>,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    let target = normalize_target_branch(branch_name, options);
    let should_set = options.set_upstream || should_auto_set_upstream(repo, branch_name, target);
    let mut refspec = build_push_refspec(branch_name, target);
    if !refspec.contains(':') {
        refspec = format!("{refspec}:{refspec}");
    }
    if refspec.starts_with('+') && !options.force_with_lease {
        return Err(GitError::InvalidInput {
            message: "显式强推必须使用 force-with-lease".into(),
        });
    }
    // A source is resolved once; subsequent local branch changes cannot silently
    // change the object this operation publishes.
    let (source, destination) = refspec.trim_start_matches('+').split_once(':').unwrap();
    if !git2::Reference::is_valid_name(destination) || destination.contains('*') {
        return Err(GitError::InvalidInput {
            message: "invalid destination reference".into(),
        });
    }
    {
        let raw = repo.inner.write().unwrap();
        let source = if source.is_empty() {
            String::new()
        } else {
            raw.revparse_single(source)?.id().to_string()
        };
        let prefix = if options.force_with_lease { "+" } else { "" };
        let mut specs = vec![format!("{prefix}{source}:{destination}")];
        if options.push_tags {
            for name in raw.tag_names(None)?.iter().flatten() {
                let reference = format!("refs/tags/{name}");
                if reference != destination {
                    let id = raw.refname_to_id(&reference)?;
                    specs.push(format!("{id}:{reference}"));
                }
            }
        }
        push_refspecs(
            &raw,
            remote_name,
            &specs,
            options.force_with_lease,
            None,
            credentials,
        )?;
    }
    if should_set && !is_explicit_refspec(branch_name) {
        set_branch_upstream(repo, branch_name, remote_name, target)?;
    }
    Ok(())
}

pub(crate) fn push_selected_commit(
    repo: &Repository,
    target: &crate::commit_actions::PushCurrentBranchTarget,
) -> Result<(), GitError> {
    let raw = repo.inner.write().unwrap();
    let reference = format!("refs/heads/{}", target.upstream_branch_name);
    let prefix = if target.requires_force_with_lease {
        "+"
    } else {
        ""
    };
    let spec = format!("{prefix}{}:{reference}", target.selected_commit);
    push_refspecs(
        &raw,
        &target.remote_name,
        &[spec],
        target.requires_force_with_lease,
        Some((&reference, Oid::from_str(&target.expected_remote_oid)?)),
        None,
    )
}

pub(super) fn pull_with_options(
    repo: &Repository,
    remote_name: &str,
    options: PullOptions<'_>,
    credentials: Option<(&str, &str)>,
) -> Result<(), GitError> {
    if (options.ff_only && options.no_ff)
        || (options.rebase && options.squash)
        || (options.squash && options.no_ff)
        || (options.rebase && options.no_ff)
    {
        return Err(GitError::InvalidInput {
            message: "拉取选项冲突，请选择一种合并策略".into(),
        });
    }
    let branch = options
        .branch_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| configured_upstream_branch(repo, remote_name))
        .unwrap_or(current_branch(repo, "pull")?);
    let reference = if branch.starts_with("refs/heads/") {
        branch.clone()
    } else {
        format!("refs/heads/{branch}")
    };
    if !git2::Reference::is_valid_name(&reference) {
        return Err(GitError::InvalidInput {
            message: "invalid pull branch".into(),
        });
    }
    let fetched = {
        let raw = repo.inner.write().unwrap();
        let mut remote = raw.find_remote(remote_name)?;
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(build_remote_callbacks(raw.config()?, credentials));
        remote.fetch(&[&reference], Some(&mut fetch_options), None)?;
        // FETCH_HEAD contains only this explicitly requested branch.
        raw.find_reference("FETCH_HEAD")?.peel_to_commit()?.id()
    };
    let fast_forward = {
        let raw = repo.inner.read().unwrap();
        let head = raw.head()?.peel_to_commit()?.id();
        if head == fetched || raw.graph_descendant_of(head, fetched)? {
            return Ok(());
        }
        raw.graph_descendant_of(fetched, head)?
    };
    if options.ff_only && !fast_forward {
        return Err(GitError::OperationFailed {
            operation: "pull".into(),
            details: "无法快进；本地与远端历史已经分叉".into(),
        });
    }
    if options.rebase {
        crate::rebase::start_onto(repo, &fetched.to_string(), options.force_autocrlf_true)?;
        return Ok(());
    }
    crate::native::merge::start(
        repo,
        &fetched.to_string(),
        options.no_ff,
        options.squash,
        options.force_autocrlf_true,
    )
}
