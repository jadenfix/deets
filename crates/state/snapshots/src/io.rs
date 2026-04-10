use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::generator::{generate_snapshot, StateSnapshot};
use crate::importer::import_snapshot;
use aether_state_storage::Storage;

/// Name format: `snapshot_{height:010}.bin.zst`
fn snapshot_filename(height: u64) -> String {
    format!("snapshot_{height:010}.bin.zst")
}

/// Export a snapshot at the given height to disk.
///
/// The file is written atomically (write to temp, then rename) to prevent
/// partial files from being visible if the process crashes mid-write.
pub fn export_snapshot_to_file(
    storage: &Storage,
    height: u64,
    snapshot_dir: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(snapshot_dir)
        .with_context(|| format!("failed to create snapshot dir: {}", snapshot_dir.display()))?;

    let compressed = generate_snapshot(storage, height)?;

    let dest = snapshot_dir.join(snapshot_filename(height));
    let tmp = snapshot_dir.join(format!(".tmp_snapshot_{height}.bin.zst"));

    fs::write(&tmp, &compressed)
        .with_context(|| format!("failed to write temp snapshot: {}", tmp.display()))?;
    fs::rename(&tmp, &dest).with_context(|| {
        format!(
            "failed to rename snapshot: {} -> {}",
            tmp.display(),
            dest.display()
        )
    })?;

    Ok(dest)
}

/// Import a snapshot from a file on disk into storage.
pub fn import_snapshot_from_file(storage: &Storage, snapshot_path: &Path) -> Result<StateSnapshot> {
    if !snapshot_path.exists() {
        bail!("snapshot file not found: {}", snapshot_path.display());
    }
    let bytes = fs::read(snapshot_path)
        .with_context(|| format!("failed to read snapshot: {}", snapshot_path.display()))?;
    import_snapshot(storage, &bytes)
}

