use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Cursor};
use tgsum_core::analysis::{AnalysisSpec, Completion, FailureCode, RunTicket};
use tgsum_core::bundle::{BundleOptions, EvidenceRef};
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::{CoverageLevel, SourceScope};

struct Fixture {
    root: tempfile::TempDir,
    store: ProjectStore,
    project: Project,
    bundle: String,
    evidence: EvidenceRef,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let mut project = store.create("Synthetic result project").unwrap();
    for chat in [1, 2] {
        let scope = SourceScope::telegram("synthetic", chat.to_string());
        let snapshot = format!("snapshot-{chat}");
        store
            .snapshots(&project.project_id)
            .unwrap()
            .import_telegram(
                &snapshot,
                &scope,
                Cursor::new(format!(
                    r#"{{"id":{chat},"messages":[{{"id":1,"text":"Scoped context {chat}"}}]}}"#
                )),
            )
            .unwrap();
        project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: format!("source-{chat}"),
                    connector_id: "telegram_json".into(),
                    scope,
                    archive_path: None,
                    latest_snapshot_id: Some(snapshot),
                    selection: Default::default(),
                }),
            )
            .unwrap();
    }
    let bundle = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions {
                redact_candidates: true,
            },
            || false,
        )
        .unwrap();
    let reference = bundle
        .preview
        .lines()
        .find_map(|l| l.strip_prefix("## Evidence "))
        .unwrap();
    let (id, revision) = reference.split_once('@').unwrap();
    Fixture {
        root,
        store,
        project,
        bundle: bundle.bundle_id,
        evidence: EvidenceRef {
            id: id.into(),
            revision: revision.into(),
        },
    }
}
fn spec() -> AnalysisSpec {
    AnalysisSpec {
        agent: "codex".into(),
        agent_version: "0.155.1".into(),
        isolation_profile: "synthetic-offline".into(),
        destination: "synthetic_local".into(),
        model: "fixture-model".into(),
        recipe: "summary".into(),
        recipe_version: 1,
    }
}
impl Fixture {
    fn begin(&self) -> RunTicket {
        self.store
            .begin_analysis(
                &self.project.project_id,
                &self.bundle,
                self.project.revision,
                spec(),
                || false,
            )
            .unwrap()
    }
    fn answer(&self) -> Answer {
        Answer {
            summary: "PRIVATE_SYNTHETIC_RESULT".into(),
            evidence: vec![self.evidence.clone()],
        }
    }
    fn path(&self, ticket: &RunTicket, name: &str) -> std::path::PathBuf {
        self.root
            .path()
            .join(&self.project.project_id)
            .join("analyses")
            .join(&ticket.request().run_id)
            .join(name)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    summary: String,
    evidence: Vec<EvidenceRef>,
}
fn validate(answer: &Answer) -> io::Result<Vec<EvidenceRef>> {
    if answer.summary.trim().is_empty() || answer.evidence.is_empty() {
        return Err(io::Error::other("summary needs text and evidence"));
    }
    Ok(answer.evidence.clone())
}

#[test]
fn recent_runs_are_bounded_and_listing_does_not_promote_or_read_results() {
    let f = fixture();
    assert!(f
        .store
        .recent_analysis_ids(&f.project.project_id, 20)
        .unwrap()
        .is_empty());
    let a = f.begin();
    let b = f.begin();
    let c = f.begin();
    f.store.fail_analysis(&a, FailureCode::Agent).unwrap();
    // A damaged request remains visible as an ID so a UI can report it as
    // unavailable; listing must not parse arbitrary result payloads.
    fs::write(f.path(&b, "request.json"), b"broken").unwrap();
    let all = f
        .store
        .recent_analysis_ids(&f.project.project_id, 20)
        .unwrap();
    assert_eq!(all.len(), 3);
    for run in [&a, &b, &c] {
        assert!(all.contains(&run.request().run_id));
    }
    assert_eq!(
        f.store
            .recent_analysis_ids(&f.project.project_id, 2)
            .unwrap()
            .len(),
        2
    );
    for limit in [0, 101, usize::MAX] {
        assert!(f
            .store
            .recent_analysis_ids(&f.project.project_id, limit)
            .is_err());
    }
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
    assert!(f
        .store
        .read_analysis(&f.project.project_id, &c.request().run_id)
        .unwrap()
        .completion
        .is_none());
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            f.path(&a, "request.json")
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("run-linked"),
        )
        .unwrap();
        assert!(f
            .store
            .recent_analysis_ids(&f.project.project_id, 20)
            .is_err());
    }
}

