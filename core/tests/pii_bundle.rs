use std::io::Cursor;

use serde_json::{json, Value};
use tgsum_core::bundle::BundleOptions;
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

fn add_source(
    store: &ProjectStore,
    project: Project,
    source_id: &str,
    scope: SourceScope,
    messages: Value,
) -> Project {
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            source_id,
            &scope,
            Cursor::new(
                serde_json::to_vec(&json!({"id":scope.conversation_id,"messages":messages}))
                    .unwrap(),
            ),
        )
        .unwrap();
    store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: source_id.into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: None,
                latest_snapshot_id: Some(source_id.into()),
                selection: Default::default(),
            }),
        )
        .unwrap()
}

fn participants() -> BundleOptions {
    serde_json::from_value(json!({"redact_candidates":false,"pii":{"categories":["participants"]}}))
        .unwrap()
}

#[test]
fn equal_names_keep_distinct_native_identities_and_ambiguous_mentions_choose_neither() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("synthetic", "1"),
        json!([
            {"id":1,"from":"Alice","from_id":"user1","text":"Alice and Bob, ask Alice."},
            {"id":2,"from":"Alice","from_id":"user2","text":"Alice"},
            {"id":3,"from":"Bob","from_id":"user3","text":"Bob"}
        ]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("Sender: \"PERSON_0001\""));
    assert!(review.preview.contains("Sender: \"PERSON_0002\""));
    assert!(review.preview.contains("Sender: \"PERSON_0003\""));
    assert!(review
        .preview
        .contains("[REDACTED_PERSON] and PERSON_0003, ask [REDACTED_PERSON]."));
    assert!(!review.preview.contains("Alice") && !review.preview.contains("Bob"));
    let map = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(map.originals("PERSON_0001").unwrap(), ["Alice"]);
    assert_eq!(map.originals("PERSON_0002").unwrap(), ["Alice"]);
    assert_eq!(review.project_revision, project.revision + 1);
    let summary = serde_json::to_value(&review.manifest).unwrap();
    assert_eq!(summary["pii"]["replacements"], 8);
    assert_eq!(summary["pii"]["ambiguous"], 3);
    let destination = tempfile::tempdir().unwrap();
    store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
}

#[test]
fn participant_aliases_respect_unicode_boundaries_and_other_structured_values() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("synthetic", "1"),
        json!([
            {"id":1,"from":"Ann","from_id":"user1","text":"Ann Anna Ann\u{301} (Ann) https://EXAMPLE.com:443/Ann Ann@example.test @Ann /home/Ann/file Ann"}
        ]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("PERSON_0001 Anna Ann\u{301} (PERSON_0001) https://EXAMPLE.com:443/Ann Ann@example.test @Ann /home/Ann/file PERSON_0001"));
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["pii"]["replacements"],
        4
    );
}

#[test]
fn participant_aliases_never_split_phone_or_unsupported_handle_candidates() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("synthetic", "1"),
        json!([
            {"id":1,"from":"202","from_id":"user1","text":"202 +1 (202) 555-0100 ext.202 @202foo-bar"},
            {"id":2,"from":"bar","from_id":"user2","text":"bar"}
        ]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(review
        .preview
        .contains("> PERSON_0001 +1 (202) 555-0100 ext.202 @202foo-bar"));
    let review=store.prepare_bundle(&project.project_id,review.project_revision,serde_json::from_value(json!({"redact_candidates":false,"pii":{"categories":["participants","phones","usernames"]}})).unwrap(),||false).unwrap();
    assert!(review
        .preview
        .contains("> PERSON_0001 PHONE_0001 @202foo-bar"));
}

