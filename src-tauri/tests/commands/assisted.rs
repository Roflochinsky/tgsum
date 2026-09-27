use super::*;
use tgsum_core::project::ProjectStore;

#[test]
fn assisted_commands_keep_launch_separate_from_confirmed_import() {
    let root = tempfile::tempdir().unwrap();
    let app =
        tgsum_app::app(command_builder().manage(ProjectStore::new(root.path().join("projects"))))
            .build(mock_context(noop_assets()))
            .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut p = invoke(&w, "create_project", json!({"name":"Assisted IPC"})).unwrap();
    p = invoke(&w,"update_project",json!({"projectId":p["project_id"],"expectedRevision":p["revision"],"change":{"kind":"source","value":{
        "source_id":"selected","connector_id":"telegram_json","scope":{"platform":"telegram","account_local_id":"synthetic","conversation_id":"111"},"archive_path":fixture(),"latest_snapshot_id":null
    }}})).unwrap();
    p = invoke(&w,"update_project",json!({"projectId":p["project_id"],"expectedRevision":p["revision"],"change":{"kind":"assisted_export","value":{
        "source_id":"selected","settings":{"directory":root.path(),"client":null}
    }}})).unwrap();
    assert_eq!(
        invoke(
            &w,
            "poll_assisted_exports",
            json!({"projectId":p["project_id"],
        "sourceId":"selected","expectedRevision":p["revision"]})
        )
        .unwrap(),
        json!([])
    );
    let args =
        json!({"projectId":p["project_id"],"sourceId":"selected","expectedRevision":p["revision"]});
    assert_eq!(
        invoke(&w, "launch_assisted_client", args).unwrap_err()["kind"],
        "failed"
    );
    let client = root.path().join(if cfg!(windows) {
        "fake client.exe"
    } else {
        "fake client ; $literal"
    });
    assert!(std::process::Command::new("rustc")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/assisted-client.rs")
        )
        .arg("-o")
        .arg(&client)
        .status()
        .unwrap()
        .success());
    p = invoke(&w,"update_project",json!({"projectId":p["project_id"],"expectedRevision":p["revision"],"change":{"kind":"assisted_export","value":{
        "source_id":"selected","settings":{"directory":root.path(),"client":client}
    }}})).unwrap();
    let receipt = invoke(
        &w,
        "launch_assisted_client",
        json!({"projectId":p["project_id"],"sourceId":"selected","expectedRevision":p["revision"]}),
    )
    .unwrap();
    assert_eq!(receipt["state"], "needs_user_action");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let marker = root.path().join("fake-client-launched.txt");
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "arguments=0");
    assert_eq!(
        invoke(&w, "open_project", json!({"projectId":p["project_id"]})).unwrap(),
        p
    );
    let mut request = json!({"project_id":p["project_id"],"source_id":"selected","expected_revision":p["revision"],"archive_path":fixture(),"scope_and_completion_confirmed":false});
    assert_eq!(
        invoke(&w, "import_assisted_export", json!({"request":request})).unwrap_err()["kind"],
        "failed"
    );
    request["scope_and_completion_confirmed"] = true.into();
    let completed = invoke(&w, "import_assisted_export", json!({"request":request})).unwrap();
    assert!(completed["delta"]["created"].as_u64().unwrap() > 0);
    let updated = &completed["project"];
    assert!(updated["sources"][0]["latest_snapshot_id"].is_string());
    assert_eq!(
        updated["revision"].as_u64(),
        Some(p["revision"].as_u64().unwrap() + 1)
    );
    assert_eq!(
        invoke(&w, "import_assisted_export", json!({"request":request})).unwrap_err()["kind"],
        "conflict"
    );
    let bundle = invoke(
        &w,
        "prepare_project_bundle",
        json!({"projectId":p["project_id"],"expectedRevision":updated["revision"]}),
    )
    .unwrap();
    assert!(!bundle.to_string().contains(root.path().to_str().unwrap()));
    assert!(!bundle.to_string().contains("assisted_exports"));
}
