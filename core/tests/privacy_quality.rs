//! Cross-stage qualification through public Project/Review/Export interfaces.
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::{fs, io::Cursor};
use tgsum_core::{
    analysis::AnalysisSpec,
    attachments::{AttachmentChoice, AttachmentSelection},
    bundle::BundleReview,
    privacy::{PrivacyPreset, PrivacyProfile},
    project::{Project, ProjectChange, ProjectSource, ProjectStore},
    scope::SourceSelection,
    snapshot::SourceScope,
};

struct Fixture {
    root: tempfile::TempDir,
    store: ProjectStore,
    project: Project,
}

fn fixture(text: &str, sender: Option<&str>, profile: PrivacyProfile) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("PRIVATE_ATTACHMENT_ROOT");
    fs::create_dir(&archive).unwrap();
    fs::write(archive.join("PRIVATE_ATTACHMENT_NAME.log"), text).unwrap();
    let store = ProjectStore::new(root.path().join("projects"));
    let project = store.create("Quality fixture").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    let mut message = json!({"id":1,"type":"message","text":text,"file":"PRIVATE_ATTACHMENT_NAME.log","file_name":"PRIVATE_ATTACHMENT_ORIGINAL.log"});
    if let Some(sender) = sender {
        message["from"] = sender.into();
        message["from_id"] = "user1".into();
    }
    let snapshot = store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "initial",
            &scope,
            Cursor::new(
                serde_json::to_vec(&json!({"id":1,"name":"Quality source","messages":[message]}))
                    .unwrap(),
            ),
        )
        .unwrap();
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
                selection: SourceSelection {
                    attachments: Some(AttachmentSelection {
                        // Match the native picker contract: macOS's temp root
                        // can contain /var -> /private/var before selection.
                        root: fs::canonicalize(archive).unwrap(),
                        files: vec![AttachmentChoice {
                            message_id: "1".into(),
                            position: 0,
                            expected: snapshot.messages[0].attachments[0].clone(),
                        }],
                    }),
                    ..Default::default()
                },
            }),
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Privacy(profile),
        )
        .unwrap();
    Fixture {
        root,
        store,
        project,
    }
}

fn work() -> PrivacyProfile {
    let mut options = PrivacyPreset::Work.options();
    options.redact_candidates = true;
    PrivacyProfile {
        preset: PrivacyPreset::Work,
        options,
        custom_terms: Default::default(),
    }
}

impl Fixture {
    fn review(&self) -> BundleReview {
        let current = self.store.open(&self.project.project_id).unwrap();
        self.store
            .prepare_saved_bundle(&current.project_id, current.revision, || false)
            .unwrap()
    }
}

