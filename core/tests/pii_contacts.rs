use std::io::Cursor;

use serde_json::json;
use tgsum_core::bundle::BundleOptions;
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

fn fixture(text: &str) -> (tempfile::TempDir, ProjectStore, Project) {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Contacts").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    store.snapshots(&project.project_id).unwrap().import_telegram("initial",&scope,Cursor::new(serde_json::to_vec(&json!({"id":1,"messages":[{"id":1,"from":"Alice","from_id":"user1","text":text}]})).unwrap())).unwrap();
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
    (root, store, project)
}

fn options(categories: &[&str]) -> BundleOptions {
    serde_json::from_value(json!({"redact_candidates":false,"pii":{"categories":categories}}))
        .unwrap()
}

#[test]
fn email_identity_preserves_local_case_and_normalizes_domain_without_enabling_participants() {
    let (root,store,project)=fixture("Alice@EXAMPLE.test Alice@example.test alice@example.test user@пример.рф user@xn--e1afmkfd.xn--p1ai");
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&["emails"]),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("Sender: \"Alice\""));
    assert!(review
        .preview
        .contains("> EMAIL_0001 EMAIL_0001 EMAIL_0002 EMAIL_0003 EMAIL_0003"));
    let mapping = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        mapping.originals("EMAIL_0001").unwrap(),
        ["Alice@EXAMPLE.test", "Alice@example.test"]
    );
    assert_eq!(
        mapping.originals("EMAIL_0002").unwrap(),
        ["alice@example.test"]
    );
    assert!(mapping.originals("PERSON_0001").is_none());
    let summary = serde_json::to_value(&review.manifest).unwrap();
    assert_eq!(summary["pii"]["replacements"], 5);
    assert_eq!(summary["pii"]["by_category"], json!({"emails":5}));
    let repeat = ProjectStore::new(root.path())
        .prepare_bundle(
            &project.project_id,
            review.project_revision,
            options(&["emails"]),
            || false,
        )
        .unwrap();
    assert_eq!(repeat.preview, review.preview);
    assert_eq!(repeat.project_revision, review.project_revision);
}

#[test]
fn email_shape_corpus_preserves_wrappers_and_rejects_malformed_partial_addresses() {
    let cases = [
        ("a+b@example.test", "EMAIL_0001"),
        ("o'connor@example.test", "EMAIL_0001"),
        ("пользователь@пример.рф", "EMAIL_0001"),
        ("😀alice@example.test", "EMAIL_0001"),
        ("<Alice@EXAMPLE.test>,", "<EMAIL_0001>,"),
        ("alice@example.test.", "EMAIL_0001."),
        ("'Alice@example.test'", "'EMAIL_0001'"),
        ("\"Alice@example.test\"", "\"EMAIL_0001\""),
        ("“Alice@example.test”", "“EMAIL_0001”"),
        ("`Alice@example.test`", "`EMAIL_0001`"),
        ("a@b.test: next", "EMAIL_0001: next"),
        (".a@example.test", ".a@example.test"),
        ("a..b@example.test", "a..b@example.test"),
        ("a@-example.test", "a@-example.test"),
        ("a@example..test", "a@example..test"),
        ("a@example.test..", "a@example.test.."),
        ("a@example_test.com", "a@example_test.com"),
        ("\"two words\"@example.test", "\"two words\"@example.test"),
        ("foo@[127.0.0.1]", "foo@[127.0.0.1]"),
        ("foo@localhost", "foo@localhost"),
        ("foo@example.test:22", "foo@example.test:22"),
        ("foo@example.test/path", "foo@example.test/path"),
        ("foo@example.test%extra", "foo@example.test%extra"),
        (
            "https://example.test/?mail=foo@example.test",
            "https://example.test/?mail=foo@example.test",
        ),
        ("mailto:foo@example.test", "mailto:foo@example.test"),
        ("/home/foo@example.test/file", "/home/foo@example.test/file"),
        ("ssh foo@example.test", "ssh foo@example.test"),
        ("foo@example.test@evil.test", "foo@example.test@evil.test"),
    ];
    for (input, expected) in cases {
        let (_root, store, project) = fixture(input);
        let review = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                options(&["emails"]),
                || false,
            )
            .unwrap();
        assert_eq!(
            review
                .preview
                .lines()
                .find_map(|s| s.strip_prefix("> "))
                .unwrap(),
            expected,
            "{input}"
        );
    }
}

