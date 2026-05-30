//! Tests for line-level staging / unstaging in git-core.

use git_core::index::{self, LineSide};
use git_core::Repository;
use std::fs;
use tempfile::TempDir;

/// Helper: create a bare repo with user config and an initial commit.
fn setup_repo() -> (Repository, TempDir) {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path()).unwrap();

    for args in [
        ["config", "user.name", "Test"],
        ["config", "user.email", "test@example.com"],
    ] {
        std::process::Command::new("git")
            .args(args)
            .current_dir(temp.path())
            .output()
            .unwrap();
    }

    (repo, temp)
}

/// Commit a file with the given content as the initial state.
fn initial_commit(temp: &TempDir, rel: &str, content: &str) {
    let path = temp.path().join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();

    std::process::Command::new("git")
        .args(["add", rel])
        .current_dir(temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(temp.path())
        .output()
        .unwrap();
}

/// Write content to a file without staging it.
fn write_workdir(temp: &TempDir, rel: &str, content: &str) {
    let path = temp.path().join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
}

/// Return the index contents of a file (as a string) by reading the blob
/// that HEAD points to after applying the patch.
fn read_index_blob(temp: &TempDir, rel: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["show", &format!(":{}", rel)])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "git show :{} failed: {}", rel, String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn stage_two_of_five_added_lines() {
    let (repo, temp) = setup_repo();

    // Initial file: 3 lines
    initial_commit(
        &temp,
        "file.txt",
        "line1\nline2\nline3\n",
    );

    // Workdir: add 5 new lines after line2
    write_workdir(
        &temp,
        "file.txt",
        "line1\nline2\nnew_a\nnew_b\nnew_c\nnew_d\nnew_e\nline3\n",
    );

    // Get hunks (workdir vs index)
    let hunks = index::get_file_hunks(&repo, std::path::Path::new("file.txt")).unwrap();
    assert!(!hunks.is_empty(), "Expected at least one hunk");

    // The hunk should contain the 5 added lines.
    // Find the indices of the '+' lines.
    let hunk = &hunks[0];
    let add_indices: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.origin == '+')
        .map(|(i, _)| i)
        .collect();
    assert!(add_indices.len() >= 5, "Expected at least 5 added lines, got {}", add_indices.len());

    // Stage only the 2nd and 4th added lines (new_b and new_d).
    let to_stage = [add_indices[1], add_indices[3]];
    index::stage_lines(&repo, std::path::Path::new("file.txt"), 0, &to_stage, LineSide::New)
        .expect("stage_lines should succeed");

    // Verify the index now has the partially staged content.
    let index_content = read_index_blob(&temp, "file.txt");
    assert!(
        index_content.contains("new_b"),
        "Index should contain new_b, got:\n{}",
        index_content
    );
    assert!(
        index_content.contains("new_d"),
        "Index should contain new_d, got:\n{}",
        index_content
    );
    assert!(
        !index_content.contains("new_a"),
        "Index should NOT contain new_a, got:\n{}",
        index_content
    );
    assert!(
        !index_content.contains("new_c"),
        "Index should NOT contain new_c, got:\n{}",
        index_content
    );
    assert!(
        !index_content.contains("new_e"),
        "Index should NOT contain new_e, got:\n{}",
        index_content
    );
}

#[test]
fn unstage_specific_lines() {
    let (repo, temp) = setup_repo();

    // Initial file
    initial_commit(&temp, "file.txt", "aaa\nbbb\nccc\n");

    // Modify: add 3 new lines after aaa
    write_workdir(&temp, "file.txt", "aaa\nx1\nx2\nx3\nbbb\nccc\n");

    // Stage the whole file first
    index::stage_file(&repo, std::path::Path::new("file.txt")).unwrap();

    // Verify all 3 lines are staged
    let staged = read_index_blob(&temp, "file.txt");
    assert!(staged.contains("x1") && staged.contains("x2") && staged.contains("x3"));

    // Get index-to-HEAD hunks for unstaging
    let hunks = index::get_index_hunks(&repo, std::path::Path::new("file.txt")).unwrap();
    assert!(!hunks.is_empty());

    let hunk = &hunks[0];
    let add_indices: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.origin == '+')
        .map(|(i, _)| i)
        .collect();
    assert!(add_indices.len() >= 3, "Expected 3 added lines in index hunk");

    // Unstage only x2 (the 2nd added line)
    index::unstage_lines(
        &repo,
        std::path::Path::new("file.txt"),
        0,
        &[add_indices[1]],
        LineSide::New,
    )
    .expect("unstage_lines should succeed");

    // Index should now have x1 and x3, but NOT x2
    let after = read_index_blob(&temp, "file.txt");
    assert!(after.contains("x1"), "Index should still contain x1, got:\n{}", after);
    assert!(after.contains("x3"), "Index should still contain x3, got:\n{}", after);
    assert!(!after.contains("x2"), "Index should NOT contain x2 after unstaging, got:\n{}", after);
}

#[test]
fn partial_staging_preserves_existing_content() {
    let (repo, temp) = setup_repo();

    // Initial file with several lines
    initial_commit(&temp, "f.txt", "A\nB\nC\nD\nE\n");

    // Workdir: insert 2 lines between B and C
    write_workdir(&temp, "f.txt", "A\nB\nX\nY\nC\nD\nE\n");

    let hunks = index::get_file_hunks(&repo, std::path::Path::new("f.txt")).unwrap();
    let hunk = &hunks[0];
    let add_indices: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.origin == '+')
        .map(|(i, _)| i)
        .collect();

    // Stage only the first added line (X)
    index::stage_lines(
        &repo,
        std::path::Path::new("f.txt"),
        0,
        &[add_indices[0]],
        LineSide::New,
    )
    .unwrap();

    let idx = read_index_blob(&temp, "f.txt");
    // Original lines must still be present
    for c in &["A", "B", "C", "D", "E"] {
        assert!(idx.contains(&format!("{}\n", c)), "Missing {} in index:\n{}", c, idx);
    }
    // X is staged, Y is not
    assert!(idx.contains("X\n"), "Index should contain X, got:\n{}", idx);
    assert!(!idx.contains("Y\n"), "Index should NOT contain Y, got:\n{}", idx);
}