#[test]
fn observed_rename_and_new_conversation_reuse_identity_while_other_accounts_do_not() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "first",
        SourceScope::telegram("one", "1"),
        json!([
            {"id":1,"from":"Иван Петров","from_id":"user9007199254740993","text":"Иван Петров"}
        ]),
    );
    let first = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    let current = store.open(&project.project_id).unwrap();
    let current = add_source(
        &store,
        current,
        "second",
        SourceScope::telegram("one", "2"),
        json!([
            {"id":1,"from":"Иван Сидоров","from_id":"user9007199254740993","text":"Иван Петров и Иван Сидоров"}
        ]),
    );
    let current = add_source(
        &store,
        current,
        "third",
        SourceScope::telegram("two", "1"),
        json!([
            {"id":1,"from":"Other account","from_id":"user9007199254740993","text":"Иван Петров, Other account"}
        ]),
    );
    let second = store
        .prepare_bundle(
            &project.project_id,
            current.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert_eq!(second.preview.matches("Sender: \"PERSON_0001\"").count(), 2);
    assert_eq!(second.preview.matches("Sender: \"PERSON_0002\"").count(), 1);
    assert!(second.preview.contains("PERSON_0001 и PERSON_0001"));
    let mapping = store
        .bundle_pseudonyms(&project.project_id, &second.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        mapping.originals("PERSON_0001").unwrap(),
        ["Иван Петров", "Иван Сидоров"]
    );
    assert_eq!(
        store
            .bundle_pseudonyms(&project.project_id, &first.bundle_id)
            .unwrap()
            .unwrap()
            .originals("PERSON_0001")
            .unwrap(),
        ["Иван Петров"]
    );
    let repeat = ProjectStore::new(root.path())
        .prepare_bundle(
            &project.project_id,
            second.project_revision,
            participants(),
            || false,
        )
        .unwrap();
    assert_eq!(repeat.project_revision, second.project_revision);
    assert_eq!(repeat.preview, second.preview);
}

#[test]
fn unknown_participants_are_not_merged_by_name_and_empty_identity_does_not_create_a_map() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("one", "1"),
        json!([
            {"id":1,"from":"Alice","text":"Alice"},
            {"id":2,"text":"No sender"}
        ]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("Sender: \"[REDACTED_PERSON]\""));
    assert!(review.preview.contains("Sender: \"Unknown sender\""));
    assert!(!review.preview.contains("Alice"));
    assert_eq!(review.project_revision, project.revision);
    assert!(review.manifest.pseudonym_mapping_id.is_none());
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["pii"]["unresolved"],
        2
    );
    let destination = tempfile::tempdir().unwrap();
    store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
    let current = add_source(
        &store,
        store.open(&project.project_id).unwrap(),
        "known",
        SourceScope::telegram("one", "2"),
        json!([
            {"id":1,"from":"Alice","from_id":"user1","text":"Alice"},
            {"id":2,"from_id":"user2","text":"user2"}
        ]),
    );
    let next = store
        .prepare_bundle(
            &project.project_id,
            current.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(next.preview.contains("Sender: \"PERSON_0001\""));
    assert!(next.preview.contains("Sender: \"PERSON_0002\""));
    assert!(!next.preview.contains("> PERSON_0001"));
    assert!(next.preview.contains("> user2"));
    assert_eq!(
        serde_json::to_value(&next.manifest).unwrap()["pii"]["ambiguous"],
        2
    );
}

#[test]
fn an_invalid_longer_alias_does_not_hide_a_valid_overlapping_name() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("one", "1"),
        json!([
            {"id":1,"from":"?Bob","from_id":"user1","text":"a?Bob"},
            {"id":2,"from":"Bob","from_id":"user2","text":"?Bob Bob"}
        ]),
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("> a?PERSON_0002"));
    assert!(review.preview.contains("> PERSON_0001 PERSON_0002"));
}