#[test]
fn international_phone_shapes_reuse_digits_without_guessing_a_region() {
    let (_root,store,project)=fixture("+1 (202) 555-0100 +12025550100 +44 20 7946 0958 +442079460958 user@example.test 2026-09-27");
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&["phones"]),
            || false,
        )
        .unwrap();
    assert!(review
        .preview
        .contains("> PHONE_0001 PHONE_0001 PHONE_0002 PHONE_0002 user@example.test 2026-09-27"));
    assert!(review.preview.contains("Sender: \"Alice\""));
    let map = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        map.originals("PHONE_0001").unwrap(),
        ["+1 (202) 555-0100", "+12025550100"]
    );
    assert!(map.originals("PERSON_0001").is_none() && map.originals("EMAIL_0001").is_none());
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["pii"]["by_category"],
        json!({"phones":4})
    );
}

#[test]
fn phone_extensions_are_hidden_and_keep_distinct_endpoint_identities() {
    let (_root, store, project) = fixture(
        "+12025550100 ext.123; +1 (202) 555-0100 x123; +12025550100;ext=456; +12025550100 ext.abc",
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&["phones"]),
            || false,
        )
        .unwrap();
    assert!(review
        .preview
        .contains("> PHONE_0001; PHONE_0001; PHONE_0002; +12025550100 ext.abc"));
    let map = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        map.originals("PHONE_0001").unwrap(),
        ["+1 (202) 555-0100 x123", "+12025550100 ext.123"]
    );
    assert_eq!(
        map.originals("PHONE_0002").unwrap(),
        ["+12025550100;ext=456"]
    );
}

#[test]
fn phone_shape_corpus_preserves_nonphones_and_complete_unsupported_containers() {
    let cases = [
        ("(+1 (202) 555-0100).", "(PHONE_0001)."),
        ("+44\u{00a0}20\u{202f}7946\u{00a0}0958", "PHONE_0001"),
        ("+12025550100 доб. 001", "PHONE_0001"),
        ("+12025550100 x 123", "PHONE_0001"),
        ("+12025550100;ext=123", "PHONE_0001"),
        ("+12025550100 extension 123", "PHONE_0001"),
        ("+12025550100 ext.１２３", "+12025550100 ext.１２３"),
        (
            "+12025550100 ext.12345678901",
            "+12025550100 ext.12345678901",
        ),
        ("+12025550100 ext.1 ext.2", "+12025550100 ext.1 ext.2"),
        ("+12025550100 ext.", "+12025550100 ext."),
        ("+1 (202 555-0100", "+1 (202 555-0100"),
        ("+1 ((202)) 555-0100", "+1 ((202)) 555-0100"),
        ("+1202555010012345", "+1202555010012345"),
        ("+012025550100", "+012025550100"),
        ("+1234567", "+1234567"),
        ("+2026-09-27", "+2026-09-27"),
        (
            "2026-09-27 2025550100 0012025550100",
            "2026-09-27 2025550100 0012025550100",
        ),
        ("id+12025550100", "id+12025550100"),
        ("+12025550100abc", "+12025550100abc"),
        ("+12025550100１２", "+12025550100１２"),
        ("+12025550100@example.test", "+12025550100@example.test"),
        ("tel:+12025550100;ext=1", "tel:+12025550100;ext=1"),
        (
            "https://example.test/+12025550100",
            "https://example.test/+12025550100",
        ),
        ("/contacts/+12025550100.txt", "/contacts/+12025550100.txt"),
    ];
    for (input, expected) in cases {
        let (_root, store, project) = fixture(input);
        let review = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                options(&["phones"]),
                || false,
            )
            .unwrap();
        assert_eq!(
            review
                .preview
                .lines()
                .find_map(|line| line.strip_prefix("> "))
                .unwrap(),
            expected,
            "input: {input}"
        );
    }
}