/// Retain only the most recent `keep` snapshots in the directory, deleting older ones.
///
/// Snapshots are identified by the `snapshot_*.bin.zst` naming convention and
/// sorted lexicographically (which equals chronological order due to zero-padded heights).
pub fn prune_old_snapshots(snapshot_dir: &Path, keep: usize) -> Result<usize> {
    if !snapshot_dir.exists() {
        return Ok(0);
    }

    let mut snapshot_files: Vec<PathBuf> = fs::read_dir(snapshot_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("snapshot_")
                && name.ends_with(".bin.zst")
                && !name.starts_with(".tmp_")
            {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();

    snapshot_files.sort();

    let mut deleted = 0;
    if snapshot_files.len() > keep {
        let to_remove = snapshot_files.len() - keep;
        for path in &snapshot_files[..to_remove] {
            fs::remove_file(path)
                .with_context(|| format!("failed to delete old snapshot: {}", path.display()))?;
            deleted += 1;
        }
    }

    Ok(deleted)
}

/// List available snapshot heights in the directory, sorted ascending.
pub fn list_snapshots(snapshot_dir: &Path) -> Result<Vec<u64>> {
    if !snapshot_dir.exists() {
        return Ok(vec![]);
    }

    let mut heights: Vec<u64> = fs::read_dir(snapshot_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            // Parse height from "snapshot_0000000100.bin.zst"
            let stem = name.strip_prefix("snapshot_")?.strip_suffix(".bin.zst")?;
            stem.parse::<u64>().ok()
        })
        .collect();

    heights.sort();
    Ok(heights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_merkle::SparseMerkleTree;
    use aether_state_storage::{Storage, CF_METADATA};
    use tempfile::TempDir;

    fn seeded_storage(dir: &Path) -> Storage {
        let storage = Storage::open(dir).unwrap();
        // Use the correct empty-tree Merkle root so import verification passes
        let empty_root = SparseMerkleTree::new().root();
        storage
            .put(CF_METADATA, b"state_root", empty_root.as_bytes())
            .unwrap();
        storage
    }

    #[test]
    fn export_and_import_roundtrip() {
        let db_dir = TempDir::new().unwrap();
        let snap_dir = TempDir::new().unwrap();
        let storage = seeded_storage(db_dir.path());

        let path = export_snapshot_to_file(&storage, 100, snap_dir.path()).unwrap();
        assert!(path.exists());
        assert!(path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("0000000100"));

        // Import into fresh storage
        let db2_dir = TempDir::new().unwrap();
        let storage2 = Storage::open(db2_dir.path()).unwrap();
        let snapshot = import_snapshot_from_file(&storage2, &path).unwrap();
        assert_eq!(snapshot.metadata.height, 100);
    }

    #[test]
    fn list_snapshots_works() {
        let db_dir = TempDir::new().unwrap();
        let snap_dir = TempDir::new().unwrap();
        let storage = seeded_storage(db_dir.path());

        export_snapshot_to_file(&storage, 50, snap_dir.path()).unwrap();
        export_snapshot_to_file(&storage, 100, snap_dir.path()).unwrap();
        export_snapshot_to_file(&storage, 200, snap_dir.path()).unwrap();

        let heights = list_snapshots(snap_dir.path()).unwrap();
        assert_eq!(heights, vec![50, 100, 200]);
    }

    #[test]
    fn prune_keeps_recent() {
        let db_dir = TempDir::new().unwrap();
        let snap_dir = TempDir::new().unwrap();
        let storage = seeded_storage(db_dir.path());

        for h in [10, 20, 30, 40, 50] {
            export_snapshot_to_file(&storage, h, snap_dir.path()).unwrap();
        }

        let deleted = prune_old_snapshots(snap_dir.path(), 2).unwrap();
        assert_eq!(deleted, 3);

        let remaining = list_snapshots(snap_dir.path()).unwrap();
        assert_eq!(remaining, vec![40, 50]);
    }

    #[test]
    fn prune_noop_when_under_limit() {
        let db_dir = TempDir::new().unwrap();
        let snap_dir = TempDir::new().unwrap();
        let storage = seeded_storage(db_dir.path());

        export_snapshot_to_file(&storage, 10, snap_dir.path()).unwrap();
        let deleted = prune_old_snapshots(snap_dir.path(), 5).unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn import_missing_file_errors() {
        let db_dir = TempDir::new().unwrap();
        let storage = Storage::open(db_dir.path()).unwrap();
        let result =
            import_snapshot_from_file(&storage, Path::new("/nonexistent/snapshot.bin.zst"));
        assert!(result.is_err());
    }

    #[test]
    fn list_empty_dir() {
        let heights = list_snapshots(Path::new("/nonexistent")).unwrap();
        assert!(heights.is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_state_merkle::SparseMerkleTree;
    use aether_state_storage::{Storage, CF_METADATA};
    use proptest::prelude::*;
    use std::collections::BTreeSet;
    use tempfile::TempDir;

    fn seeded_storage(dir: &Path) -> Storage {
        let storage = Storage::open(dir).unwrap();
        let empty_root = SparseMerkleTree::new().root();
        storage
            .put(CF_METADATA, b"state_root", empty_root.as_bytes())
            .unwrap();
        storage
    }

    proptest! {
        /// list_snapshots returns heights in sorted ascending order.
        #[test]
        fn list_returns_sorted(heights in prop::collection::vec(0u64..100_000, 1..10)) {
            let db_dir = TempDir::new().unwrap();
            let snap_dir = TempDir::new().unwrap();
            let storage = seeded_storage(db_dir.path());

            let unique: BTreeSet<u64> = heights.into_iter().collect();
            for &h in &unique {
                export_snapshot_to_file(&storage, h, snap_dir.path()).unwrap();
            }

            let listed = list_snapshots(snap_dir.path()).unwrap();
            prop_assert_eq!(listed.len(), unique.len());
            // Must be sorted
            for w in listed.windows(2) {
                prop_assert!(w[0] < w[1]);
            }
        }

        /// Pruning with keep=N retains exactly min(N, total) snapshots.
        #[test]
        fn prune_retains_correct_count(
            heights in prop::collection::vec(0u64..100_000, 1..8),
            keep in 0usize..10,
        ) {
            let db_dir = TempDir::new().unwrap();
            let snap_dir = TempDir::new().unwrap();
            let storage = seeded_storage(db_dir.path());

            let unique: BTreeSet<u64> = heights.into_iter().collect();
            for &h in &unique {
                export_snapshot_to_file(&storage, h, snap_dir.path()).unwrap();
            }

            let total = unique.len();
            let deleted = prune_old_snapshots(snap_dir.path(), keep).unwrap();
            let remaining = list_snapshots(snap_dir.path()).unwrap();

            let expected_remaining = total.min(keep);
            prop_assert_eq!(remaining.len(), expected_remaining);
            prop_assert_eq!(deleted, total - expected_remaining);

            // Remaining should be the highest heights
            let all_sorted: Vec<u64> = unique.into_iter().collect();
            if expected_remaining > 0 {
                let expected_kept = &all_sorted[all_sorted.len() - expected_remaining..];
                prop_assert_eq!(remaining, expected_kept);
            }
        }

        /// Export then import roundtrip preserves snapshot height.
        #[test]
        fn export_import_preserves_height(height in 0u64..1_000_000) {
            let db_dir = TempDir::new().unwrap();
            let snap_dir = TempDir::new().unwrap();
            let storage = seeded_storage(db_dir.path());

            let path = export_snapshot_to_file(&storage, height, snap_dir.path()).unwrap();

            let db2_dir = TempDir::new().unwrap();
            let storage2 = Storage::open(db2_dir.path()).unwrap();
            let snapshot = import_snapshot_from_file(&storage2, &path).unwrap();
            prop_assert_eq!(snapshot.metadata.height, height);
        }

        /// Snapshot filename contains zero-padded height.
        #[test]
        fn filename_contains_height(height in 0u64..1_000_000) {
            let db_dir = TempDir::new().unwrap();
            let snap_dir = TempDir::new().unwrap();
            let storage = seeded_storage(db_dir.path());

            let path = export_snapshot_to_file(&storage, height, snap_dir.path()).unwrap();
            let name = path.file_name().unwrap().to_str().unwrap();
            prop_assert!(name.starts_with("snapshot_"));
            prop_assert!(name.ends_with(".bin.zst"));
            // Parse height back from filename
            let stem = name.strip_prefix("snapshot_").unwrap().strip_suffix(".bin.zst").unwrap();
            let parsed: u64 = stem.parse().unwrap();
            prop_assert_eq!(parsed, height);
        }
    }
}
