//! Snapshot tests for auto_merge_conflict non-conflicting hunk merging.
//!
//! Tests verify that auto_merge_conflict correctly merges three-way diffs
//! where ours and theirs modify independent parts of the same content.
//! Uses realistic conflict scenarios where both sides made changes.

mod test_helpers;

use git_core::diff::{ThreeWayDiff, auto_merge_conflict};
use std::path::Path;
use std::process::Command;
use test_helpers::TestRepo;

fn run_git(repo_path: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("git command failed")
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_diff(path: &str, base: &str, ours: &str, theirs: &str) -> ThreeWayDiff {
    ThreeWayDiff {
        path: path.to_string(),
        hunks: Vec::new(),
        has_conflicts: true,
        base_content: base.to_string(),
        ours_content: ours.to_string(),
        theirs_content: theirs.to_string(),
    }
}

// ── Case 1: single file, multiple non-conflicting hunks ──────────────────────

/// ours inserts a line after alpha; theirs appends a line at end.
/// Both insertions are in non-overlapping positions → auto-merge succeeds.
#[test]
fn auto_merge_single_file_independent_insertions_both_sides() {
    let base = "alpha\nbravo\ncharlie\n";
    let ours = "alpha\nours_insert\nbravo\ncharlie\n";
    let theirs = "alpha\nbravo\ncharlie\ntheirs_append\n";

    let diff = make_diff("file.txt", base, ours, theirs);
    let result = auto_merge_conflict(&diff);

    assert!(
        !result.has_conflicts,
        "independent insertions at different positions must auto-merge; content={:?}",
        result.content
    );
    assert_eq!(result.remaining_conflicts, 0);
    assert!(
        result.content.contains("ours_insert"),
        "ours insertion must appear in merged output"
    );
    assert!(
        result.content.contains("theirs_append"),
        "theirs insertion must appear in merged output"
    );

    // Exact snapshot
    let expected = "alpha\nours_insert\nbravo\ncharlie\ntheirs_append\n";
    assert_eq!(result.content, expected, "merged snapshot mismatch");
}

/// ours inserts after line 1; theirs inserts after line 3 (non-overlapping).
#[test]
fn auto_merge_single_file_ours_insert_at_head_theirs_insert_at_tail() {
    let base = "l1\nl2\nl3\n";
    let ours = "l1\nOURS_EXTRA\nl2\nl3\n";
    let theirs = "l1\nl2\nl3\nTHEIRS_EXTRA\n";

    let diff = make_diff("data.txt", base, ours, theirs);
    let result = auto_merge_conflict(&diff);

    assert!(
        !result.has_conflicts,
        "head insert (ours) and tail insert (theirs) must auto-merge; content={:?}",
        result.content
    );
    assert_eq!(result.remaining_conflicts, 0);
    assert_eq!(
        result.content, "l1\nOURS_EXTRA\nl2\nl3\nTHEIRS_EXTRA\n",
        "merged snapshot: both inserts must appear"
    );
}

// ── Case 2: multiple files treated independently ─────────────────────────────

/// file1 and file2 each have independent non-conflicting changes.
/// Each ThreeWayDiff auto-merges cleanly.
#[test]
fn auto_merge_two_files_each_with_independent_changes() {
    // file1: ours insert after l1, theirs insert at end
    let diff1 = make_diff(
        "file1.txt",
        "l1\nl2\nl3\n",
        "l1\nOURS_EXTRA\nl2\nl3\n",
        "l1\nl2\nl3\nTHEIRS_EXTRA\n",
    );
    let result1 = auto_merge_conflict(&diff1);
    assert!(
        !result1.has_conflicts,
        "file1: insertions at non-overlapping positions must auto-merge; content={:?}",
        result1.content
    );
    assert_eq!(result1.remaining_conflicts, 0);
    assert_eq!(
        result1.content, "l1\nOURS_EXTRA\nl2\nl3\nTHEIRS_EXTRA\n",
        "file1 snapshot mismatch"
    );

    // file2: ours insert after aaa, theirs insert after bbb
    let diff2 = make_diff(
        "file2.txt",
        "aaa\nbbb\nccc\n",
        "aaa\nOURS_NEW\nbbb\nccc\n",
        "aaa\nbbb\nTHEIRS_NEW\nccc\n",
    );
    let result2 = auto_merge_conflict(&diff2);
    assert!(
        !result2.has_conflicts,
        "file2: independent insertions must auto-merge; content={:?}",
        result2.content
    );
    assert_eq!(result2.remaining_conflicts, 0);
    assert_eq!(
        result2.content, "aaa\nOURS_NEW\nbbb\nTHEIRS_NEW\nccc\n",
        "file2 snapshot mismatch"
    );
}

// ── Case 3: conflict detection still works ───────────────────────────────────

/// Both sides edit the same line differently → true conflict, markers in output.
#[test]
fn auto_merge_leaves_true_conflict_with_markers() {
    let base = "alpha\nshared_line\nomega\n";
    let ours = "alpha\nOURS_EDIT\nomega\n";
    let theirs = "alpha\nTHEIRS_EDIT\nomega\n";

    let diff = make_diff("conflict.txt", base, ours, theirs);
    let result = auto_merge_conflict(&diff);

    assert!(
        result.has_conflicts,
        "overlapping edits to the same line must remain as conflict"
    );
    assert!(result.remaining_conflicts >= 1);
    assert!(
        result.content.contains("<<<<<<< HEAD"),
        "conflict markers must be present in output"
    );
    assert!(
        result.content.contains("OURS_EDIT"),
        "ours text must appear inside markers"
    );
    assert!(
        result.content.contains("THEIRS_EDIT"),
        "theirs text must appear inside markers"
    );
}

// ── Case 4: end-to-end with real git repo ────────────────────────────────────

/// Creates a real git repo with a real merge conflict by writing conflict markers
/// directly, then calls get_conflict_diff + auto_merge_conflict.
/// Uses a simpler approach: set up the conflict state manually via index stages.
#[test]
fn auto_merge_end_to_end_real_git_repo() {
    use git_core::get_conflict_diff;

    let repo = TestRepo::new().unwrap();
    let path = repo.path();

    // Commit 1 on main: base state
    repo.add_and_commit("shared.txt", "alpha\nbravo\ncharlie\n", "base")
        .unwrap();

    // Create two diverging commits from the same base:
    // Use low-level git operations to set up a conflict state.

    // Save base blob SHA
    let base_sha_out = run_git(path, &["rev-parse", "HEAD:shared.txt"]);
    let base_sha = String::from_utf8_lossy(&base_sha_out.stdout)
        .trim()
        .to_string();

    // Write "ours" version and get its blob SHA
    repo.write_file("shared.txt", "alpha\nFROM_OURS\nbravo\ncharlie\n")
        .unwrap();
    let ours_hash_out = run_git(path, &["hash-object", "-w", "shared.txt"]);
    let ours_sha = String::from_utf8_lossy(&ours_hash_out.stdout)
        .trim()
        .to_string();

    // Write "theirs" version and get its blob SHA
    repo.write_file("shared.txt", "alpha\nbravo\ncharlie\nFROM_THEIRS\n")
        .unwrap();
    let theirs_hash_out = run_git(path, &["hash-object", "-w", "shared.txt"]);
    let theirs_sha = String::from_utf8_lossy(&theirs_hash_out.stdout)
        .trim()
        .to_string();

    // Set up conflict in index: stage 1=base, 2=ours, 3=theirs
    run_git(path, &["rm", "--cached", "shared.txt"]);
    run_git(
        path,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{},shared.txt", base_sha),
            "--stage=1",
        ],
    );
    // update-index --add only supports one --cacheinfo at a time for conflict stages,
    // so we use a separate call for each stage
    run_git(
        path,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{},shared.txt", ours_sha),
            "--stage=2",
        ],
    );
    run_git(
        path,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{},shared.txt", theirs_sha),
            "--stage=3",
        ],
    );

    // Write the conflict markers file to workdir
    repo.write_file("shared.txt", &format!(
        "<<<<<<< HEAD\nalpha\nFROM_OURS\nbravo\ncharlie\n=======\nalpha\nbravo\ncharlie\nFROM_THEIRS\n>>>>>>>\n"
    )).unwrap();

    // Now read the conflict diff and auto-merge
    let r = git_core::Repository::discover(path).unwrap();
    let diff = get_conflict_diff(&r, Path::new("shared.txt")).unwrap();
    let result = auto_merge_conflict(&diff);

    // ours inserts after alpha; theirs appends at end — non-overlapping
    assert!(
        !result.has_conflicts,
        "independent insertions (ours after alpha, theirs at end) must auto-merge; content={:?}",
        result.content
    );
    assert_eq!(result.remaining_conflicts, 0);
    assert!(
        result.content.contains("FROM_OURS"),
        "ours insertion must appear"
    );
    assert!(
        result.content.contains("FROM_THEIRS"),
        "theirs insertion must appear"
    );
    assert!(
        result.content.contains("alpha"),
        "unchanged alpha must be preserved"
    );
    assert!(
        result.content.contains("bravo"),
        "unchanged bravo must be preserved"
    );
    assert!(
        result.content.contains("charlie"),
        "unchanged charlie must be preserved"
    );
}
