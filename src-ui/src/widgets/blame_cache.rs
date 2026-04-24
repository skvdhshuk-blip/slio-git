//! LRU-style in-memory blame cache keyed by (path, rev).

use git_core::BlameInfo;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Default)]
pub struct BlameCache {
    map: RwLock<HashMap<(PathBuf, String), Arc<Vec<BlameInfo>>>>,
}

impl BlameCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, path: &PathBuf, rev: &str) -> Option<Arc<Vec<BlameInfo>>> {
        self.map
            .read()
            .unwrap()
            .get(&(path.clone(), rev.to_string()))
            .cloned()
    }

    pub fn insert(&self, path: PathBuf, rev: String, data: Vec<BlameInfo>) {
        self.map
            .write()
            .unwrap()
            .insert((path, rev), Arc::new(data));
    }

    /// Invalidate all cached entries (e.g. on HEAD switch).
    pub fn clear(&self) {
        self.map.write().unwrap().clear();
    }
}
