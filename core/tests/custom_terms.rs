use serde_json::json;
use tgsum_core::project::{ProjectChange, ProjectStore};

fn fixture(
    text: &str,
    entries: serde_json::Value,
) -> (
    tempfile::TempDir,
    ProjectStore,
    tgsum_core::project::Project,
) {
    use std::io::Cursor;
    use tgsum_core::project::ProjectSource;
    use tgsum_core::snapshot::SourceScope;
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("ACME workspace").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    store.snapshots(&project.project_id).unwrap().import_telegram("initial",&scope,Cursor::new(serde_json::to_vec(&json!({"id":1,"name":"ACME chat","messages":[{"id":1,"from":"ACME","from_id":"user1","text":text}]})).unwrap())).unwrap();
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
    let project = store
        .update(
            &project.project_id,
            project.revision,
            serde_json::from_value(json!({"kind":"custom_terms","value":{"entries":entries}}))
                .unwrap(),
        )
        .unwrap();
    (root, store, project)
}

#[test]
fn project_dictionary_survives_restart_without_exposing_values_in_debug_or_errors() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Terms fixture").unwrap();
    let policy = json!({"entries":[{"value":"PRIVATE_ACME","boundary":"word"}]});
    let change: ProjectChange =
        serde_json::from_value(json!({"kind":"custom_terms","value":policy})).unwrap();
    assert!(!format!("{change:?}").contains("PRIVATE_ACME"));
    let updated = store
        .update(&project.project_id, project.revision, change)
        .unwrap();
    let reopened = ProjectStore::new(root.path())
        .open(&project.project_id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&reopened).unwrap()["custom_terms"],
        policy
    );
    assert_eq!(reopened.revision, updated.revision);
    assert!(!format!("{reopened:?}").contains("PRIVATE_ACME"));
    let invalid=serde_json::from_value::<ProjectChange>(json!({"kind":"custom_terms","value":{"entries":[{"value":"PRIVATE_ACME","boundary":"PRIVATE_BAD_MODE"}]}})).unwrap_err();
    assert!(!invalid.to_string().contains("PRIVATE_"));
}

#[test]
fn saved_dictionary_applies_to_selected_text_and_metadata_with_longest_valid_matches() {
    use tgsum_core::bundle::BundleOptions;
    let (root, store, project) = fixture(
        "ACME Portal and ACME ACMEish /srv/ACME/repo",
        json!([{"value":"ACME"},{"value":"ACME Portal"}]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.project_title, "TERM_0001 workspace");
    assert!(review
        .preview
        .contains("> TERM_0002 and TERM_0001 ACMEish /srv/TERM_0001/repo"));
    assert!(review.preview.contains("Sender: \"TERM_0001\""));
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["custom_terms"],
        json!({"rules_version":"terms/1","replacements":6})
    );
    assert_eq!(review.project_revision, project.revision + 1);
    let map = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(map.originals("TERM_0001").unwrap(), ["ACME"]);
    assert_eq!(map.originals("TERM_0002").unwrap(), ["ACME Portal"]);
    let repeat = ProjectStore::new(root.path())
        .prepare_bundle(
            &project.project_id,
            review.project_revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(repeat.preview, review.preview);
    assert_eq!(repeat.project_revision, review.project_revision);
    let out = tempfile::tempdir().unwrap();
    store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
}

#[test]
fn terms_distinguish_generated_privacy_labels_from_identical_source_text() {
    let (_root,store,project)=fixture("ACME alice@example.test db.local password=SYNTHETIC_SECRET literal PERSON_0001 HOST_0001 EMAIL_0001 REDACTED_SECRET",json!([
        {"value":"ACME"}, {"value":"PERSON_0001","boundary":"substring"}, {"value":"HOST_0001","boundary":"substring"}, {"value":"EMAIL_0001","boundary":"substring"}, {"value":"REDACTED_SECRET","boundary":"substring"}
    ]));
    let options=serde_json::from_value(json!({"redact_candidates":false,"pii":{"categories":["participants","emails"]},"infrastructure":{"categories":["host"]}})).unwrap();
    let review = store
        .prepare_bundle(&project.project_id, project.revision, options, || false)
        .unwrap();
    assert!(review
        .preview
        .contains("> PERSON_0001 EMAIL_0001 HOST_0001 password=[REDACTED_SECRET] literal TERM_"));
    assert!(review.preview.contains("Sender: \"PERSON_0001\""));
    let line = review
        .preview
        .lines()
        .find(|line| line.starts_with("> "))
        .unwrap();
    let literal = line.split_once(" literal ").unwrap().1;
    assert_eq!(literal.split_whitespace().count(), 4);
    assert!(literal
        .split_whitespace()
        .all(|part| part.starts_with("TERM_")));
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["custom_terms"]["replacements"],
        4
    );
    assert_eq!(review.project_revision, project.revision + 1);
}

