use git_core::{ConflictResolution, Repository, index};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(p: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(p)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
fn fixture(name: &str, initial: Option<&[u8]>) -> (tempfile::TempDir, PathBuf, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let channel = if cfg!(feature = "app-store") {
        "mas"
    } else {
        "desktop"
    };
    let p = root.join(format!("{channel}-{name}"));
    fs::create_dir_all(&p).unwrap();
    git(&p, &["init", "-b", "main"]);
    git(&p, &["config", "user.name", "Review Tester"]);
    git(&p, &["config", "user.email", "review@example.invalid"]);
    git(&p, &["config", "commit.gpgsign", "false"]);
    if let Some(bytes) = initial {
        fs::write(p.join("file.txt"), bytes).unwrap();
        git(&p, &["add", "file.txt"]);
        git(&p, &["commit", "-m", "base"]);
    }
    let repo = Repository::discover(&p).unwrap();
    (dir, p, repo)
}

#[test]
fn review_stage_deleted_file_records_deletion() {
    let (_dir, p, repo) = fixture("stage-deleted", Some(b"base\n"));
    fs::remove_file(p.join("file.txt")).unwrap();
    let result = index::stage_file(&repo, Path::new("file.txt"));
    println!(
        "stage_file result={result:?}; status={}",
        git(&p, &["status", "--porcelain"])
    );
    assert!(
        result.is_ok(),
        "single-file staging must support a tracked deletion"
    );
    assert_eq!(
        git(&p, &["diff", "--cached", "--name-status"]),
        "D\tfile.txt"
    );
}
#[test]
fn review_unstage_first_commit_file() {
    let (_dir, p, repo) = fixture("unborn-unstage", None);
    fs::write(p.join("file.txt"), b"first\n").unwrap();
    index::stage_file(&repo, Path::new("file.txt")).unwrap();
    let result = index::unstage_file(&repo, Path::new("file.txt"));
    println!(
        "unstage_file result={result:?}; status={}",
        git(&p, &["status", "--porcelain"])
    );
    assert!(result.is_ok(), "unstage must work before the first commit");
    assert_eq!(git(&p, &["ls-files"]), "");
    assert!(p.join("file.txt").exists());
}

#[test]
fn unstage_all_and_hunks_before_first_commit_preserve_worktree() {
    let (_dir, p, repo) = fixture("unborn-all", None);
    fs::write(p.join("file.txt"), b"first\n").unwrap();
    index::stage_file(&repo, Path::new("file.txt")).unwrap();
    index::unstage_hunk(&repo, Path::new("file.txt"), 0).unwrap();
    assert_eq!(git(&p, &["ls-files"]), "");
    assert_eq!(fs::read(p.join("file.txt")).unwrap(), b"first\n");
    index::stage_file(&repo, Path::new("file.txt")).unwrap();
    index::unstage_all(&repo).unwrap();
    assert_eq!(git(&p, &["ls-files"]), "");
    assert!(p.join("file.txt").exists());
}

#[test]
fn unstage_literal_path_does_not_match_another_filename() {
    let (_dir, p, repo) = fixture("literal-path", Some(b"base\n"));
    for name in ["file[1].txt", "file1.txt"] {
        fs::write(p.join(name), b"old\n").unwrap();
    }
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-m", "literal paths"]);
    for name in ["file[1].txt", "file1.txt"] {
        fs::write(p.join(name), b"new\n").unwrap();
        index::stage_file(&repo, Path::new(name)).unwrap();
    }
    index::unstage_file(&repo, Path::new("file[1].txt")).unwrap();
    assert_eq!(git(&p, &["show", ":file[1].txt"]), "old");
    assert_eq!(git(&p, &["show", ":file1.txt"]), "new");
    let path = Path::new("file[1].txt");
    assert_eq!(git_core::diff::diff_file_to_index(&repo, path).unwrap().files.len(), 1);
    index::stage_hunk(&repo, path, 0).unwrap();
    assert_eq!(git(&p, &["show", ":file[1].txt"]), "new");
    index::unstage_hunk(&repo, path, 0).unwrap();
    assert_eq!(git(&p, &["show", ":file[1].txt"]), "old");
    assert_eq!(git(&p, &["show", ":file1.txt"]), "new");
}

#[test]
fn discard_already_deleted_new_index_entry_preserves_other_staged_files() {
    let (_dir, p, repo) = fixture("discard-absent", Some(b"base\n"));
    fs::write(p.join("new.txt"), b"new\n").unwrap();
    fs::write(p.join("file.txt"), b"keep staged\n").unwrap();
    git(&p, &["add", "-A"]);
    fs::remove_file(p.join("new.txt")).unwrap();
    index::discard_file(&repo, Path::new("new.txt")).unwrap();
    assert_eq!(git(&p, &["ls-files", "--", "new.txt"]), "");
    assert_eq!(git(&p, &["show", ":file.txt"]), "keep staged");
}

#[test]
fn unstage_one_of_several_hunks_preserves_unstaged_edits_and_other_hunks() {
    let base = (0..40).map(|n| format!("line {n}\n")).collect::<String>();
    let (_dir, p, repo) = fixture("several-hunks", Some(base.as_bytes()));
    let staged = base
        .replace("line 2\n", "first change\nextra line\n")
        .replace("line 30\n", "second change\n");
    fs::write(p.join("file.txt"), &staged).unwrap();
    index::stage_file(&repo, Path::new("file.txt")).unwrap();
    let working = staged.replace("line 17\n", "unstaged change\n");
    fs::write(p.join("file.txt"), &working).unwrap();
    index::unstage_hunk(&repo, Path::new("file.txt"), 0).unwrap();
    assert_eq!(
        git(&p, &["show", ":file.txt"]),
        base.replace("line 30\n", "second change\n").trim()
    );
    assert_eq!(fs::read_to_string(p.join("file.txt")).unwrap(), working);
}

#[test]
fn selected_lines_preserve_missing_final_newline_in_both_directions() {
    let (_dir, p, repo) = fixture("lines-eof", Some(b"base\n"));
    fs::write(p.join("file.txt"), b"base\nlast").unwrap();
    let hunks = index::get_file_hunks(&repo, Path::new("file.txt")).unwrap();
    let selected = hunks[0]
        .lines
        .iter()
        .position(|line| line.origin == '+')
        .unwrap();
    index::stage_lines(
        &repo,
        Path::new("file.txt"),
        0,
        &[selected],
        index::LineSide::New,
    )
    .unwrap();
    let raw = git2::Repository::open(&p).unwrap();
    let oid = raw
        .index()
        .unwrap()
        .get_path(Path::new("file.txt"), 0)
        .unwrap()
        .id;
    assert_eq!(raw.find_blob(oid).unwrap().content(), b"base\nlast");
    index::unstage_lines(
        &repo,
        Path::new("file.txt"),
        0,
        &[selected],
        index::LineSide::New,
    )
    .unwrap();
    assert_eq!(git(&p, &["diff", "--cached"]), "");
    assert_eq!(fs::read(p.join("file.txt")).unwrap(), b"base\nlast");
}

#[cfg(unix)]
#[test]
fn resolving_executable_conflict_preserves_mode_bytes_and_unrelated_changes() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, p, _) = fixture("executable-conflict", Some(b"base\n"));
    git(&p, &["checkout", "-b", "feature"]);
    fs::write(p.join("file.txt"), b"#!/bin/sh\necho theirs\n").unwrap();
    fs::set_permissions(p.join("file.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    git(&p, &["add", "file.txt"]);
    git(&p, &["commit", "-m", "theirs"]);
    git(&p, &["checkout", "main"]);
    fs::write(p.join("file.txt"), b"ours\n").unwrap();
    git(&p, &["add", "file.txt"]);
    git(&p, &["commit", "-m", "ours"]);
    let repo = Repository::discover(&p).unwrap();
    assert!(repo.merge_branch("feature").is_err());
    fs::write(p.join("unrelated.txt"), b"keep me\n").unwrap();
    git_core::resolve_conflict(&repo, Path::new("file.txt"), ConflictResolution::Theirs).unwrap();
    assert_eq!(
        fs::read(p.join("file.txt")).unwrap(),
        b"#!/bin/sh\necho theirs\n"
    );
    assert_eq!(
        fs::metadata(p.join("file.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(git(&p, &["ls-files", "--stage", "--", "file.txt"]).starts_with("100755 "));
    assert_eq!(fs::read(p.join("unrelated.txt")).unwrap(), b"keep me\n");
    assert!(
        git_core::resolve_conflict(&repo, Path::new("file.txt"), ConflictResolution::Ours).is_err()
    );
    assert_eq!(
        fs::read(p.join("file.txt")).unwrap(),
        b"#!/bin/sh\necho theirs\n"
    );
}
#[test]
fn review_discard_new_staged_file_removes_index_entry() {
    let (_dir, p, repo) = fixture("discard-new-staged", Some(b"base\n"));
    fs::write(p.join("new.txt"), b"discard this\n").unwrap();
    index::stage_file(&repo, Path::new("new.txt")).unwrap();
    index::discard_file(&repo, Path::new("new.txt")).unwrap();
    println!("after success: {}", git(&p, &["status", "--porcelain"]));
    assert_eq!(
        git(&p, &["ls-files", "--", "new.txt"]),
        "",
        "discard must remove both index and worktree version"
    );
}
#[test]
fn review_amend_includes_staged_tree() {
    let (_dir, p, repo) = fixture("amend-staged", Some(b"base\n"));
    fs::write(p.join("file.txt"), b"amended content\n").unwrap();
    index::stage_file(&repo, Path::new("file.txt")).unwrap();
    let head = git(&p, &["rev-parse", "HEAD"]);
    let new = git_core::amend_commit(&repo, &head, "amended message").unwrap();
    println!(
        "amend={new}; HEAD file={}; status={}",
        git(&p, &["show", "HEAD:file.txt"]),
        git(&p, &["status", "--porcelain"])
    );
    assert_eq!(git(&p, &["show", "HEAD:file.txt"]), "amended content");
    assert_eq!(git(&p, &["status", "--porcelain"]), "");
}
#[test]
fn review_amend_preserves_original_author() {
    let (_dir, p, repo) = fixture("amend-author", Some(b"base\n"));
    git(
        &p,
        &[
            "-c",
            "user.name=Original Author",
            "-c",
            "user.email=original@example.invalid",
            "commit",
            "--amend",
            "--reset-author",
            "--no-edit",
        ],
    );
    let original = git(&p, &["show", "-s", "--format=%an <%ae>%n%aI", "HEAD"]);
    let head = git(&p, &["rev-parse", "HEAD"]);
    git_core::amend_commit(&repo, &head, "edited message").unwrap();
    let after = git(&p, &["show", "-s", "--format=%an <%ae>%n%aI", "HEAD"]);
    println!("original={original}; amended={after}");
    assert_eq!(
        after, original,
        "amend without reset-author preserves authorship"
    );
}
#[test]
fn review_unstage_hunk_preserves_worktree() {
    let (_dir, p, repo) = fixture("unstage-hunk", Some(b"one\ntwo\nthree\n"));
    let changed = b"one\nchanged\nthree\n";
    fs::write(p.join("file.txt"), changed).unwrap();
    index::stage_file(&repo, Path::new("file.txt")).unwrap();
    let result = index::unstage_hunk(&repo, Path::new("file.txt"), 0);
    println!(
        "unstage_hunk={result:?}; worktree={:?}; status={}",
        fs::read_to_string(p.join("file.txt")).unwrap(),
        git(&p, &["status", "--porcelain"])
    );
    assert!(result.is_ok(), "unstage a normal text hunk should succeed");
    assert_eq!(
        fs::read(p.join("file.txt")).unwrap(),
        changed,
        "unstage must never discard working content"
    );
    assert_eq!(git(&p, &["diff", "--cached"]), "");
}
#[test]
fn review_stage_hunk_without_final_newline() {
    let (_dir, p, repo) = fixture("no-final-newline", Some(b"old"));
    fs::write(p.join("file.txt"), b"new").unwrap();
    let result = index::stage_hunk(&repo, Path::new("file.txt"), 0);
    println!("no-newline stage={result:?}");
    assert!(result.is_ok());
    assert_eq!(git(&p, &["show", ":file.txt"]), "new");
}
fn conflict(
    name: &str,
    base: &[u8],
    ours: Option<&[u8]>,
    theirs: &[u8],
) -> (tempfile::TempDir, PathBuf, Repository) {
    let (dir, p, _) = fixture(name, Some(base));
    git(&p, &["checkout", "-b", "feature"]);
    fs::write(p.join("file.txt"), theirs).unwrap();
    git(&p, &["add", "file.txt"]);
    git(&p, &["commit", "-m", "theirs"]);
    git(&p, &["checkout", "main"]);
    if let Some(ours) = ours {
        fs::write(p.join("file.txt"), ours).unwrap();
    } else {
        fs::remove_file(p.join("file.txt")).unwrap();
    }
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-m", "ours"]);
    let repo = Repository::discover(&p).unwrap();
    let result = repo.merge_branch("feature");
    println!("merge={result:?}");
    assert!(result.is_err());
    (dir, p, repo)
}
#[test]
fn review_binary_conflict_choice_preserves_exact_bytes() {
    let ours = b"\0\xff\xfeOURS\x80";
    let (_dir, p, repo) = conflict("binary-conflict", b"\0BASE", Some(ours), b"\0\xffTHEIRS");
    git_core::resolve_conflict(&repo, Path::new("file.txt"), ConflictResolution::Ours).unwrap();
    let after = fs::read(p.join("file.txt")).unwrap();
    println!("expected={ours:?}; actual={after:?}");
    assert_eq!(after, ours, "accept ours must preserve binary bytes");
}
#[test]
fn review_accept_deleted_side_resolves_modify_delete_conflict() {
    let (_dir, p, repo) = conflict("modify-delete", b"base\n", None, b"their change\n");
    let result = git_core::resolve_conflict(&repo, Path::new("file.txt"), ConflictResolution::Ours);
    println!(
        "choose deletion={result:?}; status={}",
        git(&p, &["status", "--porcelain"])
    );
    assert!(
        result.is_ok(),
        "accept deleted side should remove file and stages"
    );
    assert!(!p.join("file.txt").exists());
    assert!(!index::has_conflicts(&repo));
}

#[test]
fn review_fetch_uses_remote_namespace() {
    let (_dir, p, repo) = fixture("fetch-namespace", Some(b"base\n"));
    let bare = p.parent().unwrap().join(if cfg!(feature = "app-store") {
        "mas-fetch.git"
    } else {
        "desktop-fetch.git"
    });
    git(
        &p,
        &[
            "clone",
            "--bare",
            p.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    git(&p, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git_core::fetch(&repo, "origin", None).unwrap();
    let actual = git(&p, &["for-each-ref", "--format=%(refname)", "refs/remotes"]);
    println!("fetch refs={actual}");
    assert_eq!(actual, "refs/remotes/origin/main");
}

#[test]
fn review_reject_checkout_branch_owned_by_other_worktree() {
    let (_dir, p, repo) = fixture("worktree-checkout", Some(b"base\n"));
    let linked = p.parent().unwrap().join(if cfg!(feature = "app-store") {
        "mas-linked"
    } else {
        "desktop-linked"
    });
    git(
        &p,
        &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
    );
    let result = git_core::checkout_ref(&repo, "feature");
    println!(
        "checkout occupied branch={result:?}; main checkout={}",
        git(&p, &["branch", "--show-current"])
    );
    assert!(
        result.is_err(),
        "a branch checked out in another worktree cannot be checked out here"
    );
}

#[cfg(unix)]
#[test]
fn review_push_reports_server_rejection() {
    use std::{
        net::{TcpListener, TcpStream},
        os::unix::fs::PermissionsExt,
        process::{Child, Stdio},
        time::{Duration, Instant},
    };
    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (_dir, p, repo) = fixture("push-rejection", Some(b"base\n"));
    let bare = p.join("server.git");
    git(&p, &["init", "--bare", bare.to_str().unwrap()]);
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let daemon = Path::new(&git(&p, &["--exec-path"])).join("git-daemon");
    let _server = Server(
        Command::new(daemon)
            .args([
                "--reuseaddr",
                "--export-all",
                "--enable=receive-pack",
                "--listen=127.0.0.1",
                &format!("--port={port}"),
                &format!("--base-path={}", p.display()),
                p.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    git(
        &p,
        &[
            "remote",
            "add",
            "origin",
            &format!("git://127.0.0.1:{port}/server.git"),
        ],
    );
    git(&p, &["push", "-u", "origin", "main"]);
    fs::write(p.join("file.txt"), b"next\n").unwrap();
    git(&p, &["add", "file.txt"]);
    git(&p, &["commit", "-m", "next"]);
    let old = git(&bare, &["rev-parse", "refs/heads/main"]);
    let hook = bare.join("hooks/update");
    fs::write(&hook, "#!/bin/sh\necho review-policy-reject >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let result = git_core::push(&repo, "origin", "main", None);
    let after = git(&bare, &["rev-parse", "refs/heads/main"]);
    println!(
        "push={result:?}; remote_before={old}; remote_after={after}; local={}",
        git(&p, &["rev-parse", "HEAD"])
    );
    assert_eq!(old, after);
    assert!(
        result.is_err(),
        "server rejection must not be reported as success"
    );
}

#[cfg(unix)]
#[test]
fn review_symlink_conflict_does_not_overwrite_target_file() {
    use std::os::unix::fs::symlink;
    let (_dir, p, _) = fixture("symlink-conflict", Some(b"base\n"));
    for target in ["target-a.txt", "target-b.txt", "target-c.txt"] {
        fs::write(p.join(target), format!("valuable {target}\n")).unwrap();
    }
    fs::remove_file(p.join("file.txt")).unwrap();
    symlink("target-a.txt", p.join("file.txt")).unwrap();
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-m", "base symlink"]);
    git(&p, &["checkout", "-b", "feature"]);
    fs::remove_file(p.join("file.txt")).unwrap();
    symlink("target-b.txt", p.join("file.txt")).unwrap();
    git(&p, &["add", "file.txt"]);
    git(&p, &["commit", "-m", "theirs symlink"]);
    git(&p, &["checkout", "main"]);
    fs::remove_file(p.join("file.txt")).unwrap();
    symlink("target-c.txt", p.join("file.txt")).unwrap();
    git(&p, &["add", "file.txt"]);
    git(&p, &["commit", "-m", "ours symlink"]);
    let repo = Repository::discover(&p).unwrap();
    assert!(repo.merge_branch("feature").is_err());
    let before = fs::read(p.join("target-c.txt")).unwrap();
    let result =
        git_core::resolve_conflict(&repo, Path::new("file.txt"), ConflictResolution::Theirs);
    println!(
        "resolve symlink={result:?}; link={:?}; target={:?}",
        fs::read_link(p.join("file.txt")),
        fs::read_to_string(p.join("target-c.txt"))
    );
    assert_eq!(
        fs::read(p.join("target-c.txt")).unwrap(),
        before,
        "choosing a symlink target must never overwrite the pointed-to file"
    );
    assert!(result.is_ok());
    assert_eq!(
        fs::read_link(p.join("file.txt")).unwrap(),
        PathBuf::from("target-b.txt")
    );
}

#[test]
fn external_staging_survives_selected_file_operations() {
    for unstage in [false, true] {
        let (_dir, p, repo) = fixture("external-index", Some(b"base\n"));
        fs::write(p.join("file.txt"), b"selected\n").unwrap();
        index::stage_file(&repo, Path::new("file.txt")).unwrap();
        fs::write(p.join("external.txt"), b"external\n").unwrap();
        git(&p, &["add", "external.txt"]);
        if unstage {
            index::unstage_file(&repo, Path::new("file.txt")).unwrap();
        } else {
            fs::write(p.join("file.txt"), b"selected again\n").unwrap();
            index::stage_file(&repo, Path::new("file.txt")).unwrap();
        }
        assert_eq!(git(&p, &["show", ":external.txt"]), "external");
    }
}

#[test]
fn commit_reads_staging_written_by_another_repository_handle() {
    let (_dir, p, repo) = fixture("external-commit", Some(b"base\n"));
    let _cached = index::get_index(&repo).unwrap();
    fs::write(p.join("file.txt"), b"external commit\n").unwrap();
    git(&p, &["add", "file.txt"]);
    git_core::commit::create_commit(&repo, "external staged", "", "").unwrap();
    assert_eq!(git(&p, &["show", "HEAD:file.txt"]), "external commit");
    assert_eq!(git(&p, &["status", "--porcelain"]), "");
}

#[test]
fn failed_clone_can_retry_the_same_destination() {
    let dir = tempfile::tempdir().unwrap();
    let opts = git_core::clone::CloneOptions {
        url: dir.path().join("missing-source").display().to_string(),
        parent_dir: dir.path().to_path_buf(),
        directory_name: "retry".into(),
        depth: None,
        branch: None,
    };
    assert!(git_core::clone::clone(&opts, None, None).is_err());
    assert!(
        !dir.path().join("retry").exists(),
        "failed clone must not leave a destination that blocks retry"
    );
}