#[test]
fn reviewed_result_survives_restart_and_commits_all_baselines_once() {
    let f = fixture();
    let ticket = f.begin();
    assert_eq!(ticket.request().coverage.len(), 2);
    assert!(ticket
        .request()
        .coverage
        .iter()
        .all(|c| c.level == CoverageLevel::Unknown));
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
    let pending = f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .unwrap();
    assert!(pending.completion.is_none() && pending.committed_revision.is_none());
    let receipt = f
        .store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .unwrap();
    assert!(f
        .store
        .open(&f.project.project_id)
        .unwrap()
        .baselines
        .is_empty());
    let saved = f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .unwrap();
    assert!(saved.committed_revision.is_none());
    assert!(!format!("{saved:?}").contains("PRIVATE_SYNTHETIC_RESULT"));
    drop(ticket);
    let store = ProjectStore::new(f.root.path());
    let ticket = store
        .resume_analysis(&f.project.project_id, &receipt.run_id)
        .unwrap();
    assert_eq!(
        store.commit_analysis(&ticket, || false).unwrap(),
        f.project.revision + 1
    );
    let committed = store.open(&f.project.project_id).unwrap();
    assert_eq!(committed.baselines.len(), 2);
    assert!(committed
        .baselines
        .iter()
        .all(|b| b.analysis_id == receipt.run_id));
    assert_eq!(
        committed.analysis_run.as_ref().unwrap().result.as_ref(),
        Some(&receipt)
    );
    let later = store
        .update(
            &f.project.project_id,
            committed.revision,
            ProjectChange::Rename("later".into()),
        )
        .unwrap();
    assert_eq!(
        store.commit_analysis(&ticket, || false).unwrap(),
        committed.revision
    );
    assert_eq!(store.open(&f.project.project_id).unwrap(), later);
    let saved = store
        .read_analysis(&f.project.project_id, &receipt.run_id)
        .unwrap();
    assert_eq!(saved.committed_revision, Some(committed.revision));
    let Completion::Validated { value, evidence } = saved.completion.unwrap() else {
        panic!()
    };
    assert_eq!(value["summary"], "PRIVATE_SYNTHETIC_RESULT");
    assert_eq!(evidence.as_slice(), std::slice::from_ref(&f.evidence));
    assert!(store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .is_err());
}

#[test]
fn mapping_reset_preserves_saved_result_binding_and_rejects_an_inflight_commit() {
    use tgsum_core::pseudonyms::{PseudonymCategory, PseudonymInput};
    let mut f = fixture();
    let legacy_bundle = f.bundle.clone();
    f.project = f
        .store
        .assign_pseudonyms(
            &f.project.project_id,
            f.project.revision,
            &[PseudonymInput {
                category: PseudonymCategory::Person,
                identity: "synthetic-user",
                original: "Synthetic original",
            }],
        )
        .unwrap()
        .project;
    f.bundle = f
        .store
        .prepare_bundle(
            &f.project.project_id,
            f.project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap()
        .bundle_id;
    assert!(f
        .store
        .bundle_pseudonyms(&f.project.project_id, &legacy_bundle)
        .unwrap()
        .is_none());
    let completed = f.begin();
    let mut answer = f.answer();
    answer.summary = "PERSON_0001 performed the synthetic action".into();
    let receipt = f
        .store
        .save_analysis_result(&completed, &answer, validate, || false)
        .unwrap();
    f.store.commit_analysis(&completed, || false).unwrap();
    let saved = f
        .store
        .read_analysis(&f.project.project_id, &receipt.run_id)
        .unwrap();
    let committed_revision = saved.committed_revision;
    let saved_bundle = f.bundle.clone();
    f.project = f.store.open(&f.project.project_id).unwrap();
    f.bundle = f
        .store
        .prepare_bundle(
            &f.project.project_id,
            f.project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap()
        .bundle_id;
    let inflight = f.begin();
    f.store
        .save_analysis_result(&inflight, &answer, validate, || false)
        .unwrap();
    let reset = f
        .store
        .reset_pseudonyms(&f.project.project_id, f.project.revision)
        .unwrap();
    assert!(f.store.commit_analysis(&inflight, || false).is_err());
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), reset);
    let reopened = ProjectStore::new(f.root.path());
    let old_result = reopened
        .read_analysis(&f.project.project_id, &receipt.run_id)
        .unwrap();
    assert_eq!(old_result.committed_revision, committed_revision);
    let Completion::Validated { value, evidence } = old_result.completion.unwrap() else {
        panic!()
    };
    assert_eq!(value["summary"], answer.summary);
    assert_eq!(evidence, answer.evidence);
    assert_eq!(reset.analysis_run.unwrap().result.as_ref(), Some(&receipt));
    let old_mapping = reopened
        .bundle_pseudonyms(&f.project.project_id, &saved_bundle)
        .unwrap()
        .unwrap();
    assert_eq!(
        old_mapping.originals("PERSON_0001").unwrap(),
        ["Synthetic original"]
    );
    assert!(reopened
        .load_pseudonyms(&f.project.project_id, reset.pseudonyms.as_ref().unwrap())
        .unwrap()
        .originals("PERSON_0001")
        .is_none());
}