#[test]
fn term_boundaries_are_exact_unicode_aware_and_checked_before_overlap_selection() {
    use tgsum_core::bundle::BundleOptions;
    let cases = [
        (
            "a?Bob",
            json!([{"value":"?Bob"},{"value":"Bob"}]),
            "a?TERM_0001",
        ),
        (
            "é e\u{301} é\u{301} É xé é_x (é)",
            json!([{"value":"é"}]),
            "TERM_0001 e\u{301} é\u{301} É xé é_x (TERM_0001)",
        ),
        (
            "ACMEish preACMEpost acme",
            json!([{"value":"ACME","boundary":"substring"}]),
            "TERM_0001ish preTERM_0001post acme",
        ),
        (
            "https://customer.example/repo customer.example",
            json!([{"value":"customer.example"}]),
            "https://TERM_0001/repo TERM_0001",
        ),
    ];
    for (input, terms, expected) in cases {
        let (_root, store, project) = fixture(input, terms);
        let review = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                BundleOptions::default(),
                || false,
            )
            .unwrap();
        assert_eq!(
            review
                .preview
                .lines()
                .find_map(|s| s.strip_prefix("> "))
                .unwrap(),
            expected
        );
    }
}

#[test]
fn changing_or_clearing_a_dictionary_preserves_historical_mappings_and_invalidates_review() {
    use tgsum_core::bundle::BundleOptions;
    let (_root, store, project) = fixture("ACME Project", json!([{"value":"ACME"}]));
    let first = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    let change=serde_json::from_value(json!({"kind":"custom_terms","value":{"entries":[{"value":"Project"},{"value":"ACME","boundary":"substring"}]}})).unwrap();
    let updated = store
        .update(&project.project_id, first.project_revision, change)
        .unwrap();
    let out = tempfile::tempdir().unwrap();
    assert_eq!(
        store
            .export_bundle(
                &project.project_id,
                &first.bundle_id,
                first.project_revision,
                out.path(),
                || false
            )
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    let second = store
        .prepare_bundle(
            &project.project_id,
            updated.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert!(second.preview.contains("> TERM_0001 TERM_0002"));
    let cleared = store
        .update(
            &project.project_id,
            second.project_revision,
            serde_json::from_value(json!({"kind":"custom_terms","value":{"entries":[]}})).unwrap(),
        )
        .unwrap();
    let third = store
        .prepare_bundle(
            &project.project_id,
            cleared.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert!(third.preview.contains("> ACME Project"));
    assert!(serde_json::to_value(&third.manifest)
        .unwrap()
        .get("custom_terms")
        .is_none());
    assert_eq!(third.project_revision, cleared.revision);
    let old = store
        .bundle_pseudonyms(&project.project_id, &first.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(old.originals("TERM_0001").unwrap(), ["ACME"]);
    assert!(old.originals("TERM_0002").is_none());
}

#[test]
fn dictionary_changes_during_prepare_preserve_the_winning_policy_and_mapping_pointer() {
    use std::cell::Cell;
    use tgsum_core::bundle::BundleOptions;
    let (_root, store, project) = fixture("ACME", json!([{"value":"ACME"}]));
    let edited = Cell::new(false);
    let error=store.prepare_bundle(&project.project_id,project.revision,BundleOptions::default(),|| {
        if !edited.replace(true) {
            store.update(&project.project_id,project.revision,serde_json::from_value(json!({"kind":"custom_terms","value":{"entries":[{"value":"Winner"}]}})).unwrap()).unwrap();
        }
        false
    }).err().unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    let current = store.open(&project.project_id).unwrap();
    assert_eq!(current.revision, project.revision + 1);
    assert!(current.pseudonyms.is_none());
    assert_eq!(
        serde_json::to_value(&current).unwrap()["custom_terms"]["entries"][0]["value"],
        "Winner"
    );
}

#[test]
fn legacy_project_defaults_to_no_terms_without_rewriting_the_old_revision() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Legacy").unwrap();
    let path = root
        .path()
        .join(&project.project_id)
        .join("revisions")
        .join("00000000000000000000.json");
    let mut legacy = serde_json::to_value(&project).unwrap();
    legacy["schema_version"] = json!(4);
    legacy.as_object_mut().unwrap().remove("custom_terms");
    let bytes = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let reopened = store.open(&project.project_id).unwrap();
    assert!(reopened.custom_terms.is_empty());
    assert_eq!(reopened.schema_version, 8);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn term_replacements_hide_review_excerpts_but_do_not_clear_pending_secret_review() {
    use tgsum_core::bundle::BundleOptions;
    let (_root, store, project) = fixture(
        "😀 Клиент customer-private key=ordinary-value password=SYNTHETIC_SECRET",
        json!([{"value":"customer-private"},{"value":"ordinary-value"}]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.privacy.needs_review, 1);
    assert!(review.findings[0].excerpt.contains("key=TERM_"));
    assert!(
        !review.findings[0].excerpt.contains("customer-private")
            && !review.findings[0].excerpt.contains("ordinary-value")
            && !review.findings[0].excerpt.contains("SYNTHETIC_SECRET")
    );
    let out = tempfile::tempdir().unwrap();
    assert!(store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            out.path(),
            || false
        )
        .is_err());
    let ready = store
        .prepare_bundle(
            &project.project_id,
            review.project_revision,
            BundleOptions {
                redact_candidates: true,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
    assert_eq!(ready.project_revision, review.project_revision);
    let exported = store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            ready.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
    for file in std::fs::read_dir(exported.directory).unwrap() {
        let contents = std::fs::read_to_string(file.unwrap().path()).unwrap();
        for raw in [
            "customer-private",
            "ordinary-value",
            "SYNTHETIC_SECRET",
            "entries",
            "boundary",
        ] {
            assert!(!contents.contains(raw), "{raw}");
        }
    }
}

#[test]
fn invalid_or_excessive_dictionaries_do_not_publish_and_runtime_match_budget_is_bounded() {
    use tgsum_core::bundle::BundleOptions;
    use tgsum_core::custom_terms::{CustomTerm, CustomTerms, TermBoundary};
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Terms fixture").unwrap();
    let lists = vec![
        vec![String::new()],
        vec!["PRIVATE\nVALUE".into()],
        vec![" PRIVATE ".into()],
        vec!["PRIVATE_DUP".into(); 2],
        vec!["P".repeat(4097)],
        (0..1025).map(|i| format!("PRIVATE_{i}")).collect(),
        (0..17)
            .map(|i| format!("{i:04}{}", "P".repeat(4092)))
            .collect(),
    ];
    for values in lists {
        let change = ProjectChange::CustomTerms(CustomTerms {
            entries: values
                .into_iter()
                .map(|value| CustomTerm {
                    value,
                    boundary: TermBoundary::Word,
                })
                .collect(),
        });
        let error = store
            .update(&project.project_id, project.revision, change)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(!error.to_string().contains("PRIVATE"));
        assert_eq!(
            store.open(&project.project_id).unwrap().revision,
            project.revision
        );
    }
    let value = "Q".repeat(4096);
    let (_root, store, project) = fixture(&value, json!([{"value":value}]));
    let ready = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert!(ready.preview.contains("> TERM_0001"));
    let (_root, store, project) = fixture(
        &"x".repeat(100001),
        json!([{"value":"x","boundary":"substring"}]),
    );
    let error = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("limit"));
    let unchanged = store.open(&project.project_id).unwrap();
    assert_eq!(unchanged.revision, project.revision);
    assert!(unchanged.pseudonyms.is_none());
}
