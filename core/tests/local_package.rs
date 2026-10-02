#![cfg(unix)]
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde_json::json;
use tgsum_core::local_package::PackageSettings;
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

fn office(ext: &str, xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(b"<Types/>").unwrap();
    let name = if ext == "docx" {
        "word/document.xml"
    } else {
        "xl/workbook.xml"
    };
    zip.start_file(name, options).unwrap();
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn office_names_across_runs_and_shared_strings_preserve_structure() {
    let input = office(
        "docx",
        r#"<w:document xmlns:w="word"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Иванов </w:t></w:r><w:r><w:t>Иван</w:t></w:r><w:r><w:t> Иванович: готово &amp; принято.</w:t></w:r></w:p></w:body></w:document>"#,
    );
    let processed = tgsum_core::office::process_office(&input, "docx", || false).unwrap();
    assert_eq!(processed.replacements, 1);
    let mut zip = zip::ZipArchive::new(Cursor::new(&processed.bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("Иванов И. И."));
    assert!(!xml.contains("Иванович"));
    assert!(xml.contains("<w:b/>"));
    assert!(xml.contains("готово &amp; принято."));
    let again = tgsum_core::office::process_office(&processed.bytes, "docx", || false).unwrap();
    assert_eq!(again.replacements, 0);

    let input = office(
        "xlsx",
        r#"<workbook><si><r><t>Петрова Анна </t></r><r><t>Сергеевна</t></r></si><c><f>SUM(A1:A2)</f><v>42</v></c><c t="inlineStr"><is><t>Иванов Иван Иванович</t></is></c></workbook>"#,
    );
    let processed = tgsum_core::office::process_office(&input, "xlsx", || false).unwrap();
    assert_eq!(processed.replacements, 2);
    let mut zip = zip::ZipArchive::new(Cursor::new(processed.bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("Петрова А. С."));
    assert!(xml.contains("<f>SUM(A1:A2)</f><v>42</v>"));
    assert!(tgsum_core::office::process_office(&input, "xlsx", || true).is_err());
    assert!(tgsum_core::office::process_office(b"not a zip", "docx", || false).is_err());
}

struct Fixture {
    _private: tempfile::TempDir,
    input: tempfile::TempDir,
    output: tempfile::TempDir,
    store: ProjectStore,
    project: Project,
}
impl Fixture {
    fn new() -> Self {
        let private = tempfile::tempdir().unwrap();
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(private.path());
        let project = store.create("Synthetic package").unwrap();
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "selected".into(),
                    connector_id: "telegram_json".into(),
                    scope: SourceScope::telegram("fixture", "9007199254740993"),
                    archive_path: Some(input.path().join("result.json")),
                    latest_snapshot_id: None,
                    selection: Default::default(),
                }),
            )
            .unwrap();
        store
            .configure_local_package(
                &project.project_id,
                project.revision,
                PackageSettings {
                    source_id: "selected".into(),
                    input_directory: input.path().into(),
                    output_directory: output.path().into(),
                    automatic: true,
                    include_images: true,
                    include_office: true,
                    github_repository: None,
                },
            )
            .unwrap();
        Self {
            _private: private,
            input,
            output,
            store,
            project,
        }
    }
    fn write(&self, extra: bool) {
        let mut messages = vec![
            json!({"id":9007199254740993u64,"date":"2026-10-02T12:00:00","text":"Привет 🌍 password=SYNTHETIC_LOCAL_SECRET"}),
        ];
        if extra {
            messages.push(json!({"id":9007199254740994u64,"text":"Новое сообщение"}));
        }
        fs::write(
            self.input.path().join("result.json"),
            serde_json::to_vec(&json!({"id":9007199254740993u64,"messages":messages})).unwrap(),
        )
        .unwrap();
    }
    fn run(&self) -> tgsum_core::local_package::PackageState {
        self.store
            .refresh_local_package(&self.project.project_id, 123, || false)
            .unwrap()
    }
}

