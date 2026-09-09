//! Real smart HTTP Git, including authentication failure and same-path retry.
mod test_helpers;
use git_core::{CloneOptions, clone};
use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn authenticated_http_clone_and_failed_credentials_can_retry() {
    let source = test_helpers::TestRepo::new().unwrap();
    source
        .add_and_commit("hello.txt", "authenticated clone\n", "first")
        .unwrap();
    source
        .add_and_commit("hello.txt", "second\n", "second")
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let port = root.path().join("port");
    let bare = root.path().join("private.git");
    assert!(
        Command::new("git")
            .args(["clone", "--bare"])
            .arg(source.path())
            .arg(&bare)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&bare)
            .args(["config", "http.receivepack", "true"])
            .status()
            .unwrap()
            .success()
    );
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_git.py");
    let mut server = Server(
        Command::new("python3")
            .arg(script)
            .arg(root.path())
            .arg(&port)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !port.exists() {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "HTTP fixture exited before becoming ready; see its stderr above"
        );
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "HTTP fixture failed to start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let options = CloneOptions {
        url: format!(
            "http://127.0.0.1:{}/private.git",
            fs::read_to_string(port).unwrap()
        ),
        parent_dir: root.path().into(),
        directory_name: "client".into(),
        depth: Some(1),
        branch: None,
    };
    assert!(clone::clone(&options, None, None).is_err());
    assert!(!root.path().join("client").exists());
    assert!(clone::clone(&options, None, Some(("e2e", "wrong-token"))).is_err());
    assert!(!root.path().join("client").exists());
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal = cancel.clone();
    let cancelled = clone::clone_cancellable(
        &options,
        Some(Box::new(move |_| {
            signal.store(true, std::sync::atomic::Ordering::Relaxed);
        })),
        Some(("e2e", "disposable-token")),
        cancel,
    );
    assert!(cancelled.is_err());
    assert!(!root.path().join("client").exists());
    let dest = clone::clone(&options, None, Some(("e2e", "disposable-token"))).unwrap();
    assert_eq!(
        fs::read_to_string(dest.join("hello.txt")).unwrap(),
        "second\n"
    );
    let count = Command::new("git")
        .arg("-C")
        .arg(&dest)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap();
    assert!(count.status.success());
    assert_eq!(String::from_utf8(count.stdout).unwrap().trim(), "1");
    let repo = git_core::Repository::discover(&dest).unwrap();
    git_core::create_lightweight_tag(&repo, "http-tag", "HEAD").unwrap();
    git_core::remote::push(
        &repo,
        "origin",
        "refs/tags/http-tag:refs/tags/http-tag",
        Some(("e2e", "disposable-token")),
    )
    .unwrap();
    assert!(
        git2::Repository::open_bare(&bare)
            .unwrap()
            .find_reference("refs/tags/http-tag")
            .is_ok()
    );
    git_core::remote::push(
        &repo,
        "origin",
        ":refs/tags/http-tag",
        Some(("e2e", "disposable-token")),
    )
    .unwrap();
    assert!(
        git2::Repository::open_bare(&bare)
            .unwrap()
            .find_reference("refs/tags/http-tag")
            .is_err()
    );
    source
        .add_and_commit("hello.txt", "third\n", "third")
        .unwrap();
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(source.path())
            .arg("push")
            .arg(&bare)
            .arg("HEAD")
            .output()
            .unwrap()
            .status
            .success()
    );
    // Pull credentials must apply to the fetch URL, not a distinct push URL.
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&dest)
            .args([
                "remote",
                "set-url",
                "--push",
                "origin",
                "http://127.0.0.1:1/unused.git"
            ])
            .status()
            .unwrap()
            .success()
    );
    git_core::remote::pull_with_options(
        &repo,
        "origin",
        git_core::PullOptions {
            ff_only: true,
            ..Default::default()
        },
        Some(("e2e", "disposable-token")),
    )
    .unwrap();
    assert!(git2::Repository::open(&dest).unwrap().is_shallow());
    assert_eq!(
        git_core::history::get_history(&repo, None).unwrap().len(),
        2
    );
    assert_eq!(
        fs::read_to_string(dest.join("hello.txt")).unwrap(),
        "third\n"
    );
    let config = fs::read_to_string(dest.join(".git/config")).unwrap();
    assert!(!config.contains("disposable-token"));
    assert!(!config.contains("helper"));
}
