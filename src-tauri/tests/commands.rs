//! Drives the Tauri commands on the mock runtime with the same JSON the UI
//! (`ui/app.js`) sends, so argument names and response shapes stay in sync.

use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

fn window() -> WebviewWindow<tauri::test::MockRuntime> {
    let directory = tempfile::tempdir().unwrap();
    let app = tgsum_app::app(
        command_builder().manage(tgsum_core::project::ProjectStore::new(directory.path())),
    )
    .build(mock_context(noop_assets()))
    .unwrap();
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap()
}

// MockRuntime::build does not run the real event-loop setup callback, so
// command state must be supplied explicitly, as ProjectStore already is below.
fn command_builder() -> tauri::Builder<tauri::test::MockRuntime> {
    mock_builder().manage(tgsum_app::analysis::AnalysisState::default())
}

fn invoke(
    w: &WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
    get_ipc_response(
        w,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // The app's own origin, so the call counts as local.
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().unwrap())
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/sample-export.json")
}

#[test]
fn index_then_export_like_the_ui_does() {
    let w = window();
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("result.json");
    std::fs::copy(fixture(), &export).unwrap();

    // Dropping the export *folder* resolves to result.json inside it.
    let idx = invoke(&w, "index_export", json!({ "path": dir.path() })).unwrap();
    assert_eq!(idx["fileName"], "result.json");
    assert_eq!(idx["path"], export.display().to_string());
    assert_eq!(
        idx["outDir"],
        dir.path().join("tgsum-output").display().to_string()
    );
    assert_eq!(idx["size"], std::fs::metadata(&export).unwrap().len());
    let chats = idx["chats"].as_array().unwrap();
    assert_eq!(chats.len(), 2);
    assert_eq!(chats[0]["chatId"], "111");
    assert_eq!(chats[0]["type"], "personal_chat");
    assert_eq!(chats[0]["count"], 2);
    assert_eq!(chats[0]["firstDate"], "2026-06-18T10:00:00");
    assert_eq!(
        chats[1]["topics"][1],
        json!({
            "topicId": "100", "title": "Bugs", "count": 2,
            "firstDate": "2026-06-18T09:31:00", "lastDate": "2026-06-20T09:31:00",
        })
    );

    let out = dir.path().join("out");
    let res = invoke(
        &w,
        "export_selection",
        json!({
            "path": idx["path"],
            "selection": [{ "chatId": "222", "topicIds": ["100"] }, { "chatId": "111" }],
            "outDir": out,
            "maxTokens": 90000,
        }),
    )
    .unwrap();
    assert_eq!(res["outDir"], out.display().to_string());
    let names: Vec<&str> = res["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Direct with Bob.md", "Pilot Forum__Bugs.md"]);
    for f in res["files"].as_array().unwrap() {
        let path = PathBuf::from(f["path"].as_str().unwrap());
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            f["bytes"].as_u64().unwrap()
        );
    }
    let md = std::fs::read_to_string(out.join("Pilot Forum__Bugs.md")).unwrap();
    assert!(md.contains("[09:31] Bob: found a bug"), "{md}");
}

#[test]
fn no_split_budget_from_the_ui_is_accepted() {
    let w = window();
    let dir = tempfile::tempdir().unwrap();
    let res = invoke(
        &w,
        "export_selection",
        json!({
            "path": fixture(),
            "selection": [{ "chatId": "111" }],
            "outDir": dir.path(),
            "maxTokens": 9007199254740991u64, // Number.MAX_SAFE_INTEGER
        }),
    )
    .unwrap();
    assert_eq!(res["files"].as_array().unwrap().len(), 1);
}

#[test]
fn errors_come_back_as_kind_and_message() {
    let w = window();
    let dir = tempfile::tempdir().unwrap();
    let err = invoke(&w, "index_export", json!({ "path": dir.path() })).unwrap_err();
    assert_eq!(err["kind"], "failed");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .starts_with("Файл не найден"),
        "{err}"
    );

    let bad = dir.path().join("broken.json");
    std::fs::write(&bad, "<html>").unwrap();
    let err = invoke(&w, "index_export", json!({ "path": bad })).unwrap_err();
    assert_eq!(err["kind"], "failed");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("не похоже на выгрузку Telegram"),
        "{err}"
    );
}

#[test]
fn cancel_job_is_harmless_when_idle() {
    let w = window();
    assert_eq!(invoke(&w, "cancel_job", json!({})).unwrap(), Value::Null);
}

