use std::io::Cursor;
use tgsum_core::project::{AnalysisOutcome, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::scope::{select_messages, DateBasis, DateRange, MessageFilter, SourceSelection};
use tgsum_core::snapshot::{SnapshotStore, SourceScope};

const DATA: &str = r#"{"id":1,"messages":[
{"id":10,"type":"service","action":"topic_created","title":"A"},
{"id":11,"reply_to_message_id":10,"date":"2026-06-18T23:30:00-03:00","text":"old"},
{"id":12,"reply_to_message_id":10,"date":"2026-06-19T11:00:00","text":"local date"},
{"id":20,"type":"service","action":"topic_created","title":"B"},
{"id":21,"reply_to_message_id":20,"text":"undated"}
]}"#;

fn dates(basis: DateBasis) -> MessageFilter {
    MessageFilter {
        dates: Some(DateRange {
            from: Some("2026-06-18".into()),
            through: Some("2026-06-18".into()),
            basis,
        }),
        ..Default::default()
    }
}

#[test]
fn topic_date_and_unknown_time_filters_have_explicit_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let snapshot = store
        .import_telegram("input", &SourceScope::telegram("a", "1"), Cursor::new(DATA))
        .unwrap();
    let mut selection = SourceSelection::default();
    assert_eq!(
        select_messages(&snapshot, &selection, None)
            .unwrap()
            .stats
            .selected,
        3
    );
    selection.filter = dates(DateBasis::SourceDate);
    let selected = select_messages(&snapshot, &selection, None).unwrap();
    assert_eq!(selected.messages[0].key.message_id, "11");
    assert_eq!(selected.stats.selected, 1);
    assert_eq!(selected.stats.excluded_unknown_dates, 1);
    selection.filter = dates(DateBasis::Utc);
    let selected = select_messages(&snapshot, &selection, None).unwrap();
    assert_eq!(selected.stats.selected, 0); // message 11 is June 19 in UTC
    assert_eq!(selected.stats.excluded_unknown_dates, 2);
    selection.filter.include_unknown_dates = true;
    assert_eq!(
        select_messages(&snapshot, &selection, None)
            .unwrap()
            .stats
            .included_unknown_dates,
        2
    );
    selection.filter.topic_ids = Some(vec!["10".into()]);
    assert_eq!(
        select_messages(&snapshot, &selection, None)
            .unwrap()
            .messages[0]
            .key
            .message_id,
        "12"
    );
    selection.filter.topic_ids = Some(vec![]);
    assert_eq!(
        select_messages(&snapshot, &selection, None)
            .unwrap()
            .stats
            .selected,
        0
    );
    selection.filter = MessageFilter::default();
    selection.filter.topic_ids = Some(vec!["10".into()]);
    selection.filter.include_service = true;
    assert_eq!(
        select_messages(&snapshot, &selection, None)
            .unwrap()
            .stats
            .selected,
        3
    );
    selection.enabled = false;
    assert_eq!(
        select_messages(&snapshot, &selection, None)
            .unwrap()
            .stats
            .selected,
        0
    );
    let mut invalid = dates(DateBasis::SourceDate);
    invalid.dates.as_mut().unwrap().from = Some("2026-07-01".into());
    assert!(invalid.validate().is_err());
    invalid.dates.as_mut().unwrap().from = Some("2026-02-30".into());
    assert!(invalid.validate().is_err());
}