#[test]
fn attachment_choices_do_not_leak_private_names_into_project_or_analysis_diagnostics() {
    let f = fixture("password=SYNTHETIC_PRIVATE_VALUE", None, work());
    let selected = f.project.sources[0].selection.attachments.as_ref().unwrap();
    for diagnostic in [
        format!("{selected:?}"),
        format!("{:?}", selected.files[0]),
        format!("{:?}", f.project),
    ] {
        assert!(
            !diagnostic.contains("PRIVATE_ATTACHMENT_"),
            "attachment policy diagnostic exposed a private name"
        );
    }
    let review = f.review();
    let ticket = f
        .store
        .begin_analysis(
            &f.project.project_id,
            &review.bundle_id,
            review.project_revision,
            AnalysisSpec {
                agent: "synthetic".into(),
                agent_version: "1".into(),
                isolation_profile: "synthetic-offline".into(),
                destination: "synthetic_local".into(),
                model: "fixture-model".into(),
                recipe: "summary".into(),
                recipe_version: 1,
            },
            || false,
        )
        .unwrap();
    let stored = f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .unwrap();
    for diagnostic in [
        format!("{ticket:?}"),
        format!("{:?}", ticket.request()),
        format!("{stored:?}"),
    ] {
        assert!(
            !diagnostic.contains("PRIVATE_ATTACHMENT_"),
            "analysis diagnostic exposed an attachment name"
        );
        assert!(!diagnostic.contains("SYNTHETIC_PRIVATE_VALUE"));
    }
    // Private serialization deliberately retains source bindings for recovery;
    // removing them to make diagnostics safe would break that contract.
    assert!(serde_json::to_string(ticket.request())
        .unwrap()
        .contains("PRIVATE_ATTACHMENT_NAME.log"));
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema_version: u8,
    description: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    kind: CaseKind,
    sender: Option<String>,
    input: String,
    expected: String,
    #[serde(default)]
    terms: Vec<serde_json::Value>,
    #[serde(default)]
    keep: Vec<String>,
    counts: Counts,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaseKind {
    Positive,
    Negative,
    KnownLimit,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    secrets: usize,
    pii: BTreeMap<String, usize>,
    infrastructure: BTreeMap<String, usize>,
    terms: usize,
}

#[test]
fn saved_pipeline_corpus_matches_messages_files_counts_and_repeat_runs() {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/privacy-pipeline-v1.json")).unwrap();
    assert_eq!(corpus.schema_version, 1);
    assert!(corpus.description.starts_with("Synthetic"));
    let mut ids = BTreeSet::new();
    let mut covered_pii = BTreeSet::new();
    let mut covered_infrastructure = BTreeSet::new();
    let (mut positive, mut negative, mut known_limit) = (0, 0, 0);
    for case in corpus.cases {
        assert!(ids.insert(case.id.clone()), "duplicate pipeline case");
        let mut profile = work();
        profile.options.keep_values = case.keep;
        profile.custom_terms = serde_json::from_value(json!({"entries":case.terms})).unwrap();
        let f = fixture(&case.input, case.sender.as_deref(), profile);
        let review = f.review();
        assert_eq!(review.manifest.messages, 1, "{}", case.id);
        assert_eq!(review.manifest.included_attachments, 1, "{}", case.id);
        assert_eq!(
            review.manifest.privacy.redacted, case.counts.secrets,
            "{}: secrets",
            case.id
        );
        assert_eq!(
            review.manifest.privacy.needs_review, 0,
            "{}: pending",
            case.id
        );
        let pii = review.manifest.pii.as_ref().unwrap();
        let infrastructure = review.manifest.infrastructure.as_ref().unwrap();
        assert_eq!(
            serde_json::to_value(&pii.by_category).unwrap(),
            json!(case.counts.pii),
            "{}: PII",
            case.id
        );
        assert_eq!(
            serde_json::to_value(&infrastructure.by_category).unwrap(),
            json!(case.counts.infrastructure),
            "{}: infrastructure",
            case.id
        );
        assert_eq!(
            review
                .manifest
                .custom_terms
                .as_ref()
                .map_or(0, |s| s.replacements),
            case.counts.terms,
            "{}: dictionary",
            case.id
        );
        covered_pii.extend(case.counts.pii.keys().cloned());
        covered_infrastructure.extend(case.counts.infrastructure.keys().cloned());
        let items = f
            .store
            .review_items(
                &f.project.project_id,
                &review.bundle_id,
                review.project_revision,
                0,
                || false,
            )
            .unwrap();
        assert_eq!(items.total, 2);
        for item in items.items {
            let comparison = f
                .store
                .preview_evidence(
                    &f.project.project_id,
                    &review.bundle_id,
                    review.project_revision,
                    &item.reference,
                    || false,
                )
                .unwrap();
            assert_eq!(
                comparison.before.as_deref(),
                Some(case.input.as_str()),
                "{}: {} original",
                case.id,
                item.kind
            );
            assert_eq!(
                comparison.after.trim_end_matches('\n'),
                case.expected,
                "{}: {} output",
                case.id,
                item.kind
            );
            assert!(!comparison.before_truncated && !comparison.after_truncated);
        }
        let reopened = ProjectStore::new(f.root.path().join("projects"));
        let repeat = reopened
            .prepare_saved_bundle(&f.project.project_id, review.project_revision, || false)
            .unwrap();
        assert_eq!(
            repeat.project_revision, review.project_revision,
            "{}: no mapping churn",
            case.id
        );
        assert_eq!(repeat.preview, review.preview, "{}: repeat output", case.id);
        assert_eq!(
            serde_json::to_value(&repeat.manifest).unwrap(),
            serde_json::to_value(&review.manifest).unwrap(),
            "{}: repeat metadata",
            case.id
        );
        let out = tempfile::tempdir().unwrap();
        let exported = reopened
            .export_bundle(
                &f.project.project_id,
                &repeat.bundle_id,
                repeat.project_revision,
                out.path(),
                || false,
            )
            .unwrap();
        let mut names = BTreeSet::from(["manifest.json".to_owned()]);
        names.extend(exported.files.iter().map(|file| file.name.clone()));
        let actual_names: BTreeSet<_> = fs::read_dir(&exported.directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            actual_names, names,
            "{}: only public allowlist exported",
            case.id
        );
        for name in names {
            let text = fs::read_to_string(exported.directory.join(name)).unwrap();
            for forbidden in [
                "PRIVATE_ATTACHMENT_",
                "SYNTHETIC_SECRET",
                "SYNTHETIC_PRIVATE_VALUE",
                "keep_values",
                "evidence-key.bin",
                "source_sha256",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{}: private canary/metadata exported",
                    case.id
                );
            }
        }
        match case.kind {
            CaseKind::Positive => {
                positive += 1;
                assert_ne!(case.input, case.expected);
            }
            CaseKind::Negative => {
                negative += 1;
                assert_eq!(case.input, case.expected);
            }
            CaseKind::KnownLimit => {
                known_limit += 1;
                assert_eq!(case.input, case.expected);
            }
        }
    }
    assert_eq!((positive, negative, known_limit), (7, 3, 2));
    assert_eq!(
        covered_pii,
        ["participants", "emails", "phones", "usernames"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert_eq!(
        covered_infrastructure,
        [
            "ip",
            "host",
            "domain",
            "url",
            "username",
            "path",
            "cloud_resource"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    eprintln!("Synthetic saved pipeline: 12 golden cases x message/file; 7 positive, 3 negative, 2 documented limits; exact counts and restart/repeat/export checks");
}