#[test]
fn refresh_repeat_replace_and_restart_keep_only_owned_current_generation() {
    let fixture = Fixture::new();
    fixture.write(false);
    let first = fixture.run();
    assert_eq!(first.phase, "ready", "{}", first.message);
    let first = first.ready.unwrap();
    let md = fs::read_to_string(first.directory.join("context-00001.md")).unwrap();
    assert!(!md.contains("SYNTHETIC_LOCAL_SECRET"));
    assert!(md.contains("Привет 🌍"));
    assert!(first.directory.join("README.md").is_file());
    let repeat = fixture.run().ready.unwrap();
    assert_eq!(first.generation, repeat.generation);
    let root = first.directory.parent().unwrap();
    fs::write(root.join("user-note.md"), "Keep me").unwrap();
    fixture.write(true);
    let second = fixture.run();
    assert_eq!(second.phase, "ready", "{}", second.message);
    let second = second.ready.unwrap();
    assert_ne!(first.generation, second.generation);
    assert_eq!(second.messages, 2);
    assert!(!root.join(first.generation).exists());
    assert_eq!(
        fs::read_to_string(root.join("user-note.md")).unwrap(),
        "Keep me"
    );
    assert!(fixture.input.path().join("result.json").exists());
    let reopened = ProjectStore::new(fixture._private.path());
    assert_eq!(
        reopened
            .refresh_local_package(&fixture.project.project_id, 124, || false)
            .unwrap()
            .ready
            .unwrap()
            .generation,
        second.generation
    );
}

#[test]
fn bad_input_wrong_chat_and_cancel_preserve_previous_package() {
    let fixture = Fixture::new();
    fixture.write(false);
    let good = fixture.run().ready.unwrap();
    let original = fs::read(good.directory.join("context-00001.md")).unwrap();
    for input in [
        r#"{"id":9007199254740993,"messages":["#,
        r#"{"id":2,"messages":[]}"#,
    ] {
        fs::write(fixture.input.path().join("result.json"), input).unwrap();
        let state = fixture.run();
        assert_eq!(state.phase, "error");
        assert_eq!(state.ready.unwrap().generation, good.generation);
        assert_eq!(
            fs::read(good.directory.join("context-00001.md")).unwrap(),
            original
        );
    }
    fixture.write(true);
    let state = fixture
        .store
        .refresh_local_package(&fixture.project.project_id, 124, || true)
        .unwrap();
    assert_eq!(state.phase, "cancelled");
    assert_eq!(state.ready.unwrap().generation, good.generation);
}

