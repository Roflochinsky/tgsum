use serde_json::json;
use std::io::Cursor;
use tgsum_core::{
    bundle::BundleOptions,
    project::{ProjectChange, ProjectSource, ProjectStore},
    snapshot::SourceScope,
};

#[test]
fn saved_privacy_profile_survives_restart_and_applies_exact_optional_exclusions() {
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Privacy fixture").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    store.snapshots(&project.project_id).unwrap().import_telegram("initial", &scope, Cursor::new(br#"{
        "id":1,"messages":[{"id":1,"from":"Alice","from_id":"user1","text":"Alice alice@example.test bob@example.test db.local 10.0.0.1 ACME password=SYNTHETIC_SECRET"}]
    }"#)).unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "work".into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: None,
                latest_snapshot_id: Some("initial".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    let before = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    let policy = json!({"redact_candidates":false,
        "pii":{"categories":["participants","emails"]},
        "infrastructure":{"categories":["ip","host"],"hostnames":["private-unmentioned-host"],"internal_domains":[]},
        "keep_values":["Alice","alice@example.test","10.0.0.1","ACME","SYNTHETIC_SECRET"]
    });
    let change: ProjectChange = serde_json::from_value(json!({"kind":"privacy","value":{
        "preset":"custom","options":policy,"custom_terms":{"entries":[{"value":"ACME"}]}
    }}))
    .unwrap();
    assert!(!format!("{change:?}").contains("private-unmentioned-host"));
    let project = store
        .update(&project.project_id, project.revision, change)
        .unwrap();
    let store = ProjectStore::new(root.path());
    let reopened = store.open(&project.project_id).unwrap();
    assert_eq!(reopened.settings.privacy_preset, "custom");
    assert_eq!(
        serde_json::to_value(&reopened).unwrap()["privacy_options"],
        policy
    );
    assert!(!format!("{reopened:?}").contains("SYNTHETIC_SECRET"));
    assert!(store
        .export_bundle(
            &project.project_id,
            &before.bundle_id,
            before.project_revision,
            output.path(),
            || false
        )
        .is_err());
    let review = store
        .prepare_saved_bundle(&project.project_id, project.revision, || false)
        .unwrap();
    assert!(review.preview.contains("Sender: \"Alice\""));
    assert!(review.preview.contains("> Alice alice@example.test EMAIL_"));
    assert!(review
        .preview
        .contains("HOST_0001 10.0.0.1 TERM_0001 password=[REDACTED_SECRET]"));
    assert_eq!(review.manifest.privacy.redacted, 1);
    let public = serde_json::to_string(&review.manifest).unwrap();
    assert!(!public.contains("keep_values"));
    assert!(!public.contains("private-unmentioned-host"));
    store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            output.path(),
            || false,
        )
        .unwrap();
}

#[test]
fn preset_changes_are_atomic_validated_and_old_projects_keep_default_options() {
    use tgsum_core::privacy::{PrivacyPreset, PrivacyProfile};
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let mut project = store.create("Preset fixture").unwrap();
    let old_path = root
        .path()
        .join(&project.project_id)
        .join("revisions/00000000000000000000.json");
    let mut legacy = serde_json::to_value(&project).unwrap();
    legacy["schema_version"] = 6.into();
    let old_bytes = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(&old_path, &old_bytes).unwrap();
    assert!(store
        .open(&project.project_id)
        .unwrap()
        .privacy_options
        .is_default());
    assert_eq!(std::fs::read(&old_path).unwrap(), old_bytes);
    for (preset, pii, infrastructure) in [
        (PrivacyPreset::People, 4, 0),
        (PrivacyPreset::Work, 4, 7),
        (PrivacyPreset::Secrets, 0, 0),
    ] {
        let expected = project.revision + 1;
        project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Privacy(PrivacyProfile {
                    preset,
                    options: preset.options(),
                    custom_terms: Default::default(),
                }),
            )
            .unwrap();
        assert_eq!(project.revision, expected);
        assert_eq!(project.privacy_options.pii.categories.len(), pii);
        assert_eq!(
            project.privacy_options.infrastructure.categories.len(),
            infrastructure
        );
        assert_eq!(project.settings.privacy_preset, preset.id());
    }
    let invalid = PrivacyProfile {
        preset: PrivacyPreset::Secrets,
        options: PrivacyPreset::Work.options(),
        custom_terms: Default::default(),
    };
    assert!(store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Privacy(invalid)
        )
        .is_err());
    for keeps in [
        json!(["duplicate", "duplicate"]),
        json!([" private "]),
        json!(["line\nbreak"]),
        json!(["a".repeat(4097)]),
        json!(vec!["x"; 257]),
    ] {
        let result = serde_json::from_value::<ProjectChange>(
            json!({"kind":"privacy","value":{"preset":"custom","options":{"keep_values":keeps}}}),
        );
        assert_eq!(result.unwrap_err().to_string(), "invalid privacy profile");
    }
    assert_eq!(store.open(&project.project_id).unwrap(), project);
    assert_eq!(std::fs::read(old_path).unwrap(), old_bytes);
}

#[test]
fn exact_exceptions_protect_longer_aliases_without_disabling_other_values_or_review() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Exceptions").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    store.snapshots(&project.project_id).unwrap().import_telegram("initial", &scope, Cursor::new(br#"{"id":1,"messages":[
        {"id":1,"from":"Alice Smith","from_id":"user1","text":"Alice Smith Alice db.local DB.LOCAL key=REVIEW_VALUE"},
        {"id":2,"from":"Alice","from_id":"user2","text":"other"}
    ]}"#)).unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "work".into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: None,
                latest_snapshot_id: Some("initial".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    let options = serde_json::from_value(json!({"pii":{"categories":["participants"]},"infrastructure":{"categories":["host"]},"keep_values":["Alice Smith","db.local","REVIEW_VALUE"]})).unwrap();
    let review = store
        .prepare_bundle(&project.project_id, project.revision, options, || false)
        .unwrap();
    assert!(review
        .preview
        .contains("> Alice Smith PERSON_0002 db.local HOST_0001 key=REVIEW_VALUE"));
    assert!(review.preview.contains("Sender: \"Alice Smith\""));
    assert!(review.preview.contains("Sender: \"PERSON_0002\""));
    assert_eq!(review.manifest.privacy.needs_review, 1);
    assert_eq!(review.manifest.pii.unwrap().replacements, 2);
    assert_eq!(review.manifest.infrastructure.unwrap().replacements, 1);
}
