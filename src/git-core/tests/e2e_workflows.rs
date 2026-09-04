//! End-to-end workflow tests for git-core.
//!
//! Each test exercises a complete multi-step user workflow across multiple
//! modules, catching integration bugs that single-module tests miss.

mod test_helpers;

use git_core::diff::{diff_index_to_head, diff_workdir_to_index};
use git_core::index;
use git_core::repository::RepositoryState;
use git_core::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use test_helpers::TestRepo;

// ─── helpers ───────────────────────────────────────────────────────────────

fn run_git(repo_path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn try_run_git(repo_path: &Path, args: &[&str]) -> bool {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("run git command");
    output.status.success()
}

fn commit_count(repo_path: &Path) -> usize {
    let out = run_git(repo_path, &["rev-list", "--count", "HEAD"]);
    out.parse().unwrap_or(0)
}

fn commit_ids_oldest_first(repo_path: &Path) -> Vec<String> {
    run_git(repo_path, &["rev-list", "--reverse", "HEAD"])
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

// ─── 1. Interactive rebase: squash + drop ──────────────────────────────────

#[test]
#[cfg(not(feature = "app-store"))]
fn test_e2e_interactive_rebase_squash_and_drop() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    // Create 4 commits: A → B → C → D
    tr.add_and_commit("file.txt", "A\n", "commit A").unwrap();
    tr.add_and_commit("file.txt", "A\nB\n", "commit B").unwrap();
    tr.add_and_commit("file.txt", "A\nB\nC\n", "commit C")
        .unwrap();
    tr.add_and_commit("file.txt", "A\nB\nC\nD\n", "commit D")
        .unwrap();

    assert_eq!(commit_count(p), 4);

    let repo = Repository::discover(p).unwrap();
    let commits = commit_ids_oldest_first(p);

    // Prepare plan starting from commit B (index 1)
    let plan = prepare_interactive_rebase_plan(&repo, &commits[1]).unwrap();
    assert_eq!(plan.entries.len(), 3); // B, C, D

    // Modify plan: pick B, squash C into B, drop D
    let entries = vec![
        RebaseTodoEntry {
            action: "pick".to_string(),
            commit: plan.entries[0].commit.clone(),
            message: plan.entries[0].message.clone(),
        },
        RebaseTodoEntry {
            action: "squash".to_string(),
            commit: plan.entries[1].commit.clone(),
            message: plan.entries[1].message.clone(),
        },
        // D is dropped (not included)
    ];

    let result = start_interactive_rebase(&repo, plan.base_ref.as_deref(), &entries);
    assert!(result.is_ok(), "rebase failed: {:?}", result.err());

    // After rebase: A + (B squashed with C) = 2 commits
    assert_eq!(commit_count(p), 2);

    // Final file should contain A, B, C (D was dropped)
    let content = fs::read_to_string(p.join("file.txt")).unwrap();
    assert!(content.contains('A'));
    assert!(content.contains('B'));
    assert!(content.contains('C'));
    assert!(!content.contains('D'));
}

// ─── 2. Merge conflict: detect → resolve → commit ─────────────────────────

#[test]
fn test_e2e_merge_conflict_detect_resolve_commit() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    // Create base commit
    tr.add_and_commit("shared.txt", "base content\n", "base commit")
        .unwrap();

    // Create branch-a with conflicting change
    run_git(p, &["checkout", "-b", "branch-a"]);
    tr.add_and_commit("shared.txt", "content from A\n", "branch A change")
        .unwrap();

    // Go back to master and create conflicting change
    run_git(p, &["checkout", "master"]);
    tr.add_and_commit("shared.txt", "content from main\n", "main change")
        .unwrap();

    // Attempt merge (will conflict)
    let merge_ok = try_run_git(p, &["merge", "branch-a", "--no-edit"]);
    assert!(!merge_ok, "merge should conflict");

    let repo = Repository::discover(p).unwrap();

    // Detect conflict state
    assert_eq!(repo.get_state(), RepositoryState::Merging);
    assert!(index::has_conflicts(&repo));

    // Resolve conflict using "Ours" strategy
    resolve_conflict(&repo, Path::new("shared.txt"), ConflictResolution::Ours).unwrap();
    assert!(!index::has_conflicts(&repo));

    // Complete the merge commit via git (git2 create_commit doesn't finalize merge state)
    run_git(p, &["commit", "-m", "merge resolved"]);

    // Verify clean state and merge commit exists
    let repo = Repository::discover(p).unwrap();
    assert_eq!(repo.get_state(), RepositoryState::Clean);

    let history = get_history(&repo, Some(10)).unwrap();
    assert!(history.iter().any(|h| h.message.contains("merge resolved")));
    // Merge commit has 2 parents
    let merge_entry = history
        .iter()
        .find(|h| h.message.contains("merge resolved"))
        .unwrap();
    assert_eq!(merge_entry.parent_ids.len(), 2);
}