#[cfg(all(feature = "analysis-fixtures", debug_assertions))]
mod analysis_flow {
    use super::*;
    use tgsum_core::analysis::Completion;
    use tgsum_core::bundle::BundleOptions;
    use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
    use tgsum_core::snapshot::SourceScope;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: ProjectStore,
        project: Project,
        bundle: String,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = ProjectStore::new(dir.path());
            let p = store.create("Local synthetic analysis").unwrap();
            let scope = SourceScope::telegram("synthetic", "111");
            store
                .snapshots(&p.project_id)
                .unwrap()
                .import_telegram(
                    "fixture",
                    &scope,
                    std::io::Cursor::new(
                        r#"{"id":111,"messages":[{"id":1,"text":"Synthetic message"}]}"#,
                    ),
                )
                .unwrap();
            let project = store
                .update(
                    &p.project_id,
                    p.revision,
                    ProjectChange::Source(ProjectSource {
                        source_id: "source".into(),
                        connector_id: "telegram_json".into(),
                        scope,
                        archive_path: None,
                        latest_snapshot_id: Some("fixture".into()),
                        selection: Default::default(),
                    }),
                )
                .unwrap();
            let bundle = store
                .prepare_bundle(
                    &project.project_id,
                    project.revision,
                    BundleOptions {
                        redact_candidates: true,
                    },
                    || false,
                )
                .unwrap()
                .bundle_id;
            Self {
                _dir: dir,
                store,
                project,
                bundle,
            }
        }
        fn window(&self) -> WebviewWindow<tauri::test::MockRuntime> {
            let app = tgsum_app::app(
                mock_builder()
                    .manage(self.store.clone())
                    .manage(tgsum_app::analysis::AnalysisState::synthetic()),
            )
            .build(mock_context(noop_assets()))
            .unwrap();
            WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .unwrap()
        }
        fn prepare(&self, w: &WebviewWindow<tauri::test::MockRuntime>, model: &str) -> Value {
            invoke(w,"prepare_project_analysis",json!({"projectId":self.project.project_id,"bundleId":self.bundle,"expectedRevision":self.project.revision,
                "options":{"executable":"/NEVER_EXECUTE","auth_file":"/NEVER_OPEN","model":model,"recipe":"summary","destination":"local_fixture"}})).unwrap()
        }
        fn read(&self, w: &WebviewWindow<tauri::test::MockRuntime>, id: &Value) -> Value {
            invoke(
                w,
                "read_project_analysis",
                json!({"projectId":self.project.project_id,"runId":id}),
            )
            .unwrap()
        }
    }

    #[test]
    fn reviewed_run_is_single_use_and_persists_across_application_restart() {
        let f = Fixture::new();
        let w = f.window();
        let catalog = invoke(&w, "analysis_catalog", json!({})).unwrap();
        assert_eq!(catalog["fixtures"], true);
        assert_eq!(catalog["recipes"].as_array().unwrap().len(), 6);
        let reviewed = f.prepare(&w, "fixture-success");
        let id = &reviewed["run_id"];
        assert_eq!(reviewed["spec"]["destination"], "local_fixture");
        assert_eq!(reviewed["coverage"][0]["messages"], 1);
        assert!(invoke(&w, "run_project_analysis", json!({"runId":"run-wrong"})).is_err());
        let output = invoke(&w, "run_project_analysis", json!({"runId":id})).unwrap();
        assert_eq!(output["state"], "succeeded");
        assert!(output["result"]["sections"][0]["claims"][0]["text"]
            .as_str()
            .unwrap()
            .contains("<img"));
        assert!(invoke(&w, "run_project_analysis", json!({"runId":id})).is_err());
        let p = f.store.open(&f.project.project_id).unwrap();
        assert_eq!(p.revision, f.project.revision + 1);
        assert_eq!(p.baselines.len(), 1);
        let restarted = f.window();
        assert_eq!(f.read(&restarted, id), output);
        assert!(invoke(&restarted, "run_project_analysis", json!({"runId":id})).is_err());
        let entries = invoke(
            &restarted,
            "list_project_analyses",
            json!({"projectId":p.project_id}),
        )
        .unwrap();
        assert_eq!(entries[0]["state"], "succeeded");
    }

    #[test]
    fn review_changes_and_agent_failure_preserve_baselines() {
        let f = Fixture::new();
        let w = f.window();
        let first = f.prepare(&w, "fixture-success");
        let second = f.prepare(&w, "fixture-failure");
        assert_eq!(f.read(&w, &first["run_id"])["state"], "cancelled");
        assert!(invoke(&w, "run_project_analysis", json!({"runId":first["run_id"]})).is_err());
        let failed = invoke(
            &w,
            "run_project_analysis",
            json!({"runId":second["run_id"]}),
        )
        .unwrap();
        assert_eq!(failed["state"], "failed");
        assert_eq!(failed["failure"], "agent");
        assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
        let third = f.prepare(&w, "fixture-success");
        invoke(&w,"update_project",json!({"projectId":f.project.project_id,"expectedRevision":f.project.revision,"change":{"kind":"rename","value":"Edited after review"}})).unwrap();
        assert_eq!(f.read(&w, &third["run_id"])["state"], "cancelled");
        assert!(invoke(&w, "run_project_analysis", json!({"runId":third["run_id"]})).is_err());
        assert!(f
            .store
            .open(&f.project.project_id)
            .unwrap()
            .baselines
            .is_empty());
    }

    #[test]
    fn running_job_rejects_project_edit_and_can_be_cancelled() {
        let f = Fixture::new();
        let w = f.window();
        let prepared = f.prepare(&w, "fixture-wait");
        let runner = w.clone();
        let id = prepared["run_id"].clone();
        let thread = std::thread::spawn(move || {
            invoke(&runner, "run_project_analysis", json!({"runId":id}))
        });
        // Fixture waits up to five seconds and checks cancellation every 10ms.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let edit = invoke(
            &w,
            "update_project",
            json!({"projectId":f.project.project_id,"expectedRevision":f.project.revision,"change":{"kind":"rename","value":"Must not change"}}),
        );
        invoke(&w, "cancel_job", json!({})).unwrap();
        assert_eq!(thread.join().unwrap().unwrap()["state"], "cancelled");
        assert_eq!(edit.unwrap_err()["kind"], "conflict");
        assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
        let fresh = f.prepare(&w, "fixture-success");
        invoke(&w, "discard_analysis_review", json!({})).unwrap();
        assert_eq!(f.read(&w, &fresh["run_id"])["state"], "cancelled");
    }

    #[test]
    fn restart_never_replays_and_explicit_recovery_revalidates_saved_result() {
        use tgsum_core::recipe::{Recipe, RecipeEvidence, RecipeOutput};
        let f = Fixture::new();
        let w = f.window();
        let pending = f.prepare(&w, "fixture-success");
        let id = &pending["run_id"];
        let ticket = f
            .store
            .resume_analysis(&f.project.project_id, id.as_str().unwrap())
            .unwrap();
        let restarted = f.window();
        assert_eq!(f.read(&restarted, id)["state"], "interrupted");
        assert!(invoke(&restarted, "run_project_analysis", json!({"runId":id})).is_err());
        assert!(f
            .store
            .read_analysis(&f.project.project_id, id.as_str().unwrap())
            .unwrap()
            .completion
            .is_none());
        let output:RecipeOutput=serde_json::from_value(json!({"recipe":"summary","version":1,"sections":Recipe::Summary.sections().iter().map(|id|json!({"id":id,"claims":[]})).collect::<Vec<_>>(),"actions":[]})).unwrap();
        let staged = tempfile::tempdir().unwrap();
        let exported = f
            .store
            .export_bundle(
                &f.project.project_id,
                &f.bundle,
                f.project.revision,
                staged.path(),
                || false,
            )
            .unwrap();
        let parts: Vec<_> = std::fs::read_dir(exported.directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        let evidence = RecipeEvidence::from_markdown(parts.iter().map(String::as_str)).unwrap();
        f.store
            .save_analysis_result(
                &ticket,
                &output,
                |v| Recipe::Summary.validate(v, &evidence),
                || false,
            )
            .unwrap();
        assert_eq!(f.read(&restarted, id)["state"], "uncommitted");
        assert_eq!(f.read(&restarted, id)["can_commit"], true);
        assert!(f
            .store
            .open(&f.project.project_id)
            .unwrap()
            .baselines
            .is_empty());
        let recovered = invoke(
            &restarted,
            "recover_project_analysis",
            json!({"projectId":f.project.project_id,"runId":id,"commit":true}),
        )
        .unwrap();
        assert_eq!(recovered["state"], "succeeded");
        assert!(matches!(
            f.store
                .read_analysis(&f.project.project_id, id.as_str().unwrap())
                .unwrap()
                .completion,
            Some(Completion::Validated { .. })
        ));
        assert_eq!(
            invoke(
                &restarted,
                "recover_project_analysis",
                json!({"projectId":f.project.project_id,"runId":id,"commit":true})
            )
            .unwrap(),
            recovered
        );
    }
}

