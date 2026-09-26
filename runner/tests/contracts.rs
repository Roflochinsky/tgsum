use std::ffi::OsStr;
use std::fs;

use tgsum_runner::{
    discover, AdapterContract, AuthAvailability, Cancellation, Invocation, OfflineRunner,
    RunnerError, RuntimeSpec,
};

#[test]
fn discovery_does_not_execute_or_claim_authentication() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("fake-agent");
    fs::write(&executable, "this is not executable code").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let candidates = discover(
        OsStr::new("fake-agent"),
        &[directory.path().into(), directory.path().into()],
    )
    .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].path, executable.canonicalize().unwrap());
    assert_eq!(candidates[0].authentication, AuthAvailability::Unknown);
    assert_eq!(candidates[0].version, None);
    assert!(discover(OsStr::new("../fake-agent"), &[directory.path().into()]).is_err());
    assert!(discover(OsStr::new("fake-agent"), &["relative".into()]).is_err());
}

#[test]
fn unknown_profile_cannot_execute_even_a_valid_host_program() {
    let result = OfflineRunner::qualify(
        AdapterContract {
            id: "unknown".into(),
            isolation_profile: "read-only-is-enough".into(),
            version_probe: Invocation::default(),
            expected_version_output: b"1".to_vec(),
        },
        RuntimeSpec {
            executable: std::env::current_exe().unwrap(),
            files: vec![],
        },
        &Cancellation::default(),
    );
    assert!(matches!(
        result,
        Err(RunnerError::ExportOnly("unknown isolation profile"))
    ));
}