// ─── 3. Stash + branch switch + pop ───────────────────────────────────────

#[test]
fn test_e2e_stash_branch_switch_pop() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("main.txt", "main content\n", "initial commit")
        .unwrap();

    // Make dirty changes
    tr.write_file("main.txt", "modified content\n").unwrap();

    let repo = Repository::discover(p).unwrap();

    // Stash dirty changes
    stash_save(&repo, Some("wip changes")).unwrap();

    // Verify clean state
    let status = index::get_status(&repo).unwrap();
    assert!(status.is_empty(), "workdir should be clean after stash");

    // Create and switch to new branch
    run_git(p, &["checkout", "-b", "feature"]);
    tr.add_and_commit("feature.txt", "feature work\n", "feature commit")
        .unwrap();

    // Switch back to master
    run_git(p, &["checkout", "master"]);

    // Pop stash
    let repo = Repository::discover(p).unwrap();
    stash_pop(&repo, 0).unwrap();

    // Verify changes restored
    let content = fs::read_to_string(p.join("main.txt")).unwrap();
    assert_eq!(content, "modified content\n");

    // Verify stash list is now empty
    let stashes = list_stashes(&repo).unwrap();
    assert!(stashes.is_empty());
}

// ─── 4. Multi-branch diverge & merge with graph ───────────────────────────

#[test]
fn test_e2e_multi_branch_diverge_merge_graph() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    // Base commit on master
    tr.add_and_commit("base.txt", "base\n", "initial commit")
        .unwrap();

    // Create branch-a and add commit
    run_git(p, &["checkout", "-b", "branch-a"]);
    tr.add_and_commit("a.txt", "from A\n", "commit on branch-a")
        .unwrap();

    // Go back to master, create branch-b and add commit
    run_git(p, &["checkout", "master"]);
    run_git(p, &["checkout", "-b", "branch-b"]);
    tr.add_and_commit("b.txt", "from B\n", "commit on branch-b")
        .unwrap();

    // Merge branch-a into master
    run_git(p, &["checkout", "master"]);
    run_git(p, &["merge", "branch-a", "--no-ff", "-m", "merge branch-a"]);

    // Merge branch-b into master
    run_git(p, &["merge", "branch-b", "--no-ff", "-m", "merge branch-b"]);

    let repo = Repository::discover(p).unwrap();
    let history = get_history(&repo, Some(20)).unwrap();

    // Should have merge commits
    let merge_a = history
        .iter()
        .find(|h| h.message.contains("merge branch-a"));
    let merge_b = history
        .iter()
        .find(|h| h.message.contains("merge branch-b"));
    assert!(merge_a.is_some(), "merge-a commit should exist");
    assert!(merge_b.is_some(), "merge-b commit should exist");
    assert_eq!(merge_a.unwrap().parent_ids.len(), 2);
    assert_eq!(merge_b.unwrap().parent_ids.len(), 2);

    // Verify graph has merge nodes
    let commit_ids: Vec<String> = history.iter().map(|h| h.id.clone()).collect();
    let graph = compute_graph(&repo, &commit_ids).unwrap();
    let merge_nodes: Vec<_> = graph.iter().filter(|n| n.is_merge).collect();
    assert!(
        merge_nodes.len() >= 2,
        "graph should contain at least 2 merge nodes, got {}",
        merge_nodes.len()
    );

    // Verify ref labels include master
    let labels = compute_ref_labels(&repo).unwrap();
    let all_names: Vec<&str> = labels
        .values()
        .flat_map(|v| v.iter().map(|l| l.name.as_str()))
        .collect();
    assert!(all_names.contains(&"master"), "master label should exist");
}