#[test]
fn unicode_and_case_aliases_are_exact_and_platform_namespaces_are_independent() {
    use tgsum_core::connector::{
        ArchiveImporter, ConversationObservation, MessageObservation, TelegramJson,
    };
    use tgsum_core::snapshot::{CanonicalMessage, Coverage, MessageKey};
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let names = [
        "é", "e\u{301}", "Straße", "STRASSE", "Alice", "alice", "A", "А",
    ];
    let messages: Vec<_> = names.iter().enumerate().map(|(i, name)| json!({"id":i+1,"from":name,"from_id":format!("user{}",i+1),"text":if i==0 {names.join(" ")} else {String::new()}})).collect();
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("one", "1"),
        json!(messages),
    );
    let other = SourceScope {
        platform: "synthetic".into(),
        account_local_id: "one".into(),
        conversation_id: "1".into(),
    };
    let mut message = CanonicalMessage::new(
        MessageKey {
            source: other.clone(),
            message_id: "1".into(),
        },
        "é",
    );
    message.sender_id = Some("user1".into());
    message.sender_name = Some("é".into());
    let mut descriptor = TelegramJson.descriptor();
    descriptor.id = "synthetic";
    descriptor.platform = "synthetic";
    store
        .snapshots(&project.project_id)
        .unwrap()
        .publish_observation(
            descriptor,
            "other-platform",
            &other,
            ConversationObservation {
                source: other.clone(),
                title: None,
                kind: "synthetic".into(),
                coverage: Coverage::unknown("fixture"),
                messages: vec![MessageObservation::native_present(message)],
            },
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "other".into(),
                connector_id: "synthetic".into(),
                scope: other,
                archive_path: None,
                latest_snapshot_id: Some("other-platform".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            participants(),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("> [REDACTED_PERSON] PERSON_0002 PERSON_0003 PERSON_0004 PERSON_0005 PERSON_0006 PERSON_0007 PERSON_0008"));
    assert!(review.preview.contains("Sender: \"PERSON_0001\""));
    assert!(review.preview.contains("Sender: \"PERSON_0009\""));
    let map = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(map.originals("PERSON_0001"), map.originals("PERSON_0009"));
    assert_eq!(map.originals("PERSON_0002").unwrap(), ["e\u{301}"]);
}

