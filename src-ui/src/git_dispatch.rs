//! Helpers for dispatching blocking git-core operations off the UI thread.
//!
//! Nearly all git-core functions are synchronous (they call libgit2 or shell out to `git`).
//! Running them directly inside Iced's `update()` blocks the UI event loop and causes
//! frame drops during operations like `get_status`, `stage_file`, or `load_history`.
//!
//! This module provides lightweight wrappers that move the blocking work onto Tokio's
//! blocking thread pool via `tokio::task::spawn_blocking`, returning an Iced `Task`
//! that resolves to a `Message` when complete.
//!
//! ## Usage pattern
//!
//! ```ignore
//! // In an update() handler:
//! Message::Refresh => {
//!     let repo = state.current_repository.clone().unwrap();
//!     git_dispatch::run(move || {
//!         let status = git_core::get_status(&repo)?;
//!         let conflicts = git_core::index::get_conflicted_files(&repo)?;
//!         Ok(RefreshResult { status, conflicts })
//!     }, Message::RefreshComplete)
//! }
//! ```

use iced::Task;

/// Dispatch a blocking closure onto Tokio's blocking thread pool,
/// mapping the result to a `Message` via the provided constructor.
///
/// The closure `f` runs on a dedicated blocking thread so the UI thread is never stalled.
/// Panics inside `f` are caught and propagated as `Err` to the message constructor.
pub fn run<F, T, M, G>(f: F, to_message: G) -> Task<M>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
    M: Send + 'static,
    G: FnOnce(Result<T, String>) -> M + Send + 'static,
{
    Task::perform(
        async move {
            tokio::task::spawn_blocking(f)
                .await
                .unwrap_or_else(|join_err| Err(format!("git worker panicked: {join_err}")))
        },
        to_message,
    )
}

/// Like [`run`], but for infallible operations that always produce a value.
#[allow(dead_code)]
pub fn run_ok<F, T, M, G>(f: F, to_message: G) -> Task<M>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
    M: Send + 'static,
    G: FnOnce(T) -> M + Send + 'static,
{
    Task::perform(
        async move { tokio::task::spawn_blocking(f).await.unwrap() },
        to_message,
    )
}