#[test]
fn images_are_exact_office_is_processed_and_missing_files_never_publish() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.input.path().join("files")).unwrap();
    let document = office(
        "docx",
        r#"<document><p><t>Иванов Иван Иванович</t></p></document>"#,
    );
    fs::write(fixture.input.path().join("files/document.docx"), &document).unwrap();
    let picture = b"\x89PNG\r\nSYNTHETIC_IMAGE_METADATA_KEPT";
    fs::write(fixture.input.path().join("files/picture.png"), picture).unwrap();
    fs::write(
        fixture.input.path().join("result.json"),
        serde_json::to_vec(&json!({"id":9007199254740993u64,"messages":[
            {"id":1,"text":"document","file":"files/document.docx","file_size":document.len()},
            {"id":2,"text":"picture","photo":"files/picture.png","photo_file_size":picture.len()}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let first = fixture.run();
    assert_eq!(first.phase, "ready", "{}", first.message);
    let ready = first.ready.unwrap();
    assert_eq!(ready.initials_replacements, 1);
    let index = fs::read_to_string(ready.directory.join("README.md")).unwrap();
    let markdown = fs::read_to_string(ready.directory.join("context-00001.md")).unwrap();
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(ready.directory.join("package.json")).unwrap()).unwrap();
    for file in package["files"].as_array().unwrap() {
        if let Some(reference) = file["attachment_id"].as_str() {
            assert!(markdown.contains(reference));
            assert!(index.contains(reference));
        }
    }
    let image = fs::read_dir(&ready.directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "png"))
        .unwrap();
    assert_eq!(fs::read(image).unwrap(), picture);
    fs::remove_file(fixture.input.path().join("files/picture.png")).unwrap();
    let failed = fixture.run();
    assert_eq!(failed.phase, "error");
    assert_eq!(failed.ready.unwrap().generation, ready.generation);
    assert_eq!(
        fs::read(fixture.input.path().join("files/document.docx")).unwrap(),
        document
    );
}

#[test]
fn unsafe_paths_and_changed_scope_do_not_extend_the_package() {
    let fixture = Fixture::new();
    fixture.write(false);
    let ready = fixture.run().ready.unwrap();
    let project = fixture.store.open(&fixture.project.project_id).unwrap();
    fixture
        .store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Settings(tgsum_core::project::ProjectSettings {
                max_tokens: Some(42),
                ..project.settings
            }),
        )
        .unwrap();
    let state = fixture.run();
    assert_eq!(state.phase, "needs_setup");
    assert_eq!(state.ready.unwrap().generation, ready.generation);
    assert!(fixture
        .store
        .configure_local_package(
            &project.project_id,
            project.revision + 1,
            PackageSettings {
                source_id: "selected".into(),
                input_directory: fixture.input.path().into(),
                output_directory: fixture.input.path().into(),
                automatic: true,
                include_images: true,
                include_office: true,
                github_repository: None,
            }
        )
        .is_err());
    assert!(Path::new(&fixture.output.path()).is_dir());
}

#[test]
fn pause_persists_and_limits_and_symlinks_keep_last_ready() {
    let fixture = Fixture::new();
    fixture.write(false);
    let ready = fixture.run().ready.unwrap();
    fixture
        .store
        .pause_local_package(&fixture.project.project_id)
        .unwrap();
    let reopened = ProjectStore::new(fixture._private.path());
    let paused = reopened
        .local_package(&fixture.project.project_id)
        .unwrap()
        .unwrap();
    assert!(!paused.settings.automatic);
    assert_eq!(paused.phase, "paused");
    assert_eq!(paused.ready.unwrap().generation, ready.generation);
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "private bytes").unwrap();
    std::os::unix::fs::symlink(outside.path(), fixture.input.path().join("picture.png")).unwrap();
    fs::write(
        fixture.input.path().join("result.json"),
        serde_json::to_vec(&json!({
            "id":9007199254740993u64,"messages":[{"id":1,"photo":"picture.png","text":""}]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(fixture.run().phase, "error");
    fs::remove_file(fixture.input.path().join("picture.png")).unwrap();
    let large = fs::File::create(fixture.input.path().join("picture.png")).unwrap();
    large
        .set_len(tgsum_core::local_package::MAX_MEDIA_BYTES + 1)
        .unwrap();
    assert_eq!(fixture.run().phase, "error");
    fs::write(fixture.input.path().join("note.txt"), "partial").unwrap();
    fs::write(fixture.input.path().join("result.json"), serde_json::to_vec(&json!({
        "id":9007199254740993u64,"messages":[{"id":1,"file":"note.txt","file_size":100,"text":""}]
    })).unwrap()).unwrap();
    let failed = fixture.run();
    assert_eq!(failed.phase, "error");
    assert_eq!(failed.ready.unwrap().generation, ready.generation);
    assert_eq!(fs::read_to_string(outside.path()).unwrap(), "private bytes");
}

#[test]
fn cleanup_preserves_foreign_files_inside_old_generation() {
    let fixture = Fixture::new();
    fixture.write(false);
    let ready = fixture.run().ready.unwrap();
    let old = ready.directory.parent().unwrap().join(&ready.generation);
    fs::write(old.join("user-note.txt"), "Keep me").unwrap();
    fixture.write(true);
    assert_eq!(fixture.run().phase, "ready");
    assert_eq!(
        fs::read_to_string(old.join("user-note.txt")).unwrap(),
        "Keep me"
    );
}

#[test]
fn restart_recovers_interrupted_state_and_removes_verified_obsolete_generation() {
    let fixture = Fixture::new();
    fixture.write(false);
    let ready = fixture.run().ready.unwrap();
    let root = ready.directory.parent().unwrap();
    let obsolete = root.join("tgsum-context-interrupted");
    fs::create_dir(&obsolete).unwrap();
    for file in fs::read_dir(&ready.directory).unwrap() {
        let file = file.unwrap();
        fs::copy(file.path(), obsolete.join(file.file_name())).unwrap();
    }
    let path = fixture
        ._private
        .path()
        .join(&fixture.project.project_id)
        .join("local-package/state.json");
    let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["phase"] = json!("building");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let reopened = ProjectStore::new(fixture._private.path());
    let recovered = reopened
        .refresh_local_package(&fixture.project.project_id, 200, || false)
        .unwrap();
    assert_eq!(recovered.phase, "ready");
    assert_eq!(recovered.ready.unwrap().generation, ready.generation);
    assert!(!obsolete.exists());
    assert!(ready.directory.is_dir());
}

#[test]
fn office_rejects_unsafe_container_and_xml() {
    for name in [
        "../outside.xml",
        "word/vbaProject.bin",
        "word/embeddings/object.bin",
    ] {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"invalid").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(tgsum_core::office::process_office(&bytes, "docx", || false).is_err());
    }
    let bytes = office(
        "docx",
        "<!DOCTYPE a [<!ENTITY x SYSTEM 'file:///tmp/private'>]><document>&x;</document>",
    );
    assert!(tgsum_core::office::process_office(&bytes, "docx", || false).is_err());
}