#[test]
fn project_commands_persist_and_report_conflicts_and_missing_sources() {
    use tgsum_core::project::ProjectStore;
    let dir = tempfile::tempdir().unwrap();
    let make_window = || {
        let app = tgsum_app::app(command_builder().manage(ProjectStore::new(dir.path())))
            .build(mock_context(noop_assets()))
            .unwrap();
        WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap()
    };
    let w = make_window();
    assert_eq!(invoke(&w, "list_projects", json!({})).unwrap(), json!([]));
    let project = invoke(&w, "create_project", json!({ "name": "Synthetic pilot" })).unwrap();
    let id = &project["project_id"];
    let updated = invoke(&w, "update_project", json!({
        "projectId": id, "expectedRevision": 0,
        "change": {"kind": "source", "value": {
            "source_id": "client", "connector_id": "telegram_export",
            "scope": {"platform": "telegram", "account_local_id": "synthetic", "conversation_id": "111"},
            "archive_path": dir.path().join("moved.json"), "latest_snapshot_id": null
        }}
    })).unwrap();
    assert_eq!(updated["revision"], 1);
    let conflict = invoke(
        &w,
        "update_project",
        json!({
            "projectId": id, "expectedRevision": 0,
            "change": {"kind": "rename", "value": "stale edit"}
        }),
    )
    .unwrap_err();
    assert_eq!(conflict["kind"], "conflict");
    assert_eq!(
        invoke(
            &w,
            "project_source_status",
            json!({
                "projectId": id, "sourceId": "client"
            })
        )
        .unwrap(),
        json!({"state": "file_missing"})
    );
    // A fresh application instance resolves the same saved Project.
    let reopened = make_window();
    assert_eq!(
        invoke(&reopened, "open_project", json!({"projectId": id})).unwrap(),
        updated
    );
    let list = invoke(&reopened, "list_projects", json!({})).unwrap();
    assert_eq!(list[0]["state"], "ready");
    assert_eq!(list[0]["project"], updated);
    let error = invoke(
        &reopened,
        "open_project",
        json!({"projectId": "../outside"}),
    )
    .unwrap_err();
    assert_eq!(error["kind"], "failed");
}

