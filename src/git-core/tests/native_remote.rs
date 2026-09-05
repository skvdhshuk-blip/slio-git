#![cfg(all(feature = "app-store", unix))]
//! The receiver is the real Git server, not libgit2's local-file transport.
mod test_helpers;
use git_core::{GitError, PullOptions, PushOptions, Repository};
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use test_helpers::TestRepo;

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
struct Server {
    process: Child,
    dir: TempDir,
    url: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
impl Server {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "--bare", "remote.git"]);
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let process = Command::new("git")
            .args([
                "daemon",
                "--reuseaddr",
                "--export-all",
                "--enable=receive-pack",
                "--listen=127.0.0.1",
                &format!("--port={port}"),
                &format!("--base-path={}", dir.path().display()),
                dir.path().to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        Self {
            process,
            dir,
            url: format!("git://127.0.0.1:{port}/remote.git"),
        }
    }
    fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("remote.git")
    }
    fn hook(&self, name: &str, script: &str) {
        let path = self.path().join("hooks").join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn setup() -> (TestRepo, Server, Repository, String, String) {
    let local = TestRepo::new().unwrap();
    local.add_and_commit("a", "a\n", "first").unwrap();
    let base = git(local.path(), &["rev-parse", "HEAD"]);
    let branch = git(local.path(), &["branch", "--show-current"]);
    let server = Server::new();
    git(local.path(), &["remote", "add", "origin", &server.url]);
    let repo = Repository::open(local.path()).unwrap();
    git_core::push(&repo, "origin", &branch, None).unwrap();
    (local, server, repo, branch, base)
}

#[test]
fn lease_refuses_stale_observation_and_frozen_push_to_here_target() {
    let (local, server, repo, branch, base) = setup();
    local.add_and_commit("b", "b\n", "second").unwrap();
    git(local.path(), &["push", "origin", &branch]);
    let second = git(local.path(), &["rev-parse", "HEAD"]);
    let target = git_core::resolve_push_current_branch_target(&repo, &base).unwrap();
    local.add_and_commit("c", "c\n", "third").unwrap();
    git(local.path(), &["push", "origin", &branch]);
    let newest = git(local.path(), &["rev-parse", "HEAD"]);
    // The local observation changes after confirmation, but the operation uses
    // the OID the user confirmed, not the newly updated tracking reference.
    assert!(matches!(
        git_core::push_current_branch_to_commit(&repo, &target),
        Err(GitError::RemoteRefChanged { .. })
    ));
    assert_eq!(git(&server.path(), &["rev-parse", &branch]), newest);
    git(
        local.path(),
        &[
            "update-ref",
            &format!("refs/remotes/origin/{branch}"),
            &second,
        ],
    );
    git(local.path(), &["reset", "--hard", &base]);
    assert!(matches!(
        git_core::force_push(&repo, "origin", &branch),
        Err(GitError::RemoteRefChanged { .. })
    ));
    assert_eq!(git(&server.path(), &["rev-parse", &branch]), newest);
}

#[test]
fn rejects_server_failure_and_reports_partially_accepted_references() {
    let (local, server, repo, branch, base) = setup();
    local.add_and_commit("b", "b\n", "second").unwrap();
    server.hook(
        "pre-receive",
        "#!/bin/sh\necho 'policy rejection' >&2\nexit 1\n",
    );
    assert!(git_core::push(&repo, "origin", &branch, None).is_err());
    assert_eq!(git(&server.path(), &["rev-parse", &branch]), base);
    fs::remove_file(server.path().join("hooks/pre-receive")).unwrap();
    server.hook(
        "update",
        "#!/bin/sh\ncase \"$1\" in refs/tags/*) echo 'tag rejected' >&2; exit 1;; esac\nexit 0\n",
    );
    git(local.path(), &["tag", "v-test"]);
    let error = git_core::push_with_options(
        &repo,
        "origin",
        &branch,
        PushOptions {
            push_tags: true,
            ..Default::default()
        },
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("refs/tags/v-test"));
    assert_eq!(
        git(&server.path(), &["rev-parse", &branch]),
        git(local.path(), &["rev-parse", "HEAD"])
    );
    assert_eq!(git(&server.path(), &["tag", "--list"]), "");
}

#[test]
fn receiver_detects_remote_change_during_upload_without_retry() {
    let (local, server, repo, branch, base) = setup();
    local.add_and_commit("b", "b\n", "second").unwrap();
    git(local.path(), &["push", "origin", &branch]);
    let second = git(local.path(), &["rev-parse", "HEAD"]);
    local.add_and_commit("c", "c\n", "third").unwrap();
    // pre-receive runs after upload but before compare-and-swap of refs. It
    // simulates another publisher winning between negotiation and ref update.
    server.hook("pre-receive", &format!("#!/bin/sh\nunset GIT_QUARANTINE_PATH\ngit update-ref refs/heads/{branch} {base}\necho run >> hook-runs\n"));
    assert!(git_core::force_push(&repo, "origin", &branch).is_err());
    assert_eq!(git(&server.path(), &["rev-parse", &branch]), base);
    assert_eq!(
        fs::read_to_string(server.path().join("hook-runs"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_ne!(base, second);
}

#[test]
fn fetch_mapping_pull_target_and_all_merge_strategies_are_honored() {
    for strategy in ["ff-only", "no-ff", "squash", "rebase", "autocrlf"] {
        let (local, server, repo, branch, base) = setup();
        local.add_and_commit("new", "new\n", "new remote").unwrap();
        let newest = git(local.path(), &["rev-parse", "HEAD"]);
        git(local.path(), &["push", "origin", "HEAD:refs/heads/release"]);
        git(local.path(), &["reset", "--hard", &base]);
        if strategy == "rebase" {
            local.add_and_commit("local", "local\n", "local").unwrap();
        }
        git_core::fetch(&repo, "origin", None).unwrap();
        assert_eq!(
            git(local.path(), &["rev-parse", "refs/remotes/origin/release"]),
            newest
        );
        git_core::pull_with_options(
            &repo,
            "origin",
            PullOptions {
                branch_name: Some("release"),
                ff_only: strategy == "ff-only",
                no_ff: strategy == "no-ff",
                squash: strategy == "squash",
                rebase: strategy == "rebase",
                force_autocrlf_true: strategy == "autocrlf",
            },
            None,
        )
        .unwrap();
        assert_eq!(
            fs::read(local.path().join("new")).unwrap(),
            if strategy == "autocrlf" {
                b"new\r\n".as_slice()
            } else {
                b"new\n".as_slice()
            }
        );
        match strategy {
            "squash" => {
                assert_eq!(git(local.path(), &["rev-parse", "HEAD"]), base);
                git_core::create_commit(&repo, "squash", "", "").unwrap();
            }
            "no-ff" => assert_eq!(
                git(local.path(), &["log", "-1", "--format=%P"])
                    .split_whitespace()
                    .count(),
                2
            ),
            "rebase" => assert_eq!(git(local.path(), &["rev-parse", "HEAD^"]), newest),
            _ => assert_eq!(git(local.path(), &["rev-parse", "HEAD"]), newest),
        }
        assert_eq!(git(&server.path(), &["rev-parse", &branch]), base);
        assert_eq!(
            git(
                local.path(),
                if strategy == "autocrlf" {
                    &["-c", "core.autocrlf=true", "status", "--porcelain"]
                } else {
                    &["status", "--porcelain"]
                }
            ),
            ""
        );
    }
}
