//! One public IPC journey across archives and successful-analysis baselines.
//! Only the external agent is synthetic; import, selection, privacy, bundles,
//! recipe validation, persistence and atomic completion are the real backend.
use super::*;
use tgsum_core::project::ProjectStore;

const CHAT: &str = "9007199254740993";
const OTHER_CHAT: &str = "9007199254740992";

struct Journey {
    root: tempfile::TempDir,
    window: WebviewWindow<tauri::test::MockRuntime>,
    project: Value,
    agent: &'static str,
}
impl Journey {
    fn new(agent: &'static str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let window = Self::host(&root);
        let project = invoke(
            &window,
            "create_project",
            json!({"name":"Synthetic project journey"}),
        )
        .unwrap();
        Self {
            root,
            window,
            project,
            agent,
        }
    }
    fn host(root: &tempfile::TempDir) -> WebviewWindow<tauri::test::MockRuntime> {
        let app = tgsum_app::app(
            mock_builder()
                .manage(ProjectStore::new(root.path().join("projects")))
                .manage(tgsum_app::analysis::AnalysisState::synthetic()),
        )
        .build(mock_context(noop_assets()))
        .unwrap();
        WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap()
    }
    fn call(&self, command: &str, args: Value) -> Value {
        invoke(&self.window, command, args).unwrap_or_else(|e| panic!("{command}: {e}"))
    }
    fn archive(&self) -> PathBuf {
        self.root.path().join("PRIVATE_ARCHIVE.json")
    }
    fn write_archive(&self, edited: bool) {
        let mut messages = vec![
            json!({"id":u64::MAX,"date":"2026-01-02T10:00:00","text":if edited {"EDIT_AFTER token=TEST_ONLY_SECRET"} else {"EDIT_BEFORE token=TEST_ONLY_SECRET"}}),
            json!({"id":6,"date":"2026-01-02T11:00:00","text":"UNCHANGED_SELECTED"}),
            json!({"id":4,"date":"2026-01-03T10:00:00","text":"NEWLY_SELECTED_SCOPE"}),
            json!({"id":1,"date":"2026-01-01T10:00:00","text":"OUTSIDE_DATE"}),
        ];
        if edited {
            messages[0]["edited"] = json!("2026-01-04T09:00:00");
            messages.push(
                json!({"id":5,"date":"2026-01-02T12:00:00","text":"NEW_MESSAGE_WITH_LOWER_ID"}),
            );
        } else {
            messages.push(
                json!({"id":3,"date":"2026-01-02T13:00:00","text":"MISSING_FROM_NEXT_ARCHIVE"}),
            );
        }
        let data = json!({"chats":{"list":[
            {"id":9007199254740993u64,"name":"Selected work chat","messages":messages},
            {"id":9007199254740992u64,"name":"PRIVATE_OTHER_CHAT","messages":[{"id":u64::MAX,"text":"EXCLUDED_CHAT_MESSAGE"}]}
        ]}});
        std::fs::write(self.archive(), serde_json::to_vec(&data).unwrap()).unwrap();
    }
    fn selection(through: &str) -> Value {
        json!({"enabled":true,"only_changes":true,"filter":{
            "dates":{"from":"2026-01-02","through":through,"basis":"source_date"}}})
    }
    fn update(&mut self, change: Value) {
        self.project = self.call("update_project",json!({"projectId":self.project["project_id"],"expectedRevision":self.project["revision"],"change":change}));
    }
    fn refresh(&mut self) {
        self.project = self.call("refresh_project_source",json!({"projectId":self.project["project_id"],"sourceId":"selected","expectedRevision":self.project["revision"]}));
    }
    fn saved(&self) -> Value {
        self.call(
            "open_project",
            json!({"projectId":self.project["project_id"]}),
        )
    }
    fn preview(&self) -> Value {
        self.call(
            "preview_project_source",
            json!({"projectId":self.project["project_id"],"sourceId":"selected"}),
        )
    }
    fn bundle(&self) -> Value {
        self.call("prepare_project_bundle",json!({"projectId":self.project["project_id"],"expectedRevision":self.project["revision"],"options":{"redact_candidates":true}}))
    }
    fn exported_markdown(&self, bundle: &Value) -> String {
        let exported = self.call("export_project_bundle",json!({"projectId":self.project["project_id"],"bundleId":bundle["bundle_id"],"expectedRevision":self.project["revision"],"outDir":self.root.path().join("exports")}));
        let directory = PathBuf::from(exported["directory"].as_str().unwrap());
        let mut markdown = String::new();
        // These are public exported files, not the private store/index.
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for forbidden in [
                "TEST_ONLY_SECRET",
                "EXCLUDED_CHAT_MESSAGE",
                "PRIVATE_OTHER_CHAT",
                "OUTSIDE_DATE",
                "PRIVATE_ARCHIVE",
                CHAT,
                OTHER_CHAT,
                "18446744073709551615",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "public export leaked {forbidden}"
                );
            }
            if path.extension().is_some_and(|e| e == "md") {
                markdown.push_str(&text);
            }
        }
        markdown
    }
    fn prepare(&self, bundle: &Value, model: &str) -> Result<Value, Value> {
        invoke(
            &self.window,
            "prepare_project_analysis",
            json!({"projectId":self.project["project_id"],"bundleId":bundle["bundle_id"],"expectedRevision":self.project["revision"],
            "options":{"agent":self.agent,"executable":"/NEVER_EXECUTE","auth_file":"/NEVER_OPEN","model":model,"recipe":"summary","destination":"local_fixture"}}),
        )
    }
    fn run(&mut self, bundle: &Value, model: &str) -> Value {
        let review = self.prepare(bundle, model).unwrap();
        assert_eq!(review["coverage"].as_array().unwrap().len(), 1);
        assert_eq!(
            review["coverage"][0]["messages"],
            bundle["manifest"]["messages"]
        );
        let result = self.call("run_project_analysis", json!({"runId":review["run_id"]}));
        self.project = self.saved();
        result
    }
}