#[test]
fn telegram_handles_casefold_without_linking_participants_or_email_domains() {
    let (_root, store, project) = fixture("@Alice @ALICE @alice Alice alice@example.test 😀@Alice https://t.me/Alice /home/@Alice ssh user@Alice");
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&["usernames"]),
            || false,
        )
        .unwrap();
    assert!(review.preview.contains("> USER_0001 USER_0001 USER_0001 Alice alice@example.test 😀@Alice https://t.me/Alice /home/@Alice ssh user@Alice"));
    assert!(review.preview.contains("Sender: \"Alice\""));
    let map = store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        map.originals("USER_0001").unwrap(),
        ["@ALICE", "@Alice", "@alice"]
    );
    assert!(map.originals("PERSON_0001").is_none() && map.originals("EMAIL_0001").is_none());
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["pii"]["by_category"],
        json!({"usernames":3})
    );
}

#[test]
fn unscoped_project_handle_is_redacted_without_inventing_a_platform_identity() {
    let (_root, store, project) = fixture("hello");
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Rename("@Alice".into()),
        )
        .unwrap();
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&["usernames"]),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.project_title, "[REDACTED_USERNAME]");
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["pii"]["unresolved"],
        1
    );
    assert!(store
        .bundle_pseudonyms(&project.project_id, &review.bundle_id)
        .unwrap()
        .is_none());
    assert_eq!(review.project_revision, project.revision);
}

#[test]
fn contact_scope_growth_and_restart_preserve_labels_without_merging_handle_namespaces() {
    use tgsum_core::connector::{
        ArchiveImporter, ConversationObservation, MessageObservation, TelegramJson,
    };
    use tgsum_core::snapshot::{CanonicalMessage, Coverage, MessageKey};
    let (root, store, project) = fixture("@Alice @alice alice@example.test +12025550100");
    let policy = options(&["usernames", "emails", "phones"]);
    let first = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            policy.clone(),
            || false,
        )
        .unwrap();
    let mut project = store.open(&project.project_id).unwrap();
    for (id, scope) in [
        ("second", SourceScope::telegram("synthetic", "2")),
        ("account", SourceScope::telegram("another", "1")),
        (
            "platform",
            SourceScope {
                platform: "synthetic".into(),
                account_local_id: "synthetic".into(),
                conversation_id: "1".into(),
            },
        ),
    ] {
        let message = CanonicalMessage::new(
            MessageKey {
                source: scope.clone(),
                message_id: "1".into(),
            },
            "@Alice @alice alice@example.test +12025550100",
        );
        let mut descriptor = TelegramJson.descriptor();
        if scope.platform == "synthetic" {
            descriptor.id = "synthetic";
            descriptor.platform = "synthetic";
        }
        store
            .snapshots(&project.project_id)
            .unwrap()
            .publish_observation(
                descriptor,
                id,
                &scope,
                ConversationObservation {
                    source: scope.clone(),
                    title: None,
                    kind: "synthetic".into(),
                    coverage: Coverage::unknown("fixture"),
                    messages: vec![MessageObservation::native_present(message)],
                },
            )
            .unwrap();
        project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: id.into(),
                    connector_id: descriptor.id.into(),
                    scope,
                    archive_path: None,
                    latest_snapshot_id: Some(id.into()),
                    selection: Default::default(),
                }),
            )
            .unwrap();
    }
    let review = ProjectStore::new(root.path())
        .prepare_bundle(
            &project.project_id,
            project.revision,
            policy.clone(),
            || false,
        )
        .unwrap();
    let lines: Vec<_> = review
        .preview
        .lines()
        .filter_map(|line| line.strip_prefix("> "))
        .collect();
    assert_eq!(
        lines,
        [
            "USER_0001 USER_0001 EMAIL_0001 PHONE_0001",
            "USER_0001 USER_0001 EMAIL_0001 PHONE_0001",
            "USER_0002 USER_0002 EMAIL_0001 PHONE_0001",
            "USER_0003 USER_0004 EMAIL_0001 PHONE_0001"
        ]
    );
    let old = store
        .bundle_pseudonyms(&project.project_id, &first.bundle_id)
        .unwrap()
        .unwrap();
    assert!(old.originals("USER_0002").is_none());
    let repeat = store
        .prepare_bundle(&project.project_id, review.project_revision, policy, || {
            false
        })
        .unwrap();
    assert_eq!(repeat.preview, review.preview);
    assert_eq!(repeat.project_revision, review.project_revision);
    // Persisted mappings do not silently enable a disabled category.
    let disabled = store
        .prepare_bundle(
            &project.project_id,
            repeat.project_revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert!(disabled
        .preview
        .contains("> @Alice @alice alice@example.test +12025550100"));
    assert!(disabled.manifest.pii.is_none());
    assert_eq!(disabled.project_revision, repeat.project_revision);
}

