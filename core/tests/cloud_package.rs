#![cfg(unix)]
use serde_json::{json, Value};
use std::fs;
use tgsum_core::{
    local_package::PackageSettings,
    project::{ProjectChange, ProjectSource, ProjectStore},
    snapshot::SourceScope,
};

#[test]
fn structured_topics_are_selected_and_hidden_dates_have_no_numeric_bypass() {
    use tgsum_core::custom_terms::{CustomTerm, CustomTerms, TermBoundary};
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input");
    fs::create_dir(&input).unwrap();
    fs::write(
        input.join("result.json"),
        serde_json::to_vec(&json!({"id":7,"messages":[
            {"id":100,"type":"service","action":"topic_created","title":"Selected"},
            {"id":101,"date":"2026-10-08T10:00:00","reply_to_message_id":100,"text":"Selected 🌍"},
            {"id":200,"type":"service","action":"topic_created","title":"Excluded"},
            {"id":201,"reply_to_message_id":200,"text":"EXCLUDED_TOPIC_SENTINEL"}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    let store = ProjectStore::new(root.path().join("private"));
    let mut p = store.create("Topic fixture").unwrap();
    let mut source = ProjectSource {
        source_id: "selected".into(),
        connector_id: "telegram_json".into(),
        scope: SourceScope::telegram("fixture", "7"),
        archive_path: Some(input.join("result.json")),
        latest_snapshot_id: None,
        selection: Default::default(),
    };
    source.selection.filter.topic_ids = Some(vec!["100".into()]);
    source.selection.filter.include_service = true;
    p = store
        .update(&p.project_id, p.revision, ProjectChange::Source(source))
        .unwrap();
    p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::CustomTerms(CustomTerms {
                entries: vec![CustomTerm {
                    value: "2026-10-08".into(),
                    boundary: TermBoundary::Substring,
                }],
            }),
        )
        .unwrap();
    store
        .configure_local_package(
            &p.project_id,
            p.revision,
            PackageSettings {
                source_ids: vec!["selected".into()],
                input_directory: input,
                output_directory: output,
                automatic: false,
                include_images: false,
                include_office: false,
                github_repository: Some("fixture/private".into()),
                cloud_processing: true,
            },
        )
        .unwrap();
    let state = store
        .refresh_local_package(&p.project_id, 1, || false)
        .unwrap();
    assert_eq!(state.phase, "ready", "{}", state.message);
    let raw = fs::read_to_string(state.ready.unwrap().directory.join("messages.jsonl")).unwrap();
    assert!(!raw.contains("EXCLUDED_TOPIC_SENTINEL"));
    assert!(!raw.contains("2026-10-08"));
    let rows: Vec<Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row["topic_known"] == true && row["topic"].is_string()));
    assert_eq!(rows[0]["topic"], rows[1]["topic"]);
    assert!(rows.iter().all(|row| row.get("timestamp_utc").is_none()));
    assert_eq!(rows[1]["reply"], rows[0]["evidence"]);
}

#[test]
fn structured_handoff_is_opt_in_selected_private_and_stable_across_edits() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input");
    let output = root.path().join("output");
    fs::create_dir(&input).unwrap();
    fs::create_dir(&output).unwrap();
    let store = ProjectStore::new(root.path().join("private"));
    let p = store.create("Synthetic cloud").unwrap();
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "selected".into(),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("fixture", "9007199254740993"),
                archive_path: Some(input.join("result.json")),
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    let write = |text: &str| {
        fs::write(input.join("result.json"), serde_json::to_vec(&json!({"chats":{"list":[
        {"id":9007199254740993u64,"messages":[{"id":9007199254740994u64,"from":"Иванов Иван Иванович","from_id":"user9007199254740995","date":"2026-10-08T10:00:00","text":text}]},
        {"id":42,"messages":[{"id":1,"text":"FOREIGN_CHAT_SENTINEL"}]}
    ]}})).unwrap()).unwrap()
    };
    write("Привет 😀 password=CLOUD_SECRET_SENTINEL");
    let settings = PackageSettings {
        source_ids: vec!["selected".into()],
        input_directory: input.clone(),
        output_directory: output.clone(),
        automatic: false,
        include_images: false,
        include_office: false,
        github_repository: Some("fixture/private".into()),
        cloud_processing: false,
    };
    store
        .configure_local_package(&p.project_id, p.revision, settings.clone())
        .unwrap();
    let ready = store
        .refresh_local_package(&p.project_id, 1, || false)
        .unwrap()
        .ready
        .unwrap();
    assert!(!ready.directory.join("messages.jsonl").exists());
    let current = store.open(&p.project_id).unwrap();
    let mut cloud = settings.clone();
    cloud.cloud_processing = true;
    store
        .configure_local_package(&p.project_id, current.revision, cloud.clone())
        .unwrap();
    assert!(
        !store
            .local_package_allows_publication(&p.project_id)
            .unwrap(),
        "old Markdown receipt must not authorize cloud input"
    );
    let state = store
        .refresh_local_package(&p.project_id, 2, || false)
        .unwrap();
    assert_eq!(state.phase, "ready", "{}", state.message);
    let ready = state.ready.unwrap();
    assert!(store
        .local_package_allows_publication(&p.project_id)
        .unwrap());
    let raw = fs::read_to_string(ready.directory.join("messages.jsonl")).unwrap();
    for secret in [
        "CLOUD_SECRET_SENTINEL",
        "FOREIGN_CHAT_SENTINEL",
        "9007199254740993",
        "9007199254740994",
        "user9007199254740995",
        "Иванов Иван Иванович",
    ] {
        assert!(!raw.contains(secret), "outbound leak: {secret}");
    }
    assert!(raw.contains("Привет 😀"));
    let row: Value = serde_json::from_str(raw.trim()).unwrap();
    assert!(row["topic"].is_null());
    assert_eq!(row["topic_known"], false);
    write("Обновлено 😀 password=CLOUD_SECRET_SENTINEL");
    let changed = store
        .refresh_local_package(&p.project_id, 3, || false)
        .unwrap();
    assert_eq!(changed.phase, "ready", "{}", changed.message);
    let changed = changed.ready.unwrap();
    let newer: Value = serde_json::from_str(
        fs::read_to_string(changed.directory.join("messages.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(newer["evidence"]["id"], row["evidence"]["id"]);
    assert_ne!(newer["evidence"]["revision"], row["evidence"]["revision"]);
    assert_eq!(newer["sender_key"], row["sender_key"]);
    assert_eq!(
        store
            .refresh_local_package(&p.project_id, 4, || false)
            .unwrap()
            .ready
            .unwrap()
            .content_sha256,
        changed.content_sha256
    );
    // A changed privacy profile followed by Save MUST NOT reauthorize an older
    // prepared package under the new scope hash.
    let current = store.open(&p.project_id).unwrap();
    let mut profile = tgsum_core::privacy::PrivacyProfile {
        preset: tgsum_core::privacy::PrivacyPreset::Custom,
        options: current.privacy_options.clone(),
        custom_terms: current.custom_terms.clone(),
    };
    profile.options.redact_candidates = !profile.options.redact_candidates;
    let current = store
        .update(
            &p.project_id,
            current.revision,
            ProjectChange::Privacy(profile),
        )
        .unwrap();
    store
        .configure_local_package(&p.project_id, current.revision, cloud.clone())
        .unwrap();
    assert!(!store
        .local_package_allows_publication(&p.project_id)
        .unwrap());
    let rebuilt = store
        .refresh_local_package(&p.project_id, 5, || false)
        .unwrap();
    assert_eq!(rebuilt.phase, "ready", "{}", rebuilt.message);
    assert!(store
        .local_package_allows_publication(&p.project_id)
        .unwrap());
    store
        .with_local_package_publication(&p.project_id, || {
            let current = store.open(&p.project_id)?;
            assert!(store
                .update(
                    &p.project_id,
                    current.revision,
                    ProjectChange::Rename("Blocked during push".into())
                )
                .is_err());
            assert!(store
                .configure_local_package(&p.project_id, current.revision, cloud.clone())
                .is_err());
            Ok(())
        })
        .unwrap();
    let current = store.open(&p.project_id).unwrap();
    store
        .configure_local_package(&p.project_id, current.revision, settings)
        .unwrap();
    assert!(
        !store
            .local_package_allows_publication(&p.project_id)
            .unwrap(),
        "changing handoff invalidates old structured receipt"
    );
}
