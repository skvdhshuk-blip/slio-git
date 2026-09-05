#![cfg(all(feature = "app-store", target_os = "macos"))]
//! A loopback OpenSSH server runs real git-upload/receive-pack. No user SSH
//! config, authorized_keys, known_hosts, agent or external account is changed.
mod test_helpers;
use git_core::{AuthContext, CloneOptions, Repository};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use test_helpers::TestRepo;

fn run(path: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        git_core::set_auth_context(AuthContext::default());
    }
}

#[test]
fn ssh_clone_fetch_push_validate_server_and_use_only_imported_private_key() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    for key in ["host", "client", "wrong-client"] {
        run(root, "ssh-keygen", &["-t", "ed25519", "-N", "", "-f", key]);
    }
    fs::rename(root.join("client.pub"), root.join("authorized_keys")).unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let user = run(root, "id", &["-un"]);
    let config = root.join("sshd_config");
    fs::write(&config, format!(
        "Port {port}\nListenAddress 127.0.0.1\nHostKey {r}/host\nPidFile {r}/sshd.pid\nAuthorizedKeysFile {r}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {user}\n",
        r = root.display()
    )).unwrap();
    let mut server = Server(
        Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f"])
            .arg(&config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "sshd exited during startup"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "sshd startup timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let remote = root.join("remote.git");
    run(root, "git", &["init", "--bare", "remote.git"]);
    let local = TestRepo::new().unwrap();
    local.add_and_commit("first", "first\n", "first").unwrap();
    let branch = run(local.path(), "git", &["branch", "--show-current"]);
    run(
        &remote,
        "git",
        &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
    );
    let url = format!("ssh://{user}@127.0.0.1:{port}{}", remote.display());
    run(local.path(), "git", &["remote", "add", "origin", &url]);
    let repo = Repository::open(local.path()).unwrap();
    let mut auth = AuthContext {
        imported_ssh_key: Some(root.join("client")),
        ..Default::default()
    };
    git_core::set_auth_context(auth.clone());
    let error = git_core::push(&repo, "origin", &branch, None).unwrap_err();
    assert!(error.to_string().contains("known_hosts"), "{error}");
    let known_hosts = root.join("known_hosts");
    fs::write(
        &known_hosts,
        format!(
            "[127.0.0.1]:{port} {}",
            fs::read_to_string(root.join("host.pub")).unwrap()
        ),
    )
    .unwrap();
    // The common hashed OpenSSH format must work, including a non-default port.
    run(root, "ssh-keygen", &["-H", "-f", "known_hosts"]);
    auth.imported_known_hosts = Some(known_hosts.clone());
    git_core::set_auth_context(auth.clone());
    // Fetch and push URLs can use different ports. Verify the actual push URL.
    run(local.path(), "git", &["remote", "set-url", "--push", "origin", &url]);
    run(local.path(), "git", &["remote", "set-url", "origin", "ssh://127.0.0.1:1/unused.git"]);
    git_core::push(&repo, "origin", &branch, None).unwrap();
    let clone = git_core::clone(
        &CloneOptions {
            url,
            parent_dir: root.into(),
            directory_name: "clone".into(),
            depth: None,
            branch: None,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        run(&clone, "git", &["rev-parse", "HEAD^{tree}"]),
        run(local.path(), "git", &["rev-parse", "HEAD^{tree}"])
    );
    local
        .add_and_commit("second", "second\n", "second")
        .unwrap();
    git_core::push(&repo, "origin", &branch, None).unwrap();
    let clone_repo = Repository::open(&clone).unwrap();
    git_core::fetch(&clone_repo, "origin", None).unwrap();
    let remote_head = run(&remote, "git", &["rev-parse", "HEAD"]);
    assert_eq!(
        run(
            &clone,
            "git",
            &["rev-parse", &format!("refs/remotes/origin/{branch}")]
        ),
        remote_head
    );
    auth.imported_ssh_key = Some(root.join("wrong-client"));
    git_core::set_auth_context(auth.clone());
    assert!(git_core::fetch(&clone_repo, "origin", None).is_err());
    auth.imported_ssh_key = Some(root.join("client"));
    git_core::set_auth_context(auth);
    fs::write(
        &known_hosts,
        format!(
            "[127.0.0.1]:{port} {}",
            fs::read_to_string(root.join("wrong-client.pub")).unwrap()
        ),
    )
    .unwrap();
    let error = git_core::fetch(&clone_repo, "origin", None).unwrap_err();
    assert!(error.to_string().contains("changed host key"), "{error}");
    assert_eq!(run(&remote, "git", &["rev-parse", "HEAD"]), remote_head);
    assert!(!root.join("client.pub").exists());
}
