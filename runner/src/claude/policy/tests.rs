use super::*;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("managed-settings.d")).unwrap();
    fs::set_permissions(
        dir.path().join("managed-settings.d"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    dir
}

#[test]
fn incompatible_or_malformed_policy_is_refused_before_any_mount_staging() {
    for (name, body) in [
        ("managed-settings.json", "{\"env\":{\"NEUTRAL\":\"value\"}}"),
        (
            "managed-settings.d/10-helpers.json",
            "{\"policyHelper\":\"/not-executed\"}",
        ),
        (
            "managed-mcp.json",
            "{\"mcpServers\":{\"a\":{\"command\":\"/not-executed\"}}}",
        ),
        ("managed-settings.json", "{ /* not JSON */ }"),
        (
            "managed-settings.json",
            "{\"env\":{\"x\":\"y\"},\"env\":{}}",
        ),
    ] {
        let dir = root();
        fs::write(dir.path().join(name), body).unwrap();
        let cancel = Cancellation::default();
        let policy = EndpointPolicy::fixture(dir.path(), &cancel).unwrap();
        let output = tempfile::tempdir().unwrap();
        assert!(policy.stage(output.path(), &cancel).is_err());
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
        assert_eq!(fs::read_to_string(dir.path().join(name)).unwrap(), body);
    }
    for body in ["\u{feff}{\"availableModels\":[\"sonnet\"]}", "  "] {
        let dir = root();
        fs::write(dir.path().join("managed-settings.json"), body).unwrap();
        let cancel = Cancellation::default();
        let policy = EndpointPolicy::fixture(dir.path(), &cancel).unwrap();
        let output = tempfile::tempdir().unwrap();
        assert!(policy.stage(output.path(), &cancel).is_ok());
    }
}

#[test]
fn preserves_main_fragments_and_mcp_bytes_without_selecting_profile_files() {
    let dir = root();
    let files = [
        (
            "managed-settings.json",
            "{\n  \"permissions\": {\"deny\":[\"Bash\"]}\n}\n",
        ),
        (
            "managed-settings.d/20-model.json",
            "{\"availableModels\":[\"sonnet\"]}",
        ),
        (
            "managed-settings.d/10-company.json",
            "\u{feff}{\"synthetic\":true}",
        ),
        (
            "managed-mcp.json",
            "{\"mcpServers\":{\"synthetic\":{\"command\":\"not-executed\"}}}",
        ),
    ];
    for (name, text) in files {
        fs::write(dir.path().join(name), text).unwrap();
    }
    for name in [
        "settings.json",
        ".credentials.json",
        "managed-settings.d/readme.txt",
        "managed-settings.d/.ignored.json",
    ] {
        // A socket in an unselected path proves it is not opened/read.
        let _socket = std::os::unix::net::UnixListener::bind(dir.path().join(name)).unwrap();
    }
    let policy = EndpointPolicy::fixture(dir.path(), &Cancellation::default()).unwrap();
    assert_eq!(policy.snapshot.files.len(), files.len());
    for (name, bytes) in files {
        assert_eq!(policy.snapshot.files[Path::new(name)], bytes.as_bytes());
    }
    assert_eq!(format!("{policy:?}"), "EndpointPolicy { files: 4, .. }");
    policy.check_unchanged(&Cancellation::default()).unwrap();
}

#[test]
fn changed_removed_added_and_newly_created_policy_require_new_review() {
    for change in ["edit", "remove", "add-fragment", "replace"] {
        let dir = root();
        let path = dir.path().join("managed-settings.json");
        fs::write(&path, "{\"version\":1}").unwrap();
        let policy = EndpointPolicy::fixture(dir.path(), &Cancellation::default()).unwrap();
        match change {
            "edit" => fs::write(&path, "{\"version\":2}").unwrap(),
            "remove" => fs::remove_file(&path).unwrap(),
            "add-fragment" => {
                fs::write(dir.path().join("managed-settings.d/new.json"), "{}").unwrap()
            }
            _ => {
                fs::rename(&path, dir.path().join("previous")).unwrap();
                fs::write(&path, "{\"version\":2}").unwrap();
            }
        }
        assert!(
            policy.check_unchanged(&Cancellation::default()).is_err(),
            "{change}"
        );
    }
    let dir = root();
    let missing = dir.path().join("absent-system-directory");
    let policy = EndpointPolicy::fixture(&missing, &Cancellation::default()).unwrap();
    assert!(!policy.snapshot.root_present);
    fs::create_dir(&missing).unwrap();
    fs::write(missing.join("managed-settings.json"), "{}").unwrap();
    assert!(policy.check_unchanged(&Cancellation::default()).is_err());
}

#[test]
fn unsafe_and_unreadable_entries_are_not_treated_as_absent() {
    let dir = root();
    let path = dir.path().join("managed-settings.json");
    fs::write(&path, "{}").unwrap();
    for mode in [0o666, 0o644 | 0o4000, 0o000] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let other = dir.path().join("hard-link");
    fs::hard_link(&path, &other).unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    fs::remove_file(&other).unwrap();
    fs::rename(&path, &other).unwrap();
    symlink(&other, &path).unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    fs::remove_dir(&path).unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    fs::remove_file(&path).unwrap();
    rustix::fs::mknodat(
        CWD,
        &path,
        rustix::fs::FileType::Fifo,
        Mode::RUSR | Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    fs::remove_file(&path).unwrap();
    fs::remove_dir(dir.path().join("managed-settings.d")).unwrap();
    symlink(dir.path(), dir.path().join("managed-settings.d")).unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    assert!(EndpointPolicy::capture_at(
        dir.path(),
        rustix::process::geteuid().as_raw() + 1,
        &Cancellation::default()
    )
    .is_err());
}

#[test]
fn size_count_and_cancellation_bounds_apply_before_launch() {
    let dir = root();
    let path = dir.path().join("managed-settings.json");
    File::create(&path)
        .unwrap()
        .set_len(MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    fs::remove_file(&path).unwrap();
    for i in 0..MAX_FILES + 1 {
        fs::write(
            dir.path().join(format!("managed-settings.d/{i}.json")),
            "{}",
        )
        .unwrap();
    }
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    let dir = root();
    for i in 0..5 {
        File::create(dir.path().join(format!("managed-settings.d/{i}.json")))
            .unwrap()
            .set_len(MAX_FILE_BYTES as u64)
            .unwrap();
    }
    assert!(EndpointPolicy::fixture(dir.path(), &Cancellation::default()).is_err());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        EndpointPolicy::fixture(dir.path(), &cancel),
        Err(RunnerError::Cancelled)
    ));
}

#[test]
fn staging_preserves_originals_and_adds_force_after_native_utf16_order() {
    let dir = root();
    // JS UTF-16 puts an astral character BEFORE U+E000; Rust UTF-8 ordering
    // would pick the wrong largest name. Include false in every source.
    let names = [
        "managed-settings.json",
        "managed-settings.d/\u{1f600}.json",
        "managed-settings.d/\u{e000}.json",
        "managed-mcp.json",
    ];
    let original = b"{\"forceRemoteSettingsRefresh\":false,\"synthetic\":\"must-survive\"}";
    for name in names {
        fs::write(dir.path().join(name), original).unwrap();
    }
    let policy = EndpointPolicy::fixture(dir.path(), &Cancellation::default()).unwrap();
    let staged = tempfile::tempdir().unwrap();
    let mounts = policy
        .stage(staged.path(), &Cancellation::default())
        .unwrap();
    assert_eq!(mounts.len(), names.len() + 1);
    for name in names {
        let (source, _) = mounts
            .iter()
            .find(|(_, guest)| guest == &Path::new(ROOT).join(name))
            .unwrap();
        assert_eq!(fs::read(source).unwrap(), original);
        assert_eq!(
            fs::metadata(source).unwrap().permissions().mode() & 0o777,
            0o400
        );
    }
    let (_, overlay) = mounts
        .iter()
        .find(|(_, guest)| guest.to_string_lossy().ends_with(".tgsum.json"))
        .unwrap();
    assert_eq!(
        overlay,
        &Path::new(ROOT).join("managed-settings.d/\u{e000}.json.tgsum.json")
    );
    assert_eq!(
        fs::read_dir(dir.path().join("managed-settings.d"))
            .unwrap()
            .count(),
        2
    );
    fs::write(dir.path().join("managed-settings.json"), "{}").unwrap();
    assert!(policy
        .stage(staged.path(), &Cancellation::default())
        .is_err());
}
