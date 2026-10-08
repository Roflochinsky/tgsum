//! Synthetic-only resource probe. Generation/compilation are outside measurement.
use std::{cell::Cell, fs, path::Path, time::Instant};
use tgsum_core::{
    local_package::PackageSettings,
    project::{ProjectChange, ProjectSource, ProjectStore},
    snapshot::SourceScope,
};

fn bytes(path: &Path) -> u64 {
    fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                bytes(&e.path())
            } else if e.file_type().unwrap().is_file() {
                e.metadata().unwrap().len()
            } else {
                0
            }
        })
        .sum()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty()
        || args.len() > 2
        || args.get(1).is_some_and(|s| s != "cancel" && s != "multi")
    {
        return Err(
            "usage: local_package_probe <synthetic-export-directory> [cancel|multi] (chat ID 1; multi IDs 1..20)".into(),
        );
    }
    let root = tempfile::tempdir()?;
    let output = root.path().join("output");
    fs::create_dir(&output)?;
    let store = ProjectStore::new(root.path().join("private"));
    let mut project = store.create("Synthetic resource qualification")?;
    let chats = if args.get(1).is_some_and(|s| s == "multi") {
        20
    } else {
        1
    };
    for chat in 1..=chats {
        project = store.update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: format!("synthetic-{chat}"),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("synthetic", chat.to_string()),
                archive_path: None,
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )?;
    }
    store.configure_local_package(
        &project.project_id,
        project.revision,
        PackageSettings {
            source_ids: project
                .sources
                .iter()
                .map(|s| s.source_id.clone())
                .collect(),
            input_directory: Path::new(&args[0]).canonicalize()?,
            output_directory: output.clone(),
            automatic: false,
            include_images: true,
            include_office: true,
            github_repository: None,
            cloud_processing: false,
        },
    )?;
    let start = Instant::now();
    let calls = Cell::new(0);
    let cancel = args.get(1).is_some_and(|s| s == "cancel");
    let state = store.refresh_local_package(&project.project_id, 1, || {
        calls.set(calls.get() + 1);
        cancel && calls.get() > 20
    })?;
    let elapsed = start.elapsed().as_millis();
    let peak = fs::read_to_string("/proc/self/status").ok().and_then(|s| {
        s.lines()
            .find(|l| l.starts_with("VmHWM:"))
            .map(str::to_owned)
    });
    println!(
        "{}",
        serde_json::json!({"phase":state.phase,"elapsed_ms":elapsed,
        "peak":peak,"private_bytes":bytes(&root.path().join("private")),
        "output_bytes":bytes(&output),"messages":state.ready.as_ref().map(|r|r.messages),
        "files":state.ready.as_ref().map(|r|r.files),"conversations":state.ready.as_ref().map(|r|r.conversations),"cancel_checks":calls.get()})
    );
    if state.phase != if cancel { "cancelled" } else { "ready" } {
        return Err(state.message.into());
    }
    Ok(())
}
