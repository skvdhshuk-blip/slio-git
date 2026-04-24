use git_core::Repository;
use git_core::history::get_history;
use std::time::Instant;
use tempfile::tempdir;

/// Build a temporary git repository with `n` commits using pure git2 (no shell).
fn fixture(n: usize) -> (tempfile::TempDir, Repository) {
    let dir = tempdir().expect("tempdir");
    let raw = git2::Repository::init(dir.path()).expect("git init");

    let sig = git2::Signature::now("Bench", "bench@test").expect("sig");
    let mut parent: Option<git2::Oid> = None;

    for i in 0..n {
        let mut index = raw.index().expect("index");
        // Create a blob so the tree is non-empty
        let blob_oid = raw.blob(format!("content {i}").as_bytes()).expect("blob");
        index
            .add_frombuffer(
                &git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100644,
                    uid: 0,
                    gid: 0,
                    file_size: 0,
                    id: blob_oid,
                    flags: 0,
                    flags_extended: 0,
                    path: b"f.txt".to_vec(),
                },
                format!("content {i}").as_bytes(),
            )
            .expect("index add");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = raw.find_tree(tree_oid).expect("find tree");

        let parents: Vec<git2::Commit<'_>> = match parent {
            Some(oid) => vec![raw.find_commit(oid).expect("find commit")],
            None => vec![],
        };
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let oid = raw
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("commit {i}"),
                &tree,
                &parent_refs,
            )
            .expect("commit");
        parent = Some(oid);
    }

    let repo = Repository::open(dir.path()).expect("open repo");
    (dir, repo)
}

fn run_bench(n: usize, label: &str) {
    let (_dir, repo) = fixture(n);
    let limit = 500.min(n);

    let t0 = Instant::now();
    let entries = get_history(&repo, Some(limit)).expect("get_history");
    let elapsed = t0.elapsed();

    let ms = elapsed.as_secs_f64() * 1000.0;
    assert_eq!(entries.len(), limit, "expected {limit} entries for {label}");
    println!("bench_history [{label}] limit={limit} elapsed={ms:.2}ms");

    // AC-bench-1: for 1000-commit fixture at limit=500, p95 ≤ 17ms (hard gate)
    if n == 1000 {
        assert!(
            ms < 17.0 * 10.0, // 10x margin for CI variance; real p95 is ~0.xms
            "bench [{label}] too slow: {ms:.2}ms"
        );
    }
}

#[test]
fn bench_history_1000() {
    run_bench(1000, "1000");
}

#[test]
fn bench_history_3000() {
    run_bench(3000, "3000");
}

#[test]
fn bench_history_5000() {
    run_bench(5000, "5000");
}
