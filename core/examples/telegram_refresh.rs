//! Reproducible offline Bridge simulation using two synthetic exports.
//! Run with `cargo run -p tgsum-core --example telegram_refresh -- /new/store`.
//! No messenger process, account, API, or network is used.

use std::fs;
use std::io;
use std::path::PathBuf;

use tgsum_core::bridge::{ClientStatus, ClientUpdate, ExportJob, ExportRequest};
use tgsum_core::snapshot::{SnapshotStore, SourceScope};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let directory = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: telegram_refresh <snapshot-directory>; uses synthetic fixtures only")?;
    if args.next().is_some() {
        return Err("expected one snapshot directory".into());
    }
    let store = SnapshotStore::new(directory);
    let client_output = tempfile::tempdir()?;
    let source = SourceScope::telegram("synthetic-account", "9007199254740993");

    for (run_id, bytes) in [
        (
            "before",
            include_bytes!("../tests/fixtures/telegram-single-before.json").as_slice(),
        ),
        (
            "after",
            include_bytes!("../tests/fixtures/telegram-single-after.json").as_slice(),
        ),
    ] {
        let request = ExportRequest {
            run_id: run_id.into(),
            source: source.clone(),
            archive_path: client_output.path().join(format!("{run_id}.json")),
        };
        let mut job = ExportJob::new(request.clone())?;
        job.apply(
            ClientUpdate {
                request: request.clone(),
                status: ClientStatus::Exporting,
            },
            &store,
        )?;
        // Simulated client finishes writing before announcing completion.
        fs::write(&request.archive_path, bytes)?;
        job.apply(
            ClientUpdate {
                request,
                status: ClientStatus::Completed,
            },
            &store,
        )?;
    }
    let diff = store.load("after")?.diff(&store.load("before")?)?;
    serde_json::to_writer_pretty(io::stdout().lock(), &diff)?;
    println!();
    eprintln!(
        "Synthetic snapshots saved in {}",
        store.directory().display()
    );
    Ok(())
}