#[test]
fn discovery_uses_selected_messages_and_both_privacy_stages_share_one_revision() {
    use tgsum_core::scope::{DateBasis, DateRange};
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = add_source(
        &store,
        store.create("PII fixture").unwrap(),
        "work",
        SourceScope::telegram("one", "1"),
        json!([
            {"id":1,"date":"2026-09-26T12:00:00","from":"Excluded","from_id":"user99","text":"Excluded"},
            {"id":2,"date":"2026-09-27T12:00:00","from":"Alice","from_id":"user1","text":"Alice db.local 10.0.0.2 key=ordinary-value token=SYNTHETIC_PRIVATE"}
        ]),
    );
    let mut selection = project.sources[0].selection.clone();
    selection.filter.dates = Some(DateRange {
        from: Some("2026-09-27".into()),
        through: None,
        basis: DateBasis::SourceDate,
    });
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection,
            },
        )
        .unwrap();
    let options: BundleOptions = serde_json::from_value(json!({"redact_candidates":false,"pii":{"categories":["participants"]},"infrastructure":{"categories":["host","ip"]}})).unwrap();
    let pending = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options.clone(),
            || false,
        )
        .unwrap();
    assert_eq!(pending.project_revision, project.revision + 1);
    assert_eq!(pending.manifest.messages, 1);
    assert_eq!(pending.manifest.privacy.needs_review, 1);
    assert!(pending.findings[0]
        .excerpt
        .contains("PERSON_0001 HOST_0001 IP_0001 key=ordinary-value"));
    assert!(!pending.findings[0].excerpt.contains("SYNTHETIC_PRIVATE"));
    let map = store
        .bundle_pseudonyms(&project.project_id, &pending.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(map.originals("PERSON_0001").unwrap(), ["Alice"]);
    assert!(map.originals("PERSON_0002").is_none());
    let ready = store
        .prepare_bundle(
            &project.project_id,
            pending.project_revision,
            BundleOptions {
                redact_candidates: true,
                ..options
            },
            || false,
        )
        .unwrap();
    assert_eq!(ready.project_revision, pending.project_revision);
    let destination = tempfile::tempdir().unwrap();
    let exported = store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            ready.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
    for file in std::fs::read_dir(exported.directory).unwrap() {
        let content = std::fs::read_to_string(file.unwrap().path()).unwrap();
        for forbidden in [
            "Alice",
            "Excluded",
            "user1",
            "user99",
            "db.local",
            "10.0.0.2",
            "SYNTHETIC_PRIVATE",
            "ordinary-value",
        ] {
            assert!(!content.contains(forbidden), "{forbidden}");
        }
    }
    assert_eq!(
        std::fs::read_dir(root.path().join(&project.project_id).join("pseudonyms"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn participant_budgets_fail_before_mapping_or_bundle_publication() {
    for oversized_dictionary in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path());
        let messages = if oversized_dictionary {
            (0..10_001).map(|i| json!({"id":i+1,"from":format!("Fixture Person {i}"),"from_id":format!("user{i}"),"text":"ordinary"})).collect::<Vec<_>>()
        } else {
            vec![json!({"id":1,"from":"🦀","from_id":"user1","text":"🦀 ".repeat(100_001)})]
        };
        let project = add_source(
            &store,
            store.create("PII fixture").unwrap(),
            "work",
            SourceScope::telegram("one", "1"),
            json!(messages),
        );
        let error = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                participants(),
                || false,
            )
            .err()
            .unwrap();
        assert!(error.to_string().contains(if oversized_dictionary {
            "alias dictionary exceeds limits"
        } else {
            "match attempts exceed limit"
        }));
        assert!(!error.to_string().contains("Fixture Person") && !error.to_string().contains("🦀"));
        let current = store.open(&project.project_id).unwrap();
        assert_eq!(current.revision, project.revision);
        assert!(current.pseudonyms.is_none());
        let directory = root.path().join(&project.project_id);
        assert!(!directory.join("pseudonyms").exists());
        if directory.join("bundles").exists() {
            assert_eq!(
                std::fs::read_dir(directory.join("bundles"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }
}

#[test]
fn late_cancel_and_concurrent_edit_preserve_the_current_participant_mapping() {
    use std::{cell::Cell, fs};
    for conflict in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path());
        let project = add_source(
            &store,
            store.create("PII fixture").unwrap(),
            "work",
            SourceScope::telegram("one", "1"),
            json!([
                {"id":1,"from":"Alice","from_id":"user1","text":"Alice"}
            ]),
        );
        let reached = Cell::new(false);
        let bundles = root.path().join(&project.project_id).join("bundles");
        let error = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                participants(),
                || {
                    if fs::read_dir(&bundles).is_ok_and(|entries| {
                        entries
                            .flatten()
                            .any(|e| e.path().join("private.json").exists())
                    }) && !reached.replace(true)
                    {
                        if conflict {
                            store
                                .update(
                                    &project.project_id,
                                    project.revision,
                                    ProjectChange::Rename("Winner".into()),
                                )
                                .unwrap();
                        }
                        return !conflict;
                    }
                    false
                },
            )
            .err()
            .unwrap();
        assert!(reached.get());
        assert_eq!(
            error.kind(),
            if conflict {
                std::io::ErrorKind::WouldBlock
            } else {
                std::io::ErrorKind::Other
            }
        );
        let current = store.open(&project.project_id).unwrap();
        assert_eq!(current.revision, project.revision + u64::from(conflict));
        assert!(current.pseudonyms.is_none() && current.baselines.is_empty());
        assert_eq!(fs::read_dir(&bundles).unwrap().count(), 0);
        let retry = store
            .prepare_bundle(
                &project.project_id,
                current.revision,
                participants(),
                || false,
            )
            .unwrap();
        assert!(retry.preview.contains("Sender: \"PERSON_0001\""));
    }
}