#[test]
fn handle_shape_corpus_does_not_replace_unicode_or_invalid_suffixes_partially() {
    let cases = [
        (
            "(@alice), @a @alice_123.",
            "(USER_0002), USER_0001 USER_0003.",
        ),
        ("“@Alice”", "“USER_0001”"),
        (
            "@Аlice @alice\u{301} @alice😀",
            "@Аlice @alice\u{301} @alice😀",
        ),
        (
            "@alice-foo @alice.example @@alice @alice@example.test",
            "@alice-foo @alice.example @@alice @alice@example.test",
        ),
        (
            "@abcdefghijklmnopqrstuvwxyz1234567",
            "@abcdefghijklmnopqrstuvwxyz1234567",
        ),
        (
            "@alice/path user@alice C:\\@alice\\file",
            "@alice/path user@alice C:\\@alice\\file",
        ),
    ];
    for (input, expected) in cases {
        let (_root, store, project) = fixture(input);
        let review = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                options(&["usernames"]),
                || false,
            )
            .unwrap();
        assert_eq!(
            review
                .preview
                .lines()
                .find_map(|line| line.strip_prefix("> "))
                .unwrap(),
            expected,
            "input: {input}"
        );
    }
}

#[test]
fn all_categories_share_one_private_revision_and_secret_review_survives_contact_replacements() {
    let (_root,store,project)=fixture("Alice alice@example.test +12025550100 @Alice db.local 10.0.0.2 key=ordinary-value token=SYNTHETIC_PRIVATE");
    let policy:BundleOptions=serde_json::from_value(json!({"redact_candidates":false,"pii":{"categories":["participants","emails","phones","usernames"]},"infrastructure":{"categories":["host","ip"]}})).unwrap();
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            policy.clone(),
            || false,
        )
        .unwrap();
    assert_eq!(review.project_revision, project.revision + 1);
    assert!(review.preview.contains(
        "> PERSON_0001 EMAIL_0001 PHONE_0001 USER_0001 HOST_0001 IP_0001 key=ordinary-value"
    ));
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["pii"]["by_category"],
        json!({"participants":2,"emails":1,"phones":1,"usernames":1})
    );
    assert_eq!(review.manifest.privacy.needs_review, 1);
    assert!(review.findings[0]
        .excerpt
        .contains("USER_0001 HOST_0001 IP_0001 key=ordinary-value"));
    assert!(!review.findings[0].excerpt.contains("SYNTHETIC_PRIVATE"));
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
                ..policy
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
            "Alice",
            "alice@example.test",
            "+12025550100",
            "db.local",
            "10.0.0.2",
            "ordinary-value",
            "SYNTHETIC_PRIVATE",
            "synthetic",
            "user1",
        ] {
            assert!(!contents.contains(raw), "{raw}");
        }
    }
}

#[test]
fn oversized_contact_candidates_fail_without_publishing_aliases_or_input_values() {
    for (category, input) in [
        ("emails", format!("{}@example.test", "a".repeat(17000))),
        ("phones", format!("+{}", "1".repeat(17000))),
        ("usernames", format!("@{}", "a".repeat(17000))),
    ] {
        let (_root, store, project) = fixture(&input);
        let error = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                options(&[category]),
                || false,
            )
            .err()
            .expect("bounded failure");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("limit"));
        assert!(!error.to_string().contains(&input));
        let unchanged = store.open(&project.project_id).unwrap();
        assert_eq!(unchanged.revision, project.revision);
        assert!(unchanged.pseudonyms.is_none());
    }
}
