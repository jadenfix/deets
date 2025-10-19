use std::fs;

use crate::config::{expand_path, ResolvedConfig};
use aether_state_snapshots::{generate_snapshot, import_snapshot};
use aether_state_storage::Storage;
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
pub struct SnapshotsCommand {
    #[command(subcommand)]
    command: SnapshotSubcommand,
}

#[derive(Subcommand, Debug)]
enum SnapshotSubcommand {
    /// Create a new snapshot from the local ledger state
    Create {
        /// Slot number to label the snapshot with
        #[arg(long)]
        slot: Option<u64>,

        /// Output snapshot file path
        #[arg(long, default_value = "snapshot.bin")]
        output: String,

        /// Ledger data directory (default ~/.aether/data)
        #[arg(long, default_value = "~/.aether/data")]
        data_dir: String,
    },
    /// List snapshot files in a directory
    List {
        /// Directory containing snapshot files
        #[arg(long, default_value = "~/.aether/data")]
        data_dir: String,
    },
    /// Restore the local ledger state from a snapshot
    Restore {
        /// Snapshot file path
        #[arg(long)]
        input: String,

        /// Ledger data directory
        #[arg(long, default_value = "~/.aether/data")]
        data_dir: String,
    },
}

impl SnapshotsCommand {
    pub async fn execute(&self, _config: &ResolvedConfig) -> Result<()> {
        match &self.command {
            SnapshotSubcommand::Create {
                slot,
                output,
                data_dir,
            } => self.create_snapshot(*slot, output, data_dir).await,
            SnapshotSubcommand::List { data_dir } => self.list_snapshots(data_dir).await,
            SnapshotSubcommand::Restore { input, data_dir } => {
                self.restore_snapshot(input, data_dir).await
            }
        }
    }

    async fn create_snapshot(&self, slot: Option<u64>, output: &str, data_dir: &str) -> Result<()> {
        let storage = open_storage(data_dir)?;
        let height = slot.unwrap_or(0);
        let bytes = generate_snapshot(&storage, height)
            .with_context(|| "failed to generate snapshot from storage")?;

        let output_path = expand_path(output)?;
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create snapshot directory {}", parent.display())
            })?;
        }
        fs::write(&output_path, &bytes)
            .with_context(|| format!("failed to write snapshot file {}", output_path.display()))?;

        println!(
            "Snapshot written to {} ({} bytes, slot {})",
            output_path.display(),
            bytes.len(),
            height
        );
        Ok(())
    }

    async fn list_snapshots(&self, data_dir: &str) -> Result<()> {
        let base = expand_path(data_dir)?;
        let dir = if base.is_dir() {
            base
        } else {
            return Err(anyhow!(
                "{} is not a directory; pass --data-dir pointing to snapshot directory",
                base.display()
            ));
        };

        let mut entries: Vec<_> = fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .collect();

        entries.sort_by_key(|entry| entry.path());

        if entries.is_empty() {
            println!("No snapshot files found in {}", dir.display());
            return Ok(());
        }

        println!("Snapshots in {}:", dir.display());
        for entry in entries {
            let path = entry.path();
            let metadata = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|ts| ts.elapsed().ok())
                .map(|elapsed| format!("{} seconds ago", elapsed.as_secs()))
                .unwrap_or_else(|| "unknown age".to_string());
            let size = entry
                .metadata()
                .ok()
                .map(|m| format!("{} bytes", m.len()))
                .unwrap_or_else(|| "unknown size".to_string());
            println!("  {} ({}; {})", path.display(), size, metadata);
        }

        Ok(())
    }

    async fn restore_snapshot(&self, input: &str, data_dir: &str) -> Result<()> {
        let snapshot_path = expand_path(input)?;
        let bytes = fs::read(&snapshot_path)
            .with_context(|| format!("failed to read snapshot file {}", snapshot_path.display()))?;

        let storage = open_storage(data_dir)?;
        let snapshot = import_snapshot(&storage, &bytes)
            .with_context(|| "failed to import snapshot into storage")?;

        println!(
            "Restored snapshot height {} ({} accounts, {} utxos)",
            snapshot.metadata.height,
            snapshot.accounts.len(),
            snapshot.utxos.len()
        );
        Ok(())
    }
}

fn open_storage(data_dir: &str) -> Result<Storage> {
    let dir = expand_path(data_dir)?;
    let ledger_path = dir.join("ledger");
    fs::create_dir_all(&ledger_path).with_context(|| {
        format!(
            "failed to ensure ledger directory {}",
            ledger_path.display()
        )
    })?;
    Storage::open(&ledger_path)
        .with_context(|| format!("failed to open ledger storage at {}", ledger_path.display()))
}
