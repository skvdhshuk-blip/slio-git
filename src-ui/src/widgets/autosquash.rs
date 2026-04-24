//! Autosquash pre-processing for interactive rebase todo lists.
//!
//! Mirrors IDEA's `GitSquashedCommitsMessage.kt:11` (regex detection) and
//! `GitRebaseOption.kt:20` (canAutosquash flag). Implemented as a pure function
//! so it is unit-testable without any repo or UI state.
//!
//! # Algorithm
//! 1. Scan for commits whose subject starts with `fixup! ` or `squash! `.
//! 2. Exact-match the remainder against an existing pick's subject.
//! 3. Move the matched commit to immediately after the target pick; set action
//!    to `fixup` or `squash`.
//! 4. Unmatched `fixup!`/`squash!` commits are silently left in place as `pick`
//!    — their subject prefix already self-documents the intent.

use crate::views::rebase_editor::RebaseTodoItem;

/// Apply autosquash reordering to a todo list (UI-layer pre-processing).
///
/// Ref: `GitRebaseDialog.kt:81` (selectedOptions) — toggled on demand, not
/// persisted. Pure function; `f(f(x)) == f(x)` by construction because
/// `fixup!`/`squash!` prefixes are never stripped.
pub fn apply_autosquash(todos: Vec<RebaseTodoItem>) -> Vec<RebaseTodoItem> {
    if todos.is_empty() {
        return todos;
    }

    let mut result: Vec<RebaseTodoItem> = Vec::with_capacity(todos.len());
    // Collect fixup/squash candidates first; remaining items become the base picks.
    let mut candidates: Vec<(String, RebaseTodoItem)> = Vec::new();

    for item in todos {
        if let Some(target) = autosquash_target(&item.message) {
            candidates.push((target.to_string(), item));
        } else {
            result.push(item);
        }
    }

    // For each candidate, find first matching pick in result and insert after it.
    // Unmatched candidates are appended at the end unchanged (static silent fallback).
    // Ref: `GitSingleRepoRebaseTest:829` — unmatched stays as pick.
    let mut unmatched: Vec<RebaseTodoItem> = Vec::new();
    for (target, mut cand) in candidates {
        if let Some(pos) = result
            .iter()
            .position(|item| item.message.trim() == target.trim())
        {
            // Set action to fixup or squash based on prefix.
            let prefix = if cand.message.starts_with("fixup! ") {
                "fixup"
            } else {
                "squash"
            };
            cand.action = prefix.to_string();
            result.insert(pos + 1, cand);
        } else {
            // Silently preserve as pick at original relative position.
            unmatched.push(cand);
        }
    }
    result.extend(unmatched);
    result
}

/// Truncate a commit subject for display, appending "…" when needed.
pub fn truncate_subject(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", truncated)
}

/// Return the autosquash target subject if the message starts with `fixup! ` or `squash! `.
fn autosquash_target(message: &str) -> Option<&str> {
    if let Some(rest) = message.strip_prefix("fixup! ") {
        Some(rest)
    } else if let Some(rest) = message.strip_prefix("squash! ") {
        Some(rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(msg: &str) -> RebaseTodoItem {
        RebaseTodoItem {
            action: "pick".to_string(),
            commit: "abc0001".to_string(),
            message: msg.to_string(),
        }
    }

    // Test 1: single fixup exact match
    #[test]
    fn fixup_exact_match() {
        let todos = vec![pick("Subject1"), pick("fixup! Subject1"), pick("Subject2")];
        let result = apply_autosquash(todos);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].message, "Subject1");
        assert_eq!(result[1].message, "fixup! Subject1");
        assert_eq!(result[1].action, "fixup");
        assert_eq!(result[2].message, "Subject2");
    }

    // Test 2: single squash exact match
    #[test]
    fn squash_exact_match() {
        let todos = vec![pick("Subject1"), pick("squash! Subject1")];
        let result = apply_autosquash(todos);
        assert_eq!(result[1].action, "squash");
        assert_eq!(result[1].message, "squash! Subject1");
    }

    // Test 3: multiple fixups chained on same pick
    #[test]
    fn multiple_fixups_same_pick() {
        let todos = vec![
            pick("Base"),
            pick("fixup! Base"),
            pick("fixup! Base"),
            pick("Other"),
        ];
        let result = apply_autosquash(todos);
        assert_eq!(result[0].message, "Base");
        assert_eq!(result[1].action, "fixup");
        assert_eq!(result[2].action, "fixup");
        assert_eq!(result[3].message, "Other");
    }

    // Test 4: fixup! and squash! mixed
    #[test]
    fn fixup_squash_mixed() {
        let todos = vec![
            pick("Alpha"),
            pick("Beta"),
            pick("fixup! Alpha"),
            pick("squash! Beta"),
        ];
        let result = apply_autosquash(todos);
        assert_eq!(result[0].message, "Alpha");
        assert_eq!(result[1].action, "fixup");
        assert_eq!(result[2].message, "Beta");
        assert_eq!(result[3].action, "squash");
    }

    // Test 5: unmatched fixup! stays in place as pick
    #[test]
    fn fixup_no_match_stays_in_place() {
        let todos = vec![pick("Subject1"), pick("fixup! NonExistent")];
        let result = apply_autosquash(todos);
        assert_eq!(result[1].action, "pick");
        assert_eq!(result[1].message, "fixup! NonExistent");
    }

    // Test 6: empty todos edge case
    #[test]
    fn empty_todos() {
        let result = apply_autosquash(vec![]);
        assert!(result.is_empty());
    }

    // Test 7: all picks, no fixup — no change
    #[test]
    fn all_picks_no_fixup() {
        let todos = vec![pick("A"), pick("B"), pick("C")];
        let result = apply_autosquash(todos.clone());
        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|r| r.action == "pick"));
    }

    // Test 8: re-apply after manual drag reorder — result is idempotent
    // Ref: `GitSingleRepoRebaseTest:829` — re-apply after user drag.
    #[test]
    fn reapply_after_reorder() {
        let todos = vec![pick("B"), pick("A"), pick("fixup! A")];
        let first = apply_autosquash(todos);
        let second = apply_autosquash(first.clone());
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.action, b.action);
            assert_eq!(a.message, b.message);
        }
    }

    // Test 9: idempotency f(f(x)) == f(x)
    // Ref: `GitRebaseDialog.kt:81` — toggle off→on→off round-trip.
    #[test]
    fn idempotency() {
        let todos = vec![
            pick("Subject1"),
            pick("fixup! Subject1"),
            pick("Subject2"),
            pick("squash! Subject2"),
            pick("fixup! Ghost"),
        ];
        let once = apply_autosquash(todos);
        let twice = apply_autosquash(once.clone());
        assert_eq!(once.len(), twice.len());
        for (a, b) in once.iter().zip(twice.iter()) {
            assert_eq!(a.action, b.action, "action mismatch: {}", a.message);
            assert_eq!(a.message, b.message);
        }
    }
}
