//! Publish only a verified generated package to a private GitHub repository.
//! Authentication belongs to gh; credentials are never copied into TGSUM.
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tgsum_core::local_package::PackageState;
use tgsum_core::project::ProjectStore;

fn failure(message: impl ToString) -> io::Error {
    io::Error::other(message.to_string())
}

fn run(mut command: Command, cancelled: &impl Fn() -> bool) -> io::Result<String> {
    if cancelled() {
        return Err(failure("GitHub: операция отменена."));
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut output = tempfile::tempfile()?;
    // Provider diagnostics may contain URLs/configuration; only an action label
    // and exit status escape this subprocess boundary.
    let errors = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(errors.try_clone()?)
        .spawn()?;
    let start = Instant::now();
    loop {
        if cancelled()
            || start.elapsed() > Duration::from_secs(90)
            || output.metadata()?.len() > 1024 * 1024
            || errors.metadata()?.len() > 1024 * 1024
        {
            #[cfg(target_os = "linux")]
            {
                // git may own HTTPS/credential helper children. Stop only the
                // isolated group created above, never another app's process.
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure(
                "GitHub: операция отменена или превышен лимит ожидания.",
            ));
        }
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(failure(format!(
                    "GitHub: команда завершилась с кодом {}. Проверьте сеть и gh auth status.",
                    status.code().unwrap_or(-1)
                )));
            }
            output.seek(SeekFrom::Start(0))?;
            let mut text = String::new();
            output.take(1024 * 1024).read_to_string(&mut text)?;
            return Ok(text.trim().into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn git(directory: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(key);
    }
    command
        .current_dir(directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "credential.helper=",
            "-c",
            "credential.https://github.com.helper=!gh auth git-credential",
        ])
        .args(args);
    command
}

fn private(repo: &str, cancelled: &impl Fn() -> bool) -> io::Result<()> {
    let mut command = Command::new("gh");
    command.args(["api", &format!("repos/{repo}"), "--jq", ".private"]);
    if run(command, cancelled)? != "true" {
        return Err(failure(
            "Публикация остановлена: репозиторий GitHub должен быть приватным.",
        ));
    }
    Ok(())
}

pub(super) fn publish(
    store: &ProjectStore,
    state: &PackageState,
    cancelled: impl Fn() -> bool,
) -> io::Result<String> {
    if !store.local_package_allows_publication(&state.project_id)? {
        return Err(failure("Этот пакет доступен только локально."));
    }
    let repo = state
        .settings
        .github_repository
        .as_deref()
        .ok_or_else(|| failure("GitHub не настроен."))?;
    // The same validator is enforced when settings are saved. Revalidate the
    // persisted value before constructing a command argument/remote URL.
    if repo.split('/').count() != 2
        || repo.split('/').any(|part| {
            part.is_empty()
                || part.starts_with('-')
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    {
        return Err(failure("Некорректный адрес репозитория."));
    }
    private(repo, &cancelled)?;
    let staging = tempfile::tempdir()?;
    run(
        git(
            staging.path(),
            &[
                "clone",
                "--depth",
                "1",
                "--no-tags",
                &format!("https://github.com/{repo}.git"),
                "repo",
            ],
        ),
        &cancelled,
    )?;
    let repo_path = staging.path().join("repo");
    let commit = publish_worktree(store, &state.project_id, &repo_path, &cancelled)?;
    let latest = store
        .local_package(&state.project_id)?
        .ok_or_else(|| failure("package settings removed"))?;
    if latest.settings.github_repository != state.settings.github_repository
        || latest.ready.as_ref().map(|r| &r.content_sha256)
            != state.ready.as_ref().map(|r| &r.content_sha256)
    {
        return Err(failure(
            "Пакет изменился перед публикацией. Повторите обновление.",
        ));
    }
    if !store.local_package_allows_publication(&state.project_id)? {
        return Err(failure("Этот пакет доступен только локально."));
    }
    private(repo, &cancelled)?;
    // A normal fast-forward push only. A race with another writer is retried
    // from a fresh clone; unrelated changes are never force-overwritten.
    run(git(&repo_path, &["push", "origin", "HEAD"]), &cancelled)?;
    Ok(commit)
}

fn publish_worktree(
    store: &ProjectStore,
    id: &str,
    worktree: &Path,
    cancelled: &impl Fn() -> bool,
) -> io::Result<String> {
    let parent = worktree.join("packages");
    match fs::create_dir(&parent) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    if !fs::symlink_metadata(&parent)?.is_dir() {
        return Err(failure("GitHub packages must be a real directory."));
    }
    let destination = parent.join(id);
    if let Ok(meta) = fs::symlink_metadata(&destination) {
        if !meta.is_dir() {
            return Err(failure("GitHub package path is not a directory."));
        }
        let owner_path = destination.join(".tgsum-owner");
        if !fs::symlink_metadata(&owner_path)?.is_file() || fs::read_to_string(&owner_path)? != id {
            return Err(failure("GitHub: папка не принадлежит этому проекту TGSUM."));
        }
        fs::remove_dir_all(&destination)?;
    }
    fs::create_dir(&destination)?;
    store.copy_local_package(id, &destination, cancelled)?;
    fs::write(destination.join(".tgsum-owner"), id)?;
    let relative = format!("packages/{id}");
    run(git(worktree, &["add", "--all", "--", &relative]), cancelled)?;
    let changed = run(
        git(worktree, &["diff", "--cached", "--name-only"]),
        cancelled,
    )?;
    if !changed.is_empty() {
        let mut command = git(
            worktree,
            &[
                "-c",
                "user.name=TGSUM",
                "-c",
                "user.email=tgsum@localhost",
                "commit",
                "-m",
                "Update prepared TGSUM package",
            ],
        );
        command
            .env_remove("GIT_AUTHOR_NAME")
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("GIT_COMMITTER_NAME")
            .env_remove("GIT_COMMITTER_EMAIL");
        run(command, cancelled)?;
    }
    run(git(worktree, &["rev-parse", "HEAD"]), cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn cancellation_stops_owned_git_helper_group() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("child-pid");
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & echo $! > \"$1\"; wait", "fixture"])
            .arg(&marker);
        let result = run(command, &|| marker.exists());
        assert!(result.is_err());
        let pid = fs::read_to_string(marker).unwrap();
        let process = format!("/proc/{}/stat", pid.trim());
        let deadline = Instant::now() + Duration::from_secs(2);
        while let Ok(stat) = fs::read_to_string(&process) {
            if stat
                .split_once(") ")
                .is_some_and(|(_, tail)| tail.starts_with('Z'))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned helper survived cancellation"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(unix)]
    #[test]
    fn package_git_roundtrip_replaces_only_owned_subtree_and_repeat_has_no_commit() {
        use tgsum_core::local_package::PackageSettings;
        use tgsum_core::project::{ProjectChange, ProjectSource};
        use tgsum_core::snapshot::SourceScope;
        let tmp = tempfile::tempdir().unwrap();
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        // Native pickers provide canonical directories. Match that contract
        // even when the OS temporary directory has a symlink (macOS /var).
        let root_path = tmp.path().canonicalize().unwrap();
        let input_path = input.path().canonicalize().unwrap();
        let output_path = output.path().canonicalize().unwrap();
        let bare = root_path.join("remote.git");
        fs::create_dir(&bare).unwrap();
        run(
            git(&bare, &["init", "--bare", "--initial-branch=main"]),
            &|| false,
        )
        .unwrap();
        run(
            git(&root_path, &["clone", bare.to_str().unwrap(), "worktree"]),
            &|| false,
        )
        .unwrap();
        let worktree = root_path.join("worktree");
        fs::write(worktree.join("unrelated.txt"), "preserve").unwrap();
        run(git(&worktree, &["add", "unrelated.txt"]), &|| false).unwrap();
        run(
            git(
                &worktree,
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@localhost",
                    "commit",
                    "-m",
                    "initial",
                ],
            ),
            &|| false,
        )
        .unwrap();
        let store = ProjectStore::new(root_path.join("private"));
        let p = store.create("Synthetic Git package").unwrap();
        let p = store
            .update(
                &p.project_id,
                p.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "selected".into(),
                    connector_id: "telegram_json".into(),
                    scope: SourceScope::telegram("fixture", "111"),
                    archive_path: None,
                    latest_snapshot_id: None,
                    selection: Default::default(),
                }),
            )
            .unwrap();
        store
            .configure_local_package(
                &p.project_id,
                p.revision,
                PackageSettings {
                    source_ids: vec!["selected".into()],
                    input_directory: input_path.clone(),
                    output_directory: output_path,
                    automatic: false,
                    include_images: true,
                    include_office: true,
                    github_repository: None,
                },
            )
            .unwrap();
        fs::write(input_path.join("image.png"), b"synthetic-image").unwrap();
        fs::write(input_path.join("result.json"), r#"{"id":111,"messages":[{"id":1,"text":"password=GIT_SYNTHETIC_SECRET","photo":"image.png"}]}"#).unwrap();
        let ready = store
            .refresh_local_package(&p.project_id, 1, || false)
            .unwrap();
        assert_eq!(ready.phase, "ready", "{}", ready.message);
        let first = publish_worktree(&store, &p.project_id, &worktree, &|| false).unwrap();
        run(git(&worktree, &["push", "origin", "HEAD"]), &|| false).unwrap();
        let repeat = publish_worktree(&store, &p.project_id, &worktree, &|| false).unwrap();
        assert_eq!(first, repeat);
        assert_eq!(
            fs::read_to_string(worktree.join("unrelated.txt")).unwrap(),
            "preserve"
        );
        let folder = worktree.join("packages").join(&p.project_id);
        assert!(folder.join("file-00001.png").exists());
        assert!(!fs::read_to_string(folder.join("context-00001.md"))
            .unwrap()
            .contains("GIT_SYNTHETIC_SECRET"));
        assert!(!folder.join("result.json").exists());
        fs::write(
            input_path.join("result.json"),
            r#"{"id":111,"messages":[{"id":1,"text":"Updated without image"}]}"#,
        )
        .unwrap();
        assert_eq!(
            store
                .refresh_local_package(&p.project_id, 2, || false)
                .unwrap()
                .phase,
            "ready"
        );
        let second = publish_worktree(&store, &p.project_id, &worktree, &|| false).unwrap();
        assert_ne!(first, second);
        assert!(!folder.join("file-00001.png").exists());
        let p = store.open(&p.project_id).unwrap();
        let p = store
            .update(
                &p.project_id,
                p.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "second".into(),
                    connector_id: "telegram_json".into(),
                    scope: SourceScope::telegram("fixture", "222"),
                    archive_path: None,
                    latest_snapshot_id: None,
                    selection: Default::default(),
                }),
            )
            .unwrap();
        let mut settings = store
            .local_package(&p.project_id)
            .unwrap()
            .unwrap()
            .settings;
        settings.source_ids.push("second".into());
        store
            .configure_local_package(&p.project_id, p.revision, settings)
            .unwrap();
        fs::write(input.path().join("result.json"), r#"{"chats":{"list":[{"id":111,"messages":[{"id":1,"text":"Updated first chat"}]},{"id":222,"messages":[{"id":1,"text":"Selected second chat"}]}]}}"#).unwrap();
        let ready = store
            .refresh_local_package(&p.project_id, 3, || false)
            .unwrap();
        assert_eq!(ready.phase, "ready", "{}", ready.message);
        assert_eq!(ready.ready.unwrap().conversations, 2);
        let third = publish_worktree(&store, &p.project_id, &worktree, &|| false).unwrap();
        assert_ne!(third, second);
        let text = fs::read_to_string(folder.join("context-00001.md")).unwrap();
        assert!(text.contains("Updated first chat"));
        assert!(text.contains("Selected second chat"));
        assert_eq!(
            third,
            publish_worktree(&store, &p.project_id, &worktree, &|| false).unwrap()
        );
        run(git(&worktree, &["push", "origin", "HEAD"]), &|| false).unwrap();
        assert_eq!(
            run(git(&bare, &["rev-parse", "refs/heads/main"]), &|| false).unwrap(),
            third
        );
        assert!(publish_worktree(&store, &p.project_id, &worktree, &|| true).is_err());
        assert_eq!(
            fs::read_to_string(worktree.join("unrelated.txt")).unwrap(),
            "preserve"
        );
    }
    #[test]
    fn git_commands_do_not_load_user_hooks_or_shell_interpolate_repository_data() {
        let tmp = tempfile::tempdir().unwrap();
        run(git(tmp.path(), &["init", "--initial-branch=main"]), &|| {
            false
        })
        .unwrap();
        fs::write(tmp.path().join("unrelated.txt"), "preserve").unwrap();
        run(git(tmp.path(), &["add", "unrelated.txt"]), &|| false).unwrap();
        run(
            git(
                tmp.path(),
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@localhost",
                    "commit",
                    "-m",
                    "initial",
                ],
            ),
            &|| false,
        )
        .unwrap();
        assert!(!run(git(tmp.path(), &["rev-parse", "HEAD"]), &|| false)
            .unwrap()
            .is_empty());
        assert!(run(git(tmp.path(), &["status"]), &|| true).is_err());
    }
}