#[test]
fn failures_cancel_rejected_evidence_and_oversize_never_advance_baseline() {
    let f = fixture();
    for code in [
        FailureCode::Agent,
        FailureCode::Authentication,
        FailureCode::Transport,
        FailureCode::InvalidResult,
        FailureCode::Interrupted,
    ] {
        let ticket = f.begin();
        f.store.fail_analysis(&ticket, code).unwrap();
        assert!(f.store.commit_analysis(&ticket, || false).is_err());
        assert!(f
            .store
            .save_analysis_result(&ticket, &f.answer(), validate, || false)
            .is_err());
    }
    let ticket = f.begin();
    f.store.cancel_analysis(&ticket).unwrap();
    assert!(f.store.commit_analysis(&ticket, || false).is_err());
    assert!(f.store.fail_analysis(&ticket, FailureCode::Agent).is_err());
    let ticket = f.begin();
    assert!(f
        .store
        .save_analysis_result(&ticket, &f.answer(), validate, || true)
        .is_err());
    let mut answer = f.answer();
    answer.evidence[0].revision.push('0');
    assert!(f
        .store
        .save_analysis_result(&ticket, &answer, validate, || false)
        .is_err());
    answer.evidence = vec![];
    assert!(f
        .store
        .save_analysis_result(&ticket, &answer, validate, || false)
        .is_err());
    answer.evidence = vec![f.evidence.clone(), f.evidence.clone()];
    assert!(f
        .store
        .save_analysis_result(&ticket, &answer, validate, || false)
        .is_err());
    answer = f.answer();
    answer.summary.clear();
    assert!(f
        .store
        .save_analysis_result(&ticket, &answer, validate, || false)
        .is_err());
    answer.summary = "x".repeat(1024 * 1024);
    assert!(f
        .store
        .save_analysis_result(&ticket, &answer, validate, || false)
        .is_err());
    assert!(!f.path(&ticket, "completion.json").exists());
    f.store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .unwrap();
    assert!(f.store.commit_analysis(&ticket, || true).is_err());
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
}

#[test]
fn no_claims_recipe_can_store_empty_findings_without_inventing_evidence() {
    let f = fixture();
    let mut descriptor = spec();
    descriptor.recipe = "actions".into();
    let ticket = f
        .store
        .begin_analysis(
            &f.project.project_id,
            &f.bundle,
            f.project.revision,
            descriptor,
            || false,
        )
        .unwrap();
    let value = serde_json::json!({"actions":[]});
    f.store
        .save_analysis_result(
            &ticket,
            &value,
            |answer| {
                assert_eq!(answer["actions"], serde_json::json!([]));
                Ok(vec![])
            },
            || false,
        )
        .unwrap();
    f.store.commit_analysis(&ticket, || false).unwrap();
    let result = f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .unwrap();
    assert_eq!(result.request.coverage.len(), 2);
    assert!(result.committed_revision.is_some());
}