// ─── 5. Partial staging: stage one file, leave another unstaged ────────────

#[test]
fn test_e2e_partial_stage_commit_verify_remainder() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("a.txt", "original A\n", "initial commit")
        .unwrap();
    tr.add_and_commit("b.txt", "original B\n", "add B").unwrap();

    // Modify both files
    tr.write_file("a.txt", "modified A\n").unwrap();
    tr.write_file("b.txt", "modified B\n").unwrap();

    let repo = Repository::discover(p).unwrap();

    // Verify both files show as changed
    let status = index::get_status(&repo).unwrap();
    assert_eq!(status.len(), 2, "both files should be modified");

    // Stage only file a.txt
    index::stage_file(&repo, Path::new("a.txt")).unwrap();

    // Verify a.txt is staged, b.txt is not
    let diff_staged = diff_index_to_head(&repo, Path::new("a.txt")).unwrap();
    assert!(!diff_staged.files.is_empty(), "a.txt should be staged");

    // Commit only the staged file
    create_commit(&repo, "partial: only A changed", "", "").unwrap();

    // Verify b.txt change is still in working directory
    let remaining = diff_workdir_to_index(&repo).unwrap();
    assert!(
        !remaining.files.is_empty(),
        "should still have unstaged changes for b.txt"
    );

    let b_content = fs::read_to_string(p.join("b.txt")).unwrap();
    assert_eq!(b_content, "modified B\n");

    // Verify the committed content of a.txt
    let a_committed = run_git(p, &["show", "HEAD:a.txt"]);
    assert_eq!(a_committed, "modified A");
}

// ─── 6. Tag lifecycle + graph labels ──────────────────────────────────────

#[test]
fn test_e2e_tag_lifecycle_with_graph_labels() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("v1.txt", "release 1\n", "release 1")
        .unwrap();
    tr.add_and_commit("v2.txt", "release 2\n", "release 2")
        .unwrap();

    let repo = Repository::discover(p).unwrap();

    // Create annotated tag (need tagger name and email)
    create_tag(
        &repo,
        "v1.0.0",
        "HEAD~1",
        "Version 1.0.0",
        "Tagger",
        "tagger@example.com",
    )
    .unwrap();

    // Create lightweight tag
    create_lightweight_tag(&repo, "v2.0.0-rc1", "HEAD").unwrap();

    // List tags
    let tags = list_tags(&repo).unwrap();
    let tag_names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    assert!(tag_names.contains(&"v1.0.0"), "annotated tag should exist");
    assert!(
        tag_names.contains(&"v2.0.0-rc1"),
        "lightweight tag should exist"
    );

    // Verify annotated tag has tagger info (message body may be empty for
    // single-line messages since git for-each-ref %(contents:body) only
    // returns lines after the subject)
    let v1_tag = tags.iter().find(|t| t.name == "v1.0.0").unwrap();
    assert!(
        v1_tag.tagger_name.is_some(),
        "annotated tag should have tagger name"
    );

    // Verify tags show in graph ref labels
    let labels = compute_ref_labels(&repo).unwrap();
    let all_label_names: Vec<&str> = labels
        .values()
        .flat_map(|v| v.iter().map(|l| l.name.as_str()))
        .collect();
    assert!(
        all_label_names.contains(&"v1.0.0"),
        "v1.0.0 should appear in ref labels"
    );
    assert!(
        all_label_names.contains(&"v2.0.0-rc1"),
        "v2.0.0-rc1 should appear in ref labels"
    );

    // Delete annotated tag
    delete_tag(&repo, "v1.0.0").unwrap();

    let tags_after = list_tags(&repo).unwrap();
    let names_after: Vec<&str> = tags_after.iter().map(|t| t.name.as_str()).collect();
    assert!(
        !names_after.contains(&"v1.0.0"),
        "deleted tag should be gone"
    );
    assert!(
        names_after.contains(&"v2.0.0-rc1"),
        "other tag should remain"
    );
}

