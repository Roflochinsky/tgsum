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
