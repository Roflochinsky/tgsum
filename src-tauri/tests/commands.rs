//! Drives the Tauri commands on the mock runtime with the same JSON the UI
//! (`ui/app.js`) sends, so argument names and response shapes stay in sync.

use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

fn window() -> WebviewWindow<tauri::test::MockRuntime> {
    let app = tgsum_app::app(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap()
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

#[test]
fn project_commands_persist_and_report_conflicts_and_missing_sources() {
    use tgsum_core::project::ProjectStore;
    let dir = tempfile::tempdir().unwrap();
    let make_window = || {
        let app = tgsum_app::app(mock_builder().manage(ProjectStore::new(dir.path())))
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
    let app = tgsum_app::app(mock_builder().manage(ProjectStore::new(dir.path())))
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
        tgsum_app::app(mock_builder().manage(ProjectStore::new(root.path().join("projects"))))
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