// ─── 7. Worktree parallel development ─────────────────────────────────────

#[test]
fn test_e2e_worktree_parallel_commit_remove() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("main.txt", "main content\n", "initial commit")
        .unwrap();

    // Create a branch for the worktree
    run_git(p, &["branch", "wt-branch"]);

    let repo = Repository::discover(p).unwrap();

    // Create worktree
    let wt_path = p.parent().unwrap().join("test-worktree");
    create_worktree(&repo, &wt_path, Some("wt-branch")).unwrap();

    // List worktrees should show both
    let worktrees = list_worktrees(&repo).unwrap();
    assert!(
        worktrees.len() >= 2,
        "should have main + new worktree, got {}",
        worktrees.len()
    );

    // Commit in the worktree
    fs::write(wt_path.join("wt-file.txt"), "worktree content\n").unwrap();
    run_git(&wt_path, &["add", "wt-file.txt"]);
    run_git(&wt_path, &["commit", "-m", "commit in worktree"]);

    // Verify commit exists on wt-branch
    let wt_log = run_git(&wt_path, &["log", "--oneline", "-1"]);
    assert!(wt_log.contains("commit in worktree"));

    // Remove worktree
    remove_worktree(&repo, &wt_path).unwrap();

    let worktrees_after = list_worktrees(&repo).unwrap();
    assert!(
        worktrees_after.len() < worktrees.len(),
        "worktree count should decrease"
    );

    // Branch still has the commit
    let branch_log = run_git(p, &["log", "wt-branch", "--oneline", "-1"]);
    assert!(branch_log.contains("commit in worktree"));
}

// ─── 8. Amend + fixup + squash chain ─────────────────────────────────────

#[test]
#[cfg(not(feature = "app-store"))]
fn test_e2e_amend_fixup_squash_chain() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("a.txt", "a\n", "commit A").unwrap();
    tr.add_and_commit("b.txt", "b\n", "commit B").unwrap();
    tr.add_and_commit("c.txt", "c\n", "commit C").unwrap();

    assert_eq!(commit_count(p), 3);

    let repo = Repository::discover(p).unwrap();

    // Amend last commit message
    let head_id = run_git(p, &["rev-parse", "HEAD"]);
    amend_commit(&repo, &head_id, "commit C amended").unwrap();

    // Verify amended message
    let log = run_git(p, &["log", "-1", "--format=%s"]);
    assert_eq!(log, "commit C amended");
    assert_eq!(commit_count(p), 3); // count unchanged

    // Fixup: merge last commit into previous (C into B)
    let commits = commit_ids_oldest_first(p);
    let result = fixup_commit_to_previous(&repo, &commits[2]).unwrap();
    assert_eq!(result, RewriteExecution::Completed);
    assert_eq!(commit_count(p), 2);

    // Squash: merge last commit into previous (B+C into A)
    let commits = commit_ids_oldest_first(p);
    let result = squash_commit_to_previous(&repo, &commits[1]).unwrap();
    assert_eq!(result, RewriteExecution::Completed);
    assert_eq!(commit_count(p), 1);

    // Final commit should have combined message
    let final_msg = run_git(p, &["log", "-1", "--format=%B"]);
    assert!(
        final_msg.contains("commit A"),
        "squashed message should contain A: {}",
        final_msg
    );
}

// ─── 9. History filtering combination ─────────────────────────────────────

