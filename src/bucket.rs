use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static BUCKET_INSTANCE_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default, Serialize, Deserialize)]
struct BucketData {
    #[serde(default)]
    items: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Bucket {
    pub items: Vec<PathBuf>,
    pub cursor: usize,
    pub selected: HashSet<usize>,
    staging_dir: PathBuf,
    storage_path: Option<PathBuf>,
}

impl Bucket {
    /// Creates an ephemeral in-memory bucket (not persisted to disk).
    pub fn new() -> Self {
        Self::with_storage(None)
    }

    /// Loads a persistent bucket from `storage_path` (creating the file on first mutation).
    pub fn load(storage_path: PathBuf) -> Self {
        Self::with_storage(Some(storage_path))
    }

    fn with_storage(storage_path: Option<PathBuf>) -> Self {
        let seq = BUCKET_INSTANCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let staging_dir = std::env::temp_dir().join(format!(
            "mac-tui-fm-bucket-{}-{}",
            std::process::id(),
            seq
        ));
        let _ = fs::create_dir_all(&staging_dir);

        let mut bucket = Self {
            items: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            staging_dir,
            storage_path: storage_path.clone(),
        };

        if let Some(ref path) = storage_path {
            if let Ok(raw) = fs::read_to_string(path) {
                if let Ok(data) = serde_json::from_str::<BucketData>(&raw) {
                    for item in data.items {
                        if !bucket.items.iter().any(|p| p == &item) {
                            bucket.create_staging_link(&item);
                            bucket.items.push(item);
                        }
                    }
                }
            }
        }

        bucket
    }

    fn save(&self) {
        let Some(ref path) = self.storage_path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let data = BucketData {
            items: self.items.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = fs::write(path, json);
        }
    }

    pub fn staging_dir(&self) -> &Path {
        &self.staging_dir
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Adds a path to the bucket, deduplicating if already present.
    /// Returns true if newly added, false if already staged.
    pub fn add(&mut self, path: PathBuf) -> bool {
        let canonical = fs::canonicalize(&path).unwrap_or(path);
        if self.items.iter().any(|p| p == &canonical) {
            return false;
        }
        self.create_staging_link(&canonical);
        self.items.push(canonical);
        self.save();
        true
    }

    fn create_staging_link(&self, target: &Path) {
        let _ = fs::create_dir_all(&self.staging_dir);
        if let Some(name) = target.file_name() {
            let mut link_path = self.staging_dir.join(name);
            let mut counter = 1usize;
            while link_path.exists() || link_path.symlink_metadata().is_ok() {
                let stem = target
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("item");
                let ext = target.extension().and_then(|e| e.to_str());
                let alt = match ext {
                    Some(e) => format!("{}_{}.{}", stem, counter, e),
                    None => format!("{}_{}", stem, counter),
                };
                link_path = self.staging_dir.join(alt);
                counter += 1;
            }
            let _ = unix_fs::symlink(target, link_path);
        }
    }

    pub fn move_up(&mut self) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            self.cursor = self.items.len() - 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor + 1) % self.items.len();
    }

    /// Selects a 1-indexed item (1..=9).
    pub fn select_number(&mut self, num: usize) -> bool {
        if num >= 1 && num <= self.items.len() {
            self.cursor = num - 1;
            true
        } else {
            false
        }
    }

    /// Toggles multi-selection on the currently highlighted item.
    pub fn toggle_select(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if !self.selected.remove(&self.cursor) {
            self.selected.insert(self.cursor);
        }
    }

    /// Returns all multi-selected items, or the currently highlighted item if none are multi-selected.
    pub fn effective_targets(&self) -> Vec<PathBuf> {
        if !self.selected.is_empty() {
            let mut indices: Vec<usize> = self.selected.iter().copied().collect();
            indices.sort_unstable();
            indices
                .into_iter()
                .filter_map(|i| self.items.get(i).cloned())
                .collect()
        } else if let Some(item) = self.items.get(self.cursor) {
            vec![item.clone()]
        } else {
            Vec::new()
        }
    }

    /// Clears all items from the bucket, removes staged symlinks, and updates persisted state.
    pub fn archive(&mut self) {
        self.items.clear();
        self.selected.clear();
        self.cursor = 0;
        self.clean_staging_dir();
        self.save();
    }

    fn clean_staging_dir(&self) {
        if let Ok(entries) = fs::read_dir(&self.staging_dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

impl Default for Bucket {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Bucket {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_dedup_and_multiselect() {
        let mut bucket = Bucket::new();
        assert!(bucket.add(PathBuf::from("/tmp/a.txt")));
        assert!(!bucket.add(PathBuf::from("/tmp/a.txt")));
        assert!(bucket.add(PathBuf::from("/tmp/b.txt")));
        assert_eq!(bucket.len(), 2);

        // Default target is cursor (0)
        assert_eq!(bucket.effective_targets(), vec![PathBuf::from("/tmp/a.txt")]);

        // Toggle multi-select on 0 and 1
        bucket.toggle_select();
        bucket.move_down();
        bucket.toggle_select();
        assert_eq!(
            bucket.effective_targets(),
            vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]
        );

        bucket.archive();
        assert!(bucket.is_empty());
    }

    #[test]
    fn test_bucket_persists_across_sessions() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "mac-tui-fm-bucket-persist-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();
        let storage = tmp_dir.join("bucket.json");

        {
            let mut bucket = Bucket::load(storage.clone());
            assert!(bucket.add(PathBuf::from("/tmp/persist_a.txt")));
            assert!(bucket.add(PathBuf::from("/tmp/persist_b.txt")));
            assert_eq!(bucket.len(), 2);
        }

        // Reload in a new "session"
        {
            let mut reloaded = Bucket::load(storage.clone());
            assert_eq!(
                reloaded.items,
                vec![
                    PathBuf::from("/tmp/persist_a.txt"),
                    PathBuf::from("/tmp/persist_b.txt")
                ]
            );
            reloaded.archive();
            assert!(reloaded.is_empty());
        }

        // Reload after archive -> should remain empty
        {
            let after_archive = Bucket::load(storage.clone());
            assert!(after_archive.is_empty());
        }

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
