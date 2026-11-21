use std::collections::HashSet;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Bucket {
    pub items: Vec<PathBuf>,
    pub cursor: usize,
    pub selected: HashSet<usize>,
    staging_dir: PathBuf,
}

impl Bucket {
    pub fn new() -> Self {
        let staging_dir = std::env::temp_dir().join(format!("mac-tui-fm-bucket-{}", std::process::id()));
        let _ = fs::create_dir_all(&staging_dir);
        Self {
            items: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            staging_dir,
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

    /// Clears all items from the bucket and removes staged symlinks.
    pub fn archive(&mut self) {
        self.items.clear();
        self.selected.clear();
        self.cursor = 0;
        self.clean_staging_dir();
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
}
