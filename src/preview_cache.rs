//! LRU cache for file preview content.

use lru::LruCache;
use parking_lot::RwLock;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// Cached preview content for a file.
#[derive(Clone)]
pub struct PreviewContent {
    /// The text content of the file.
    pub text: String,
    /// Whether the file appears to be binary (contains null bytes).
    pub is_binary: bool,
    /// Whether the content was truncated due to size limits.
    pub truncated: bool,
}

/// Thread-safe LRU cache for file previews.
pub struct PreviewCache {
    cache: RwLock<LruCache<PathBuf, PreviewContent>>,
}

impl PreviewCache {
    /// Create a new cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(256).unwrap());
        Self {
            cache: RwLock::new(LruCache::new(cap)),
        }
    }

    /// Get cached content for a path, if present.
    pub fn get(&self, path: &PathBuf) -> Option<PreviewContent> {
        self.cache.write().get(path).cloned()
    }

    /// Insert content into the cache.
    pub fn insert(&self, path: PathBuf, content: PreviewContent) {
        self.cache.write().put(path, content);
    }

    /// Invalidate (remove) a specific path from the cache.
    pub fn invalidate(&self, path: &PathBuf) {
        self.cache.write().pop(path);
    }

    /// Clear the entire cache.
    pub fn clear(&self) {
        self.cache.write().clear();
    }
}
