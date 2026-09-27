use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;

use tgsum_core::bundle::BundleOptions;
use tgsum_core::infrastructure::{InfrastructureCategory as Kind, InfrastructurePolicy};
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

fn fixture(text: &str) -> (tempfile::TempDir, ProjectStore, Project) {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Project prod.local").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    let data = serde_json::json!({"id":1,"name":"Conversation prod.local","messages":[
        {"id":1,"from":"Alice","text":text},
        {"id":2,"from":"Bob","text":"10.0.0.2 db.local"}
    ]});
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "initial",
            &scope,
            Cursor::new(serde_json::to_vec(&data).unwrap()),
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            0,
            ProjectChange::Source(ProjectSource {
                source_id: "work".into(),
                connector_id: "telegram_export".into(),
                scope,
                archive_path: None,
                latest_snapshot_id: Some("initial".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    (root, store, project)
}

fn options(categories: &[Kind]) -> BundleOptions {
    BundleOptions {
        infrastructure: InfrastructurePolicy {
            categories: BTreeSet::from_iter(categories.iter().copied()),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn prepare_publishes_one_mapping_revision_and_reuses_it_for_repeat_review() {
    let (root, store, project) =
        fixture("db.local 10.0.0.2 password=SYNTHETIC_SECRET https://EXAMPLE.com:443/10.0.0.2");
    let selected = options(&[Kind::Ip, Kind::Host]);
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            selected.clone(),
            || false,
        )
        .unwrap();
    let current = store.open(&project.project_id).unwrap();
    assert_eq!(current.revision, project.revision + 1);
    assert_eq!(review.project_revision, current.revision);
    assert_eq!(current.sources, project.sources);
    assert_eq!(current.baselines, project.baselines);
    assert_eq!(
        review.manifest.pseudonym_mapping_id.as_deref(),
        Some(current.pseudonyms.as_ref().unwrap().id())
    );
    assert_eq!(review.manifest.project_title, "Project HOST_0001");
    assert_eq!(review.manifest.sources[0].title, "Conversation HOST_0001");
    assert!(review
        .preview
        .contains("HOST_0002 IP_0001 password=[REDACTED_SECRET] https://EXAMPLE.com:443/10.0.0.2"));
    assert!(!review.preview.contains("SYNTHETIC_SECRET") && !review.preview.contains("db.local"));
    let metadata = review.manifest.infrastructure.as_ref().unwrap();
    assert_eq!(metadata.rules_version, "infrastructure/1");
    assert_eq!(metadata.replacements, 6);
    assert_eq!(metadata.by_category[&Kind::Host], 4);
    assert_eq!(metadata.by_category[&Kind::Ip], 2);
    let maps = root.path().join(&project.project_id).join("pseudonyms");
    assert_eq!(std::fs::read_dir(&maps).unwrap().count(), 1);
    let again = store
        .prepare_bundle(&project.project_id, current.revision, selected, || false)
        .unwrap();
    assert_eq!(again.project_revision, current.revision);
    assert_eq!(again.preview, review.preview);
    assert_eq!(
        again.manifest.pseudonym_mapping_id,
        review.manifest.pseudonym_mapping_id
    );
    assert_eq!(std::fs::read_dir(maps).unwrap().count(), 1);
    let destination = tempfile::tempdir().unwrap();
    store
        .export_bundle(
            &project.project_id,
            &again.bundle_id,
            again.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
    let mapping = store
        .bundle_pseudonyms(&project.project_id, &again.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(mapping.originals("HOST_0002").unwrap(), ["db.local"]);
}

#[test]
fn category_selection_is_independent_and_disabled_or_unmatched_policies_do_not_write() {
    let body = "10.0.0.3 db.local domain=home.arpa https://db.local/status username=deploy /home/dev/work arn:aws:s3:::synthetic-bucket";
    for (category, marker) in [
        (Kind::Ip, "IP_0001"),
        (Kind::Host, "HOST_0001"),
        (Kind::Domain, "DOMAIN_0001"),
        (Kind::Url, "URL_0001"),
        (Kind::Username, "USER_0001"),
        (Kind::Path, "PATH_0001"),
        (Kind::CloudResource, "RESOURCE_0001"),
    ] {
        let (_root, store, project) = fixture(body);
        let review = store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                options(&[category]),
                || false,
            )
            .unwrap();
        assert!(review.preview.contains(marker), "{category:?}");
        let summary = review.manifest.infrastructure.unwrap();
        assert_eq!(summary.categories, BTreeSet::from([category]));
        assert_eq!(
            summary.by_category.keys().copied().collect::<Vec<_>>(),
            [category]
        );
        if category != Kind::Url {
            assert!(review.preview.contains("https://db.local/status"));
        }
    }
    let (root, store, project) = fixture("https://EXAMPLE.com:443/10.0.0.2");
    for selection in [options(&[]), options(&[Kind::CloudResource])] {
        let review = store
            .prepare_bundle(&project.project_id, project.revision, selection, || false)
            .unwrap();
        assert_eq!(review.project_revision, project.revision);
        assert!(review.manifest.pseudonym_mapping_id.is_none());
        assert!(review.preview.contains("https://EXAMPLE.com:443/10.0.0.2"));
        assert!(review.preview.contains("10.0.0.2 db.local"));
    }
    assert!(!root
        .path()
        .join(&project.project_id)
        .join("pseudonyms")
        .exists());
}

#[test]
fn policy_names_mapping_digest_and_high_confidence_secrets_stay_out_of_exports() {
    let (root, store, project) =
        fixture("api.private.example username=deploy password=SYNTHETIC_PRIVATE_VALUE");
    let mut selected = options(&[Kind::Host, Kind::Username]);
    selected
        .infrastructure
        .internal_domains
        .push("private.example".into());
    selected
        .infrastructure
        .hostnames
        .push("unmentioned-private-host".into());
    let review = store
        .prepare_bundle(&project.project_id, project.revision, selected, || false)
        .unwrap();
    let directory = root.path().join(&project.project_id);
    let private: serde_json::Value = serde_json::from_slice(
        &fs::read(
            directory
                .join("bundles")
                .join(&review.bundle_id)
                .join("private.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(private["schema_version"], 6);
    assert_eq!(
        private["infrastructure_policy"]["internal_domains"][0],
        "private.example"
    );
    let map = fs::read_to_string(directory.join("pseudonyms").join(format!(
        "{}.json",
        private["pseudonyms"]["id"].as_str().unwrap()
    )))
    .unwrap();
    assert!(map.contains("api.private.example"));
    assert!(!map.contains("SYNTHETIC_PRIVATE_VALUE"));
    let destination = tempfile::tempdir().unwrap();
    let exported = store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
    let mut public = serde_json::to_string(&review).unwrap();
    for file in fs::read_dir(exported.directory).unwrap() {
        public.push_str(&fs::read_to_string(file.unwrap().path()).unwrap());
    }
    for forbidden in [
        "private.example",
        "unmentioned-private-host",
        "SYNTHETIC_PRIVATE_VALUE",
        "prod.local",
        "db.local",
        private["pseudonyms"]["sha256"].as_str().unwrap(),
    ] {
        assert!(!public.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn review_excerpts_use_final_unicode_text_even_when_candidates_are_inside_replacements() {
    let (_root, store, project) = fixture(&format!("{}\nдо db.local 10.0.0.3 key=ordinary-value после https://db.local/?key=second-value конец password=SYNTHETIC_PRIVATE_VALUE", "я".repeat(14_000)));
    let selected = options(&[Kind::Host, Kind::Ip, Kind::Url]);
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            selected.clone(),
            || false,
        )
        .unwrap();
    assert!(review.preview_truncated);
    assert!(!review.preview.contains("ordinary-value"));
    assert_eq!(review.manifest.privacy.needs_review, 2);
    assert_eq!(review.findings.len(), 2);
    let excerpts = review
        .findings
        .iter()
        .map(|f| f.excerpt.as_str())
        .collect::<String>();
    assert!(excerpts.contains("ordinary-value"));
    assert!(excerpts.contains("HOST_") && excerpts.contains("IP_") && excerpts.contains("URL_"));
    for forbidden in [
        "db.local",
        "10.0.0.3",
        "second-value",
        "SYNTHETIC_PRIVATE_VALUE",
    ] {
        assert!(!excerpts.contains(forbidden));
    }
    assert!(review
        .findings
        .iter()
        .all(|f| f.excerpt.chars().count() <= 251));
    let destination = tempfile::tempdir().unwrap();
    assert!(store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            destination.path(),
            || false
        )
        .is_err());
    let ready = store
        .prepare_bundle(
            &project.project_id,
            review.project_revision,
            BundleOptions {
                redact_candidates: true,
                ..selected
            },
            || false,
        )
        .unwrap();
    assert_eq!(ready.manifest.privacy.needs_review, 0);
    assert!(ready.findings.is_empty());
    store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            ready.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
}

#[test]
fn late_cancellation_and_concurrent_project_edit_never_publish_a_partial_mapping() {
    for concurrent in [false, true] {
        let (root, store, project) = fixture("10.0.0.3 db.local");
        let directory = root.path().join(&project.project_id);
        let reached = Cell::new(false);
        let result = store.prepare_bundle(
            &project.project_id,
            project.revision,
            options(&[Kind::Host, Kind::Ip]),
            || {
                let private_ready = fs::read_dir(directory.join("bundles")).is_ok_and(|files| {
                    files
                        .flatten()
                        .any(|file| file.path().join("private.json").exists())
                });
                if private_ready && !reached.replace(true) {
                    if concurrent {
                        store
                            .update(
                                &project.project_id,
                                project.revision,
                                ProjectChange::Rename("Concurrent winner".into()),
                            )
                            .unwrap();
                    }
                    return !concurrent;
                }
                false
            },
        );
        let error = result.err().unwrap();
        assert!(reached.get());
        assert_eq!(
            error.kind(),
            if concurrent {
                std::io::ErrorKind::WouldBlock
            } else {
                std::io::ErrorKind::Other
            }
        );
        let current = store.open(&project.project_id).unwrap();
        assert_eq!(current.revision, project.revision + u64::from(concurrent));
        assert_eq!(
            current.name,
            if concurrent {
                "Concurrent winner"
            } else {
                &project.name
            }
        );
        assert!(current.pseudonyms.is_none());
        assert_eq!(current.sources, project.sources);
        assert_eq!(current.baselines, project.baselines);
        assert_eq!(fs::read_dir(directory.join("bundles")).unwrap().count(), 0);
        // Immutable orphan maps may remain; retry must follow Project state.
        let retry = store
            .prepare_bundle(
                &project.project_id,
                current.revision,
                options(&[Kind::Ip]),
                || false,
            )
            .unwrap();
        let map = store
            .bundle_pseudonyms(&project.project_id, &retry.bundle_id)
            .unwrap()
            .unwrap();
        assert!(map.originals("IP_0001").is_some());
        assert!(map.originals("HOST_0001").is_none());
    }
}

#[test]
fn aggregate_alias_budget_applies_across_message_fields_without_advancing_project() {
    let (root, store, project) = fixture("bootstrap");
    let messages: Vec<_> = (0..257)
        .map(|n| {
            let host: String = "abcdefghij"
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if n & (1 << i) != 0 {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect();
            serde_json::json!({"id": n + 1, "text": format!("{host}.local")})
        })
        .collect();
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "aliases",
            &project.sources[0].scope,
            Cursor::new(
                serde_json::to_vec(&serde_json::json!({"id":1,"messages":messages})).unwrap(),
            ),
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: "work".into(),
                snapshot_id: "aliases".into(),
            },
        )
        .unwrap();
    let error = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&[Kind::Host]),
            || false,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("alias limits"));
    assert!(!error.to_string().contains("abcdefghij"));
    let current = store.open(&project.project_id).unwrap();
    assert_eq!(current.revision, project.revision);
    assert!(current.pseudonyms.is_none());
    let directory = root.path().join(&project.project_id);
    assert_eq!(fs::read_dir(directory.join("bundles")).unwrap().count(), 0);
    assert!(!directory.join("pseudonyms").exists());
}

#[test]
fn refresh_keeps_existing_labels_evidence_and_historical_mapping_with_earlier_new_messages() {
    use tgsum_core::bundle::EvidenceRef;
    let (_root, store, project) = fixture("db.local");
    let first = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            options(&[Kind::Host]),
            || false,
        )
        .unwrap();
    let reference = first
        .preview
        .lines()
        .find_map(|line| line.strip_prefix("## Evidence "))
        .unwrap();
    let (id, revision) = reference.split_once('@').unwrap();
    let evidence = EvidenceRef {
        id: id.into(),
        revision: revision.into(),
    };
    store.snapshots(&project.project_id).unwrap().import_telegram("expanded", &project.sources[0].scope, Cursor::new(br#"{"id":1,"name":"Conversation prod.local","messages":[{"id":0,"text":"a.local"},{"id":1,"from":"Alice","text":"db.local"},{"id":2,"from":"Bob","text":"10.0.0.2 db.local"}]}"#)).unwrap();
    let current = store
        .update(
            &project.project_id,
            first.project_revision,
            ProjectChange::RecordSnapshot {
                source_id: "work".into(),
                snapshot_id: "expanded".into(),
            },
        )
        .unwrap();
    let second = store
        .prepare_bundle(
            &project.project_id,
            current.revision,
            options(&[Kind::Host]),
            || false,
        )
        .unwrap();
    assert!(second.preview.contains(reference));
    assert!(second.preview.contains("HOST_0003"));
    let historical = store
        .bundle_pseudonyms(&project.project_id, &first.bundle_id)
        .unwrap()
        .unwrap();
    let expanded = store
        .bundle_pseudonyms(&project.project_id, &second.bundle_id)
        .unwrap()
        .unwrap();
    assert_eq!(historical.originals("HOST_0002").unwrap(), ["db.local"]);
    assert_eq!(
        expanded.originals("HOST_0002"),
        historical.originals("HOST_0002")
    );
    assert!(historical.originals("HOST_0003").is_none());
    assert_eq!(expanded.originals("HOST_0003").unwrap(), ["a.local"]);
    assert_eq!(
        store
            .resolve_evidence(&project.project_id, &first.bundle_id, &evidence)
            .unwrap()
            .message
            .text,
        "db.local"
    );
    let destination = tempfile::tempdir().unwrap();
    assert_eq!(
        store
            .export_bundle(
                &project.project_id,
                &first.bundle_id,
                first.project_revision,
                destination.path(),
                || false
            )
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    store
        .export_bundle(
            &project.project_id,
            &second.bundle_id,
            second.project_revision,
            destination.path(),
            || false,
        )
        .unwrap();
}
