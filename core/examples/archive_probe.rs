//! Measure an existing synthetic archive without including generation/build
//! cost. Invoke under an OS memory profiler; see docs/development/archive-core.md.

use std::fs::File;
use std::time::Instant;

use tgsum_core::snapshot::{SnapshotStore, SourceScope};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: archive_probe index|snapshot <synthetic-export.json> (chat ID 1)".into(),
        );
    }
    let start = Instant::now();
    let file = File::open(&args[1])?;
    if args[0] == "index" {
        let chats = tgsum_core::index_reader(file)?;
        println!(
            "{} chats, {} messages",
            chats.len(),
            chats.iter().map(|c| c.count).sum::<usize>()
        );
    } else if args[0] == "snapshot" {
        let directory = tempfile::tempdir()?;
        let store = SnapshotStore::new(directory.path());
        let snapshot = store.import_telegram(
            "measurement",
            &SourceScope::telegram("synthetic", "1"),
            file,
        )?;
        println!("{} messages", snapshot.messages.len());
    } else {
        return Err("mode must be index or snapshot".into());
    }
    eprintln!("elapsed_ms={}", start.elapsed().as_millis());
    // Linux reports the process's actual high-water mark, excluding any parent
    // interpreter or compiler. Other OSes can use their native process profiler.
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(peak) = status.lines().find(|line| line.starts_with("VmHWM:")) {
            eprintln!("{peak}");
        }
    }
    Ok(())
}