#[test]
fn project_import_scope_and_failed_analysis_use_real_backend_commands() {
    use tgsum_core::project::ProjectStore;
    let dir = tempfile::tempdir().unwrap();
    let app = tgsum_app::app(command_builder().manage(ProjectStore::new(dir.path())))
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut project = invoke(&w, "create_project", json!({"name":"Scoped fixture"})).unwrap();
    let id = project["project_id"].clone();
    project = invoke(&w, "update_project", json!({"projectId":id,"expectedRevision":project["revision"],
        "change":{"kind":"source","value":{
            "source_id":"forum","connector_id":"telegram_json",
            "scope":{"platform":"telegram","account_local_id":"synthetic","conversation_id":"222"},
            "archive_path":fixture(),"latest_snapshot_id":null,
            "selection":{"enabled":true,"only_changes":true,"filter":{
                "topic_ids":["100"],"dates":{"from":"2026-06-18","through":"2026-06-18","basis":"source_date"}
            }}
        }}})).unwrap();
    project = invoke(
        &w,
        "refresh_project_source",
        json!({"projectId":id,"sourceId":"forum","expectedRevision":project["revision"]}),
    )
    .unwrap();
    assert!(project["sources"][0]["latest_snapshot_id"]
        .as_str()
        .unwrap()
        .starts_with("import-"));
    let preview = invoke(
        &w,
        "preview_project_source",
        json!({"projectId":id,"sourceId":"forum"}),
    )
    .unwrap();
    assert_eq!(preview["stats"]["selected"], 1);
    assert_eq!(preview["stats"]["has_baseline"], false);
    assert!(preview["topics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["title"] == "Bugs"));
    project = invoke(
        &w,
        "update_project",
        json!({"projectId":id,"expectedRevision":project["revision"],
        "change":{"kind":"begin_analysis","value":{"run_id":"synthetic-failure"}}}),
    )
    .unwrap();
    project = invoke(&w,"update_project",json!({"projectId":id,"expectedRevision":project["revision"],
        "change":{"kind":"finish_analysis","value":{"run_id":"synthetic-failure","outcome":"failed"}}})).unwrap();
    assert_eq!(project["baselines"], json!([]));
    assert_eq!(
        invoke(
            &w,
            "preview_project_source",
            json!({"projectId":id,"sourceId":"forum"})
        )
        .unwrap()["stats"]["selected"],
        1
    );
    let conflict = invoke(
        &w,
        "refresh_project_source",
        json!({"projectId":id,"sourceId":"forum","expectedRevision":0}),
    )
    .unwrap_err();
    assert_eq!(conflict["kind"], "conflict");
}

#[test]
fn project_review_and_export_commands_enforce_privacy_scope_and_revision() {
    use tgsum_core::project::ProjectStore;
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let archive = root.path().join("PRIVATE_INPUT.json");
    std::fs::write(&archive, serde_json::to_vec(&json!({"chats":{"list":[
        {"id":111,"name":"Selected","messages":[{"id":1,"text":"token=SYNTHETIC_SECRET key=REVIEW_VALUE"}]},
        {"id":222,"name":"Excluded","messages":[{"id":2,"text":"EXCLUDED_TEXT"}]}
    ]}})).unwrap()).unwrap();
    let app =
        tgsum_app::app(command_builder().manage(ProjectStore::new(root.path().join("projects"))))
            .build(mock_context(noop_assets()))
            .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut project = invoke(&w, "create_project", json!({"name":"Synthetic review"})).unwrap();
    let id = project["project_id"].clone();
    project = invoke(
        &w,
        "update_project",
        json!({"projectId":id,"expectedRevision":project["revision"],
        "change":{"kind":"source","value":{
            "source_id":"selected","connector_id":"telegram_json",
            "scope":{"platform":"telegram","account_local_id":"synthetic","conversation_id":"111"},
            "archive_path":archive,"latest_snapshot_id":null
        }}}),
    )
    .unwrap();
    project = invoke(
        &w,
        "refresh_project_source",
        json!({"projectId":id,"sourceId":"selected","expectedRevision":project["revision"]}),
    )
    .unwrap();
    let prepare = |redact| {
        invoke(&w,"prepare_project_bundle",json!({"projectId":id,"expectedRevision":project["revision"],"options":{"redact_candidates":redact}})).unwrap()
    };
    let pending = prepare(false);
    assert_eq!(pending["manifest"]["messages"], 1);
    assert_eq!(pending["manifest"]["destination"], "export_only");
    assert_eq!(pending["manifest"]["privacy"]["redacted"], 1);
    assert_eq!(pending["manifest"]["privacy"]["needs_review"], 1);
    assert_eq!(pending["findings"][0]["field"], "text");
    assert!(!pending.to_string().contains("SYNTHETIC_SECRET"));
    assert!(invoke(&w,"export_project_bundle",json!({"projectId":id,"bundleId":pending["bundle_id"],"expectedRevision":project["revision"],"outDir":output.path()})).is_err());
    let ready = prepare(true);
    assert_eq!(ready["manifest"]["privacy"]["needs_review"], 0);
    let args = json!({"projectId":id,"bundleId":ready["bundle_id"],"expectedRevision":project["revision"],"outDir":output.path()});
    let exported = invoke(&w, "export_project_bundle", args.clone()).unwrap();
    let directory = PathBuf::from(exported["directory"].as_str().unwrap());
    for entry in std::fs::read_dir(directory).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for forbidden in [
            "SYNTHETIC_SECRET",
            "REVIEW_VALUE",
            "EXCLUDED_TEXT",
            "PRIVATE_INPUT",
            "evidence.jsonl",
        ] {
            assert!(!text.contains(forbidden), "export contained {forbidden}");
        }
    }
    let saved = invoke(&w, "open_project", json!({"projectId":id})).unwrap();
    assert_eq!(saved["baselines"], json!([]));
    assert_eq!(saved["revision"], project["revision"]);
    invoke(&w,"update_project",json!({"projectId":id,"expectedRevision":project["revision"],"change":{"kind":"rename","value":"Changed after review"}})).unwrap();
    assert_eq!(
        invoke(&w, "export_project_bundle", args).unwrap_err()["kind"],
        "conflict"
    );
}