#[test]
fn stale_review_and_concurrent_commits_cannot_overwrite_project_or_other_result() {
    let f = fixture();
    let a = f.begin();
    let b = f.begin();
    for ticket in [&a, &b] {
        f.store
            .save_analysis_result(ticket, &f.answer(), validate, || false)
            .unwrap();
    }
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            barrier.wait();
            f.store.commit_analysis(&a, || false)
        });
        let two = scope.spawn(|| {
            barrier.wait();
            f.store.commit_analysis(&b, || false)
        });
        [one.join().unwrap(), two.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let project = f.store.open(&f.project.project_id).unwrap();
    assert_eq!(project.revision, f.project.revision + 1);
    assert_eq!(project.baselines.len(), 2);
    assert!(f
        .store
        .begin_analysis(
            &project.project_id,
            &f.bundle,
            project.revision,
            spec(),
            || false
        )
        .is_err());
    let loser = if results[0].is_ok() { &b } else { &a };
    assert!(f
        .store
        .read_analysis(&project.project_id, &loser.request().run_id)
        .unwrap()
        .committed_revision
        .is_none());
    assert!(f.store.commit_analysis(loser, || false).is_err());
    let f = fixture();
    let ticket = f.begin();
    let changed = f
        .store
        .update(
            &f.project.project_id,
            f.project.revision,
            ProjectChange::RemoveSource("source-1".into()),
        )
        .unwrap();
    f.store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .unwrap();
    assert!(f.store.commit_analysis(&ticket, || false).is_err());
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), changed);
}

#[test]
fn changed_ticket_bundle_and_committed_result_are_detected_without_payload_errors() {
    let f = fixture();
    let ticket = f.begin();
    let request_path = f.path(&ticket, "request.json");
    let request = fs::read(&request_path).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&request).unwrap();
    changed["spec"]["destination"] = "other.example".into();
    fs::write(&request_path, changed.to_string()).unwrap();
    assert!(f
        .store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .is_err());
    fs::write(&request_path, &request).unwrap();
    let public = f
        .root
        .path()
        .join(&f.project.project_id)
        .join("bundles")
        .join(&f.bundle)
        .join("context");
    let file = fs::read_dir(&public)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|v| v == "md"))
        .unwrap();
    let original = fs::read(&file).unwrap();
    fs::write(&file, "TAMPERED_PAYLOAD").unwrap();
    let error = f
        .store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .unwrap_err();
    assert!(!error.to_string().contains("TAMPERED_PAYLOAD"));
    fs::write(&file, original).unwrap();
    f.store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .unwrap();
    let result_path = f.path(&ticket, "completion.json");
    let original = fs::read(&result_path).unwrap();
    let changed = String::from_utf8(original.clone())
        .unwrap()
        .replace("PRIVATE_SYNTHETIC_RESULT", "TAMPERED_RESULT");
    fs::write(&result_path, changed).unwrap();
    // Corruption between validation and the baseline commit is rejected too.
    assert!(f.store.commit_analysis(&ticket, || false).is_err());
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
    fs::write(&result_path, &original).unwrap();
    f.store.commit_analysis(&ticket, || false).unwrap();
    let mut altered_request: serde_json::Value = serde_json::from_slice(&request).unwrap();
    altered_request["spec"]["model"] = "wrong-model".into();
    fs::write(&request_path, altered_request.to_string()).unwrap();
    assert!(f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .is_err());
    fs::write(&request_path, &request).unwrap();
    fs::write(
        &result_path,
        String::from_utf8(original)
            .unwrap()
            .replace("PRIVATE_SYNTHETIC_RESULT", "TAMPERED_RESULT"),
    )
    .unwrap();
    assert!(f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .is_err());
    assert!(f.store.commit_analysis(&ticket, || false).is_err());
    fs::remove_file(&result_path).unwrap();
    assert!(f
        .store
        .read_analysis(&f.project.project_id, &ticket.request().run_id)
        .is_err());
}

#[cfg(unix)]
#[test]
fn analysis_storage_is_private_and_symlink_replacements_are_rejected() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = fixture();
    let ticket = f.begin();
    let request = f.path(&ticket, "request.json");
    for path in [
        request.parent().unwrap(),
        request.parent().unwrap().parent().unwrap(),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert_eq!(
        fs::metadata(&request).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let outside = tempfile::tempdir().unwrap();
    let copy = outside.path().join("request.json");
    fs::copy(&request, &copy).unwrap();
    fs::remove_file(&request).unwrap();
    symlink(&copy, &request).unwrap();
    assert!(f
        .store
        .resume_analysis(&f.project.project_id, &ticket.request().run_id)
        .is_err());
    assert!(f
        .store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .is_err());
    fs::remove_file(&request).unwrap();
    fs::copy(&copy, &request).unwrap();
    let output = outside.path().join("output");
    fs::write(&output, "sentinel").unwrap();
    symlink(&output, f.path(&ticket, "completion.json")).unwrap();
    assert!(f
        .store
        .save_analysis_result(&ticket, &f.answer(), validate, || false)
        .is_err());
    assert_eq!(fs::read_to_string(&output).unwrap(), "sentinel");
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
}