#[test]
fn delta_includes_old_edits_and_newly_selected_scope_without_max_id_logic() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let scope = SourceScope::telegram("a", "1");
    let before = store
        .import_telegram("before", &scope, Cursor::new(DATA))
        .unwrap();
    let after = store
        .import_telegram(
            "after",
            &scope,
            Cursor::new(DATA.replace("\"old\"", "\"edited\"")),
        )
        .unwrap();
    let previous_filter = MessageFilter {
        topic_ids: Some(vec!["10".into()]),
        ..Default::default()
    };
    let selection = SourceSelection {
        only_changes: true,
        ..Default::default()
    };
    let delta = select_messages(&after, &selection, Some((&before, &previous_filter))).unwrap();
    assert_eq!(delta.stats.edited, 1);
    assert_eq!(delta.stats.created, 1); // already exported, newly selected topic B
    assert_eq!(delta.stats.unchanged, 1);
    assert_eq!(
        delta
            .messages
            .iter()
            .map(|m| m.key.message_id.as_str())
            .collect::<Vec<_>>(),
        ["11", "21"]
    );
    let empty = store
        .import_telegram("empty", &scope, Cursor::new(r#"{"id":1,"messages":[]}"#))
        .unwrap();
    let removed = select_messages(&empty, &selection, Some((&after, &selection.filter))).unwrap();
    assert_eq!(removed.stats.missing, 3);
    assert_eq!(removed.stats.deleted, 0);
    assert!(removed.messages.is_empty());
}

#[test]
fn only_successful_runs_advance_frozen_baselines() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(dir.path());
    let mut project = store.create("scope baseline").unwrap();
    let scope = SourceScope::telegram("a", "1");
    let snapshots = store.snapshots(&project.project_id).unwrap();
    for id in ["before", "after"] {
        snapshots
            .import_telegram(id, &scope, Cursor::new(DATA))
            .unwrap();
    }
    let source = ProjectSource {
        source_id: "chosen".into(),
        connector_id: "telegram_json".into(),
        scope,
        archive_path: None,
        latest_snapshot_id: Some("before".into()),
        selection: SourceSelection::default(),
    };
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(source),
        )
        .unwrap();
    for (id, outcome) in [
        ("failed", AnalysisOutcome::Failed),
        ("cancelled", AnalysisOutcome::Cancelled),
    ] {
        project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::BeginAnalysis { run_id: id.into() },
            )
            .unwrap();
        project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::FinishAnalysis {
                    run_id: id.into(),
                    outcome,
                },
            )
            .unwrap();
        assert!(project.baselines.is_empty());
    }
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::BeginAnalysis {
                run_id: "successful".into(),
            },
        )
        .unwrap();
    assert!(store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::BeginAnalysis {
                run_id: "overlap".into()
            }
        )
        .is_err());
    // An import and filter change during analysis must not change its frozen inputs.
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: "chosen".into(),
                snapshot_id: "after".into(),
            },
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "chosen".into(),
                selection: SourceSelection {
                    filter: dates(DateBasis::Utc),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    assert_eq!(
        project.analysis_run.as_ref().unwrap().inputs[0].snapshot_id,
        "before"
    );
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::FinishAnalysis {
                run_id: "successful".into(),
                outcome: AnalysisOutcome::Succeeded,
            },
        )
        .unwrap();
    assert_eq!(project.baselines[0].snapshot_id, "before");
    assert_eq!(project.baselines[0].filter, MessageFilter::default());
    assert_eq!(
        ProjectStore::new(dir.path())
            .open(&project.project_id)
            .unwrap(),
        project
    );
    assert!(store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::FinishAnalysis {
                run_id: "successful".into(),
                outcome: AnalysisOutcome::Succeeded
            }
        )
        .is_err());
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::BeginAnalysis {
                run_id: "next-failure".into(),
            },
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::FinishAnalysis {
                run_id: "next-failure".into(),
                outcome: AnalysisOutcome::Failed,
            },
        )
        .unwrap();
    assert_eq!(project.baselines[0].snapshot_id, "before");
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::BeginAnalysis {
                run_id: "removed".into(),
            },
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RemoveSource("chosen".into()),
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::FinishAnalysis {
                run_id: "removed".into(),
                outcome: AnalysisOutcome::Succeeded,
            },
        )
        .unwrap();
    assert!(project.sources.is_empty() && project.baselines.is_empty());
}