#[test]
fn test_e2e_history_combined_filters() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    // Commit by default author (Codex Test)
    tr.add_and_commit("alpha.txt", "alpha\n", "alpha file added")
        .unwrap();
    tr.add_and_commit("beta.txt", "beta\n", "beta file added")
        .unwrap();

    // Commit by different author
    run_git(p, &["config", "user.name", "Other Author"]);
    run_git(p, &["config", "user.email", "other@example.com"]);
    tr.add_and_commit("gamma.txt", "gamma\n", "gamma by other")
        .unwrap();

    // Restore original author
    run_git(p, &["config", "user.name", "Codex Test"]);
    run_git(p, &["config", "user.email", "codex@example.com"]);
    tr.add_and_commit("delta.txt", "delta\n", "delta file added")
        .unwrap();

    let repo = Repository::discover(p).unwrap();

    // Filter by author
    let by_codex = get_history_for_author(&repo, "Codex Test", Some(20)).unwrap();
    assert_eq!(by_codex.len(), 3); // alpha, beta, delta
    assert!(by_codex.iter().all(|h| h.author_name == "Codex Test"));

    let by_other = get_history_for_author(&repo, "Other Author", Some(20)).unwrap();
    assert_eq!(by_other.len(), 1);
    assert!(by_other[0].message.contains("gamma"));

    // Filter by path
    let by_path = get_history_for_path(&repo, "alpha.txt", Some(20)).unwrap();
    assert_eq!(by_path.len(), 1);
    assert!(by_path[0].message.contains("alpha"));

    // Search by message text
    let search_results = search_history(&repo, "beta", Some(20)).unwrap();
    assert_eq!(search_results.len(), 1);
    assert!(search_results[0].message.contains("beta"));

    // Full history should have all 4
    let all = get_history(&repo, Some(20)).unwrap();
    assert_eq!(all.len(), 4);
}

// ─── 10. Repository state machine transitions ─────────────────────────────

#[test]
#[cfg(not(feature = "app-store"))]
fn test_e2e_repo_state_transitions() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("file.txt", "original\n", "initial commit")
        .unwrap();

    let repo = Repository::discover(p).unwrap();
    // git2 RepositoryState only tracks special states (Merge, Rebase, etc.)
    // Clean/Dirty depends on index status, not git2::RepositoryState
    assert_eq!(repo.get_state(), RepositoryState::Clean);

    // Verify dirty detection via index status
    tr.write_file("file.txt", "modified\n").unwrap();
    let status = index::get_status(&repo).unwrap();
    assert!(!status.is_empty(), "should detect dirty working tree");

    // Restore clean state
    run_git(p, &["checkout", "--", "file.txt"]);
    let status = index::get_status(&repo).unwrap();
    assert!(status.is_empty(), "should be clean after checkout");

    // Set up merge conflict → Merging state
    run_git(p, &["checkout", "-b", "conflict-branch"]);
    tr.add_and_commit("file.txt", "conflict branch content\n", "conflict branch")
        .unwrap();
    run_git(p, &["checkout", "master"]);
    tr.add_and_commit("file.txt", "master content\n", "master update")
        .unwrap();
    let merge_ok = try_run_git(p, &["merge", "conflict-branch", "--no-edit"]);
    assert!(!merge_ok);

    let repo = Repository::discover(p).unwrap();
    assert_eq!(repo.get_state(), RepositoryState::Merging);

    // Abort merge → Clean
    run_git(p, &["merge", "--abort"]);
    let repo = Repository::discover(p).unwrap();
    assert_eq!(repo.get_state(), RepositoryState::Clean);

    // Set up rebase conflict → Rebasing state
    // Use get_rebase_status() which checks for rebase-merge/rebase-apply dirs
    // (more reliable than get_state() which only maps some git2 rebase variants)
    run_git(p, &["checkout", "conflict-branch"]);
    let rebase_ok = try_run_git(p, &["rebase", "master"]);
    if !rebase_ok {
        let repo = Repository::discover(p).unwrap();
        let rebase_status = get_rebase_status(&repo).unwrap();
        assert!(rebase_status.is_some(), "should detect rebase in progress");

        // Abort rebase → Clean
        rebase_abort(&repo).unwrap();
        let repo = Repository::discover(p).unwrap();
        let rebase_status = get_rebase_status(&repo).unwrap();
        assert!(rebase_status.is_none(), "rebase should be aborted");
        assert_eq!(repo.get_state(), RepositoryState::Clean);
    }
}
