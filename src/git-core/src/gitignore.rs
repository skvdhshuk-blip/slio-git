//! Git-ignore template registry and apply helpers.
//!
//! Templates are embedded at compile time via [`include_str!`] — no network
//! access, no extra crate dependency. Mirrors the IDEA platform behaviour of
//! `addNewElementsToIgnoreFile` (append-only, never overwrite).

use crate::error::GitError;
use crate::repository::Repository;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;

/// (display_name, raw_content) pairs, ordered for the dropdown UI.
const TEMPLATES: &[(&str, &str)] = &[
    ("Rust", include_str!("gitignore_templates/Rust.gitignore")),
    (
        "Python",
        include_str!("gitignore_templates/Python.gitignore"),
    ),
    (
        "JavaScript",
        include_str!("gitignore_templates/JavaScript.gitignore"),
    ),
    (
        "TypeScript",
        include_str!("gitignore_templates/TypeScript.gitignore"),
    ),
    ("Node", include_str!("gitignore_templates/Node.gitignore")),
    ("Go", include_str!("gitignore_templates/Go.gitignore")),
    ("Java", include_str!("gitignore_templates/Java.gitignore")),
    ("C", include_str!("gitignore_templates/C.gitignore")),
    (
        "C++",
        include_str!("gitignore_templates/CPlusPlus.gitignore"),
    ),
    ("Ruby", include_str!("gitignore_templates/Ruby.gitignore")),
    ("Swift", include_str!("gitignore_templates/Swift.gitignore")),
    (
        "Kotlin",
        include_str!("gitignore_templates/Kotlin.gitignore"),
    ),
    ("Dart", include_str!("gitignore_templates/Dart.gitignore")),
    (".NET", include_str!("gitignore_templates/Dotnet.gitignore")),
    ("macOS", include_str!("gitignore_templates/macOS.gitignore")),
    ("Linux", include_str!("gitignore_templates/Linux.gitignore")),
    (
        "Windows",
        include_str!("gitignore_templates/Windows.gitignore"),
    ),
];

/// Names of all bundled templates, in display order.
pub fn gitignore_list_templates() -> Vec<&'static str> {
    TEMPLATES.iter().map(|(name, _)| *name).collect()
}

/// Raw content of a template, or `None` if the name is unknown.
pub fn gitignore_template_content(name: &str) -> Option<&'static str> {
    TEMPLATES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, content)| *content)
}

fn gitignore_path(repo: &Repository) -> PathBuf {
    repo.command_cwd().join(".gitignore")
}

/// Append the named template to the repository's root `.gitignore`.
///
/// Creates the file if missing. Existing content is preserved — a blank line
/// is inserted before the new block when needed, and a header comment marks
/// the template's section. Mirrors IDEA's `addNewElementsToIgnoreFile`.
pub fn gitignore_add(repo: &Repository, template_name: &str) -> Result<(), GitError> {
    let content =
        gitignore_template_content(template_name).ok_or_else(|| GitError::OperationFailed {
            operation: "gitignore_add".to_string(),
            details: format!("Unknown template: {template_name}"),
        })?;

    let path = gitignore_path(repo);

    let existing =
        if path.exists() {
            let mut file = OpenOptions::new().read(true).open(&path).map_err(|e| {
                GitError::OperationFailed {
                    operation: "gitignore_add".to_string(),
                    details: format!("Failed to read .gitignore: {e}"),
                }
            })?;
            let mut buf = String::new();
            file.read_to_string(&mut buf)
                .map_err(|e| GitError::OperationFailed {
                    operation: "gitignore_add".to_string(),
                    details: format!("Failed to read .gitignore: {e}"),
                })?;
            buf
        } else {
            String::new()
        };

    let mut to_append = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        to_append.push('\n');
    }
    if !existing.is_empty() {
        to_append.push('\n');
    }
    to_append.push_str(&format!("# === {template_name} ===\n"));
    to_append.push_str(content);
    if !to_append.ends_with('\n') {
        to_append.push('\n');
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| GitError::OperationFailed {
            operation: "gitignore_add".to_string(),
            details: format!("Failed to open .gitignore for append: {e}"),
        })?;

    file.write_all(to_append.as_bytes())
        .map_err(|e| GitError::OperationFailed {
            operation: "gitignore_add".to_string(),
            details: format!("Failed to append .gitignore: {e}"),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fresh_repo() -> (Repository, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init");
        (repo, dir)
    }

    #[test]
    fn templates_are_listed() {
        let names = gitignore_list_templates();
        assert!(names.contains(&"Rust"));
        assert!(names.contains(&"Python"));
        assert!(names.contains(&"macOS"));
        assert!(names.len() >= 10);
    }

    #[test]
    fn template_content_lookup() {
        let rust = gitignore_template_content("Rust").expect("rust template");
        assert!(rust.contains("target/"));
        assert!(gitignore_template_content("nonexistent").is_none());
    }

    #[test]
    fn append_creates_file_when_missing() {
        let (repo, _dir) = fresh_repo();
        gitignore_add(&repo, "Rust").expect("apply rust");
        let body = fs::read_to_string(repo.command_cwd().join(".gitignore")).expect("read");
        assert!(body.contains("# === Rust ==="));
        assert!(body.contains("target/"));
    }

    #[test]
    fn append_preserves_existing_content() {
        let (repo, _dir) = fresh_repo();
        let path = repo.command_cwd().join(".gitignore");
        fs::write(&path, "/secret\n").expect("seed");
        gitignore_add(&repo, "Python").expect("apply python");
        let body = fs::read_to_string(&path).expect("read");
        assert!(body.starts_with("/secret\n"));
        assert!(body.contains("__pycache__/"));
    }

    #[test]
    fn unknown_template_errors() {
        let (repo, _dir) = fresh_repo();
        let err = gitignore_add(&repo, "Nope").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Nope"));
    }

    #[test]
    fn append_twice_does_not_overwrite() {
        let (repo, _dir) = fresh_repo();
        gitignore_add(&repo, "Rust").unwrap();
        gitignore_add(&repo, "macOS").unwrap();
        let body = fs::read_to_string(repo.command_cwd().join(".gitignore")).unwrap();
        assert!(body.contains("# === Rust ==="));
        assert!(body.contains("# === macOS ==="));
        assert!(body.contains(".DS_Store"));
    }
}