// Read the documented public Markdown identity alongside a known fixture body;
// do not recompute the production HMAC or read the private evidence map.
fn reference(markdown: &str, marker: &str) -> Value {
    let block = markdown
        .split("## Evidence ")
        .skip(1)
        .find(|b| b.contains(marker))
        .unwrap_or_else(|| panic!("missing {marker}"));
    let (id, revision) = block.lines().next().unwrap().split_once('@').unwrap();
    json!({"id":id,"revision":revision})
}
fn first_reference(output: &Value) -> &Value {
    &output["result"]["sections"][0]["claims"][0]["evidence"][0]
}

#[test]
fn repeated_archives_preserve_evidence_and_only_success_advances_delta() {
    for agent in ["codex", "claude"] {
        let mut j = Journey::new(agent);
        j.write_archive(false);
        let index = j.call("index_export", json!({"path":j.archive()}));
        assert_eq!(index["chats"][0]["chatId"], CHAT);
        assert_eq!(index["chats"][1]["chatId"], OTHER_CHAT);
        j.update(json!({"kind":"source","value":{
            "source_id":"selected","connector_id":"telegram_json","scope":{"platform":"telegram","account_local_id":"synthetic","conversation_id":CHAT},
            "archive_path":j.archive(),"latest_snapshot_id":null,"selection":Journey::selection("2026-01-02")}}));
        j.refresh();
        let first_snapshot = j.project["sources"][0]["latest_snapshot_id"].clone();
        let first_bundle = j.bundle();
        assert_eq!(first_bundle["manifest"]["messages"], 3);
        assert_eq!(
            first_bundle["manifest"]["sources"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            first_bundle["manifest"]["sources"][0]["coverage"],
            "unknown"
        );
        assert_eq!(first_bundle["manifest"]["privacy"]["needs_review"], 0);
        let before = j.exported_markdown(&first_bundle);
        assert!(!before.contains("NEWLY_SELECTED_SCOPE"));
        let original = reference(&before, "EDIT_BEFORE");
        assert_eq!(j.saved()["baselines"], json!([])); // Export never marks analyzed.
        let first_result = j.run(&first_bundle, "fixture-success");
        assert_eq!(first_result["state"], "succeeded");
        assert_eq!(first_reference(&first_result), &original);
        let first_baselines = j.project["baselines"].clone();
        assert_eq!(first_baselines[0]["snapshot_id"], first_snapshot);
        assert_eq!(first_baselines[0]["analysis_id"], first_result["run_id"]);

        // Re-exporting identical bytes produces no new analysis work.
        j.refresh();
        assert_ne!(
            j.project["sources"][0]["latest_snapshot_id"],
            first_snapshot
        );
        assert_eq!(j.project["baselines"], first_baselines);
        assert_eq!(j.preview()["stats"]["selected"], 0);
        assert_eq!(j.preview()["stats"]["unchanged"], 3);

        j.write_archive(true);
        j.refresh();
        j.update(json!({"kind":"selection","value":{"source_id":"selected","selection":Journey::selection("2026-01-03")}}));
        let expected = j.preview();
        assert_eq!(expected["stats"]["selected"], 3);
        assert_eq!(expected["stats"]["created"], 2); // new message + newly selected date
        assert_eq!(expected["stats"]["edited"], 1); // old ID/date, edited later
        assert_eq!(expected["stats"]["unchanged"], 1);
        assert_eq!(expected["stats"]["missing"], 1);
        assert_eq!(expected["stats"]["deleted"], 0); // missing is not deletion
        assert_eq!(expected["baseline_analysis_id"], first_result["run_id"]);
        let changed_bundle = j.bundle();
        let after = j.exported_markdown(&changed_bundle);
        let changed = reference(&after, "EDIT_AFTER");
        assert_eq!(changed["id"], original["id"]);
        assert_ne!(changed["revision"], original["revision"]);
        assert!(after.contains("NEWLY_SELECTED_SCOPE"));
        assert!(after.contains("NEW_MESSAGE_WITH_LOWER_ID"));
        for excluded in [
            "EDIT_BEFORE",
            "UNCHANGED_SELECTED",
            "MISSING_FROM_NEXT_ARCHIVE",
        ] {
            assert!(!after.contains(excluded));
        }
        assert_eq!(j.project["baselines"], first_baselines);

        let saved_before_failure = j.saved();
        let failure = j.run(&changed_bundle, "fixture-failure");
        assert_eq!(failure["state"], "failed");
        assert_eq!(failure["result"], Value::Null);
        assert_eq!(j.saved(), saved_before_failure);
        assert_eq!(j.preview(), expected);
        let second_result = j.run(&changed_bundle, "fixture-success");
        assert_eq!(second_result["state"], "succeeded");
        assert_eq!(first_reference(&second_result), &changed);
        assert_eq!(
            j.project["baselines"][0]["snapshot_id"],
            expected["snapshot_id"]
        );
        assert_eq!(
            j.project["baselines"][0]["filter"]["dates"]["through"],
            "2026-01-03"
        );
        assert_eq!(j.preview()["stats"]["selected"], 0);

        // Later imports and application instances preserve older cited revisions.
        j.refresh();
        assert_eq!(j.preview()["stats"]["selected"], 0);
        let empty = invoke(
            &j.window,
            "prepare_project_bundle",
            json!({
                "projectId":j.project["project_id"],"expectedRevision":j.project["revision"],
                "options":{"redact_candidates":true}
            }),
        )
        .unwrap_err();
        assert_eq!(empty["kind"], "failed");
        assert!(empty["message"].as_str().unwrap().contains("no messages"));
        let stable = j.saved();
        std::fs::write(j.archive(), b"{\"chats\":{\"list\":[").unwrap();
        assert!(invoke(&j.window,"refresh_project_source",json!({"projectId":j.project["project_id"],"sourceId":"selected","expectedRevision":j.project["revision"]})).is_err());
        assert_eq!(j.saved(), stable);
        j.window = Journey::host(&j.root);
        for output in [&first_result, &failure, &second_result] {
            assert_eq!(
                j.call(
                    "read_project_analysis",
                    json!({"projectId":j.project["project_id"],"runId":output["run_id"]})
                ),
                *output
            );
        }
        assert_eq!(
            j.call(
                "list_project_analyses",
                json!({"projectId":j.project["project_id"]})
            )
            .as_array()
            .unwrap()
            .len(),
            3
        );
        assert_eq!(j.saved(), stable);
    }
}
