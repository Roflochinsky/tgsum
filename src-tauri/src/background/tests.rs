use super::linux::Configuration;
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn foreground_and_relative_file_requests_survive_before_window_creation() {
    use tauri::Manager;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("result.json"), b"{}").unwrap();
    let app = tauri::test::mock_builder()
        .manage(super::BackgroundState::for_launch(true, false))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    super::forward_launch(
        app.handle(),
        &["tgsum".into(), "result.json".into()],
        root.path().to_str().unwrap(),
    );
    let state = app.state::<super::BackgroundState>();
    assert!(state
        .foreground_requested
        .load(std::sync::atomic::Ordering::Acquire));
    assert!(app.get_webview_window("main").is_none());
    assert_eq!(
        super::next_launch_export(state.clone()).unwrap(),
        Some(root.path().join("result.json").display().to_string())
    );
    assert_eq!(super::next_launch_export(state).unwrap(), None);
}

#[test]
fn autostart_repeat_and_disable_preserve_original_configuration() {
    let root = tempfile::tempdir().unwrap();
    let configuration = Configuration::fixture(root.path().into(), Default::default());
    assert!(!configuration.enabled().unwrap());
    configuration
        .set(true, std::path::Path::new("/opt/old/tgsum"))
        .unwrap();
    assert!(configuration.enabled().unwrap());
    let original = fs::read(configuration.path()).unwrap();
    configuration
        .set(true, std::path::Path::new("/opt/old/tgsum"))
        .unwrap();
    assert!(configuration
        .set(true, std::path::Path::new("/opt/new/tgsum"))
        .is_err());
    assert_eq!(fs::read(configuration.path()).unwrap(), original);
    assert!(configuration.enabled().unwrap());
    configuration
        .set(false, std::path::Path::new("/irrelevant"))
        .unwrap();
    assert!(!configuration.enabled().unwrap());
    assert_eq!(fs::read(configuration.path()).unwrap(), original);
}

#[test]
fn autostart_never_overwrites_a_foreign_unit_or_link() {
    let root = tempfile::tempdir().unwrap();
    let configuration = Configuration::fixture(root.path().into(), Default::default());
    fs::create_dir_all(configuration.path().parent().unwrap()).unwrap();
    let original = b"[Service]\nExecStart=/user/own/command\n";
    fs::write(configuration.path(), original).unwrap();
    for enabled in [true, false] {
        assert!(configuration
            .set(enabled, std::path::Path::new("/opt/tgsum"))
            .is_err());
    }
    assert_eq!(fs::read(configuration.path()).unwrap(), original);
    assert!(!root.path().join("fixture-autostart-enabled").exists());
    fs::remove_file(configuration.path()).unwrap();
    let target = root.path().join("other-unit");
    fs::write(&target, original).unwrap();
    symlink(&target, configuration.path()).unwrap();
    assert!(configuration
        .set(true, std::path::Path::new("/opt/tgsum"))
        .is_err());
    assert_eq!(fs::read(target).unwrap(), original);
}

#[test]
fn autostart_rejects_a_foreign_fifo_without_blocking() {
    use std::os::unix::fs::FileTypeExt;
    use std::sync::mpsc;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let configuration = Configuration::fixture(root.path().into(), Default::default());
    let directory = configuration.directory(true).unwrap().unwrap();
    rustix::fs::mkfifoat(
        &directory,
        super::linux::UNIT,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    let path = configuration.path();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || sender.send(configuration.enabled()).unwrap());
    let response = receiver.recv_timeout(Duration::from_secs(2));
    if response.is_err() {
        // Release a regressed blocking reader so the RED test itself exits.
        let unblock = rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDWR | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .unwrap();
        worker.join().unwrap();
        drop(unblock);
        panic!("foreign FIFO blocked the service-status worker");
    }
    worker.join().unwrap();
    assert!(response.unwrap().is_err());
    assert!(fs::symlink_metadata(path).unwrap().file_type().is_fifo());
}

#[test]
fn autostart_rejects_invalid_executable_before_creating_configuration() {
    let root = tempfile::tempdir().unwrap();
    let configuration = Configuration::fixture(root.path().join("missing"), Default::default());
    for path in ["relative/tgsum", "/opt/tgsum\nExecStart=/unexpected"] {
        assert!(configuration.set(true, std::path::Path::new(path)).is_err());
    }
    assert!(!root.path().join("missing").exists());
}

#[test]
fn autostart_does_not_create_directories_through_a_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("linked")).unwrap();
    let configuration =
        Configuration::fixture(root.path().join("linked/missing"), Default::default());
    assert!(configuration
        .set(true, std::path::Path::new("/opt/tgsum"))
        .is_err());
    assert!(!outside.path().join("missing").exists());
}

#[test]
fn publication_never_clobbers_a_late_foreign_leaf() {
    let root = tempfile::tempdir().unwrap();
    let configuration = Configuration::fixture(root.path().into(), Default::default());
    let directory = configuration.directory(true).unwrap().unwrap();
    fs::write(configuration.path(), b"late foreign contents").unwrap();
    assert!(super::linux::publish_new(&directory, b"new unit").is_err());
    assert_eq!(
        fs::read(configuration.path()).unwrap(),
        b"late foreign contents"
    );
}

#[test]
fn publication_remains_confined_after_parent_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let configuration = Configuration::fixture(root.path().into(), Default::default());
    let directory = configuration.directory(true).unwrap().unwrap();
    let old = root.path().join("retained-user-dir");
    fs::rename(configuration.path().parent().unwrap(), &old).unwrap();
    symlink(outside.path(), configuration.path().parent().unwrap()).unwrap();
    super::linux::publish_new(&directory, b"only original directory").unwrap();
    assert_eq!(
        fs::read(old.join(super::linux::UNIT)).unwrap(),
        b"only original directory"
    );
    assert!(!outside.path().join(super::linux::UNIT).exists());
    assert!(configuration
        .set(false, std::path::Path::new("/opt/tgsum"))
        .is_err());
}
