use super::*;
use tgsum_core::project::ProjectStore;

#[test]
fn source_access_reports_the_import_boundary_without_opening_archive_or_client() {
    let root = tempfile::tempdir().unwrap();
    let app =
        tgsum_app::app(command_builder().manage(ProjectStore::new(root.path().join("projects"))))
            .build(mock_context(noop_assets()))
            .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut project = invoke(&w, "create_project", json!({"name":"Access facts"})).unwrap();
    for (id, connector, platform) in [
        ("archive", "telegram_json", "telegram"),
        ("future-oauth", "teams_graph", "teams"),
    ] {
        project = invoke(&w, "update_project", json!({"projectId":project["project_id"],"expectedRevision":project["revision"],
            "change":{"kind":"source","value":{"source_id":id,"connector_id":connector,
                "scope":{"platform":platform,"account_local_id":"local-label","conversation_id":"111"},
                "archive_path":root.path().join("never-open-this-source"),"latest_snapshot_id":null}}})).unwrap();
    }
    let before = project.clone();
    let reply = invoke(
        &w,
        "project_source_accesses",
        json!({"projectId":project["project_id"],"expectedRevision":project["revision"]}),
    )
    .unwrap();
    assert_eq!(reply["project_revision"], project["revision"]);
    let rows = reply["sources"].as_array().unwrap();
    let local = rows.iter().find(|r| r["source_id"] == "archive").unwrap();
    assert_eq!(local["method"]["kind"], "archive");
    assert_eq!(local["method"]["credentials"], "none");
    assert_eq!(local["method"]["refresh"], "reimport");
    assert_eq!(local["method"]["attachments"], "references_only");
    assert_eq!(local["method"]["assisted_export"], true);
    assert_eq!(local["selected_scope"]["conversation_id"], "111");
    assert_eq!(local["attachment_choices"], 0);
    let future = rows
        .iter()
        .find(|r| r["source_id"] == "future-oauth")
        .unwrap();
    assert!(
        future["method"].is_null(),
        "a selected conversation is not an observed OAuth grant"
    );
    assert_eq!(future["selected_scope"]["platform"], "teams");
    assert!(!reply.to_string().contains("never-open-this-source"));
    assert!(!root.path().join("never-open-this-source").exists());
    assert_eq!(
        invoke(
            &w,
            "open_project",
            json!({"projectId":project["project_id"]})
        )
        .unwrap(),
        before
    );
    assert_eq!(
        invoke(
            &w,
            "project_source_accesses",
            json!({"projectId":project["project_id"],"expectedRevision":0})
        )
        .unwrap_err()["kind"],
        "conflict"
    );
    assert!(invoke(&w,"refresh_project_source",json!({"projectId":project["project_id"],"expectedRevision":project["revision"],"sourceId":"future-oauth"})).is_err());
}
