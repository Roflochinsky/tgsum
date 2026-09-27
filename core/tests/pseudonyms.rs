use tgsum_core::project::ProjectStore;
use tgsum_core::pseudonyms::{PseudonymCategory as Category, PseudonymInput};

fn person<'a>(identity: &'a str, original: &'a str) -> PseudonymInput<'a> {
    PseudonymInput {
        category: Category::Person,
        identity,
        original,
    }
}

#[test]
fn repeated_import_and_expanded_scope_keep_labels_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Synthetic project").unwrap();
    let first = store
        .assign_pseudonyms(
            &project.project_id,
            project.revision,
            &[
                person("telegram:work:user20", "Борис"),
                person("telegram:work:user10", "Алиса"),
                person("telegram:work:user20", "Борис"),
            ],
        )
        .unwrap();
    assert_eq!(first.labels, ["PERSON_0002", "PERSON_0001", "PERSON_0002"]);
    let reference = first.project.pseudonyms.as_ref().unwrap().clone();
    let reopened = ProjectStore::new(root.path());
    let again = reopened
        .assign_pseudonyms(
            &project.project_id,
            first.project.revision,
            &[
                person("telegram:work:user10", "Алиса"),
                person("telegram:work:user20", "Борис"),
            ],
        )
        .unwrap();
    assert_eq!(again.labels, ["PERSON_0001", "PERSON_0002"]);
    assert_eq!(again.project, first.project);
    let expanded = reopened
        .assign_pseudonyms(
            &project.project_id,
            again.project.revision,
            &[
                person("telegram:work:user01", "Вера"),
                person("telegram:work:user20", "Борис Новый"),
            ],
        )
        .unwrap();
    assert_eq!(expanded.labels, ["PERSON_0003", "PERSON_0002"]);
    let old = reopened
        .load_pseudonyms(&project.project_id, &reference)
        .unwrap();
    assert_eq!(old.originals("PERSON_0002").unwrap(), ["Борис"]);
    assert!(old
        .lookup(Category::Person, "telegram:work:user01")
        .is_none());
    let new = reopened
        .load_pseudonyms(
            &project.project_id,
            expanded.project.pseudonyms.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(
        new.originals("PERSON_0002").unwrap(),
        ["Борис", "Борис Новый"]
    );
    assert_eq!(
        new.lookup(Category::Person, "telegram:work:user01"),
        Some("PERSON_0003")
    );
}

#[test]
fn reset_starts_a_new_epoch_without_overwriting_history_or_other_projects() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let first = store.create("First").unwrap();
    let assigned = store
        .assign_pseudonyms(
            &first.project_id,
            0,
            &[
                person("user01", "First original"),
                person("user02", "Second original"),
            ],
        )
        .unwrap();
    let old = assigned.project.pseudonyms.as_ref().unwrap();
    let second = store.create("Second").unwrap();
    assert!(second.pseudonyms.is_none());
    let independent = store
        .assign_pseudonyms(
            &second.project_id,
            0,
            &[person("user02", "Different project original")],
        )
        .unwrap();
    assert_eq!(independent.labels, ["PERSON_0001"]);
    assert_ne!(
        old.epoch(),
        independent.project.pseudonyms.as_ref().unwrap().epoch()
    );
    assert!(store.load_pseudonyms(&second.project_id, old).is_err());
    let reset = store
        .reset_pseudonyms(&first.project_id, assigned.project.revision)
        .unwrap();
    let new = reset.pseudonyms.as_ref().unwrap();
    assert_eq!(new.generation(), 0);
    assert_ne!(new.epoch(), old.epoch());
    assert_ne!(new.id(), old.id());
    assert!(store
        .load_pseudonyms(&first.project_id, new)
        .unwrap()
        .originals("PERSON_0001")
        .is_none());
    let next = store
        .assign_pseudonyms(
            &first.project_id,
            reset.revision,
            &[person("user02", "Second original")],
        )
        .unwrap();
    assert_eq!(next.labels, ["PERSON_0001"]);
    assert_eq!(
        store
            .load_pseudonyms(&first.project_id, old)
            .unwrap()
            .originals("PERSON_0001")
            .unwrap(),
        ["First original"]
    );
    assert_eq!(
        store
            .load_pseudonyms(
                &second.project_id,
                independent.project.pseudonyms.as_ref().unwrap()
            )
            .unwrap()
            .originals("PERSON_0001")
            .unwrap(),
        ["Different project original"]
    );
    assert_eq!(
        store
            .reset_pseudonyms(&first.project_id, reset.revision)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn excessive_aliases_fail_without_publishing_or_disclosing_originals() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Synthetic limits").unwrap();
    let assigned = store
        .assign_pseudonyms(&project.project_id, 0, &[person("user", "Original")])
        .unwrap();
    let aliases: Vec<_> = (0..256).map(|n| format!("PRIVATE_ALIAS_{n}")).collect();
    let inputs: Vec<_> = aliases.iter().map(|a| person("user", a)).collect();
    let error = store
        .assign_pseudonyms(&project.project_id, assigned.project.revision, &inputs)
        .err()
        .expect("257 aliases must exceed the per-identity budget");
    assert!(!error.to_string().contains("PRIVATE_ALIAS"));
    assert_eq!(store.open(&project.project_id).unwrap(), assigned.project);
    let old = store
        .load_pseudonyms(
            &project.project_id,
            assigned.project.pseudonyms.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(old.originals("PERSON_0001").unwrap(), ["Original"]);
}

#[test]
fn categories_and_canonical_identity_keep_equal_display_names_distinct() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Identity namespaces").unwrap();
    let assigned = store
        .assign_pseudonyms(
            &project.project_id,
            0,
            &[
                person("telegram:work:user1", "Same name"),
                person("telegram:personal:user1", "Same name"),
                person("teams:work:user1", "Same name"),
                PseudonymInput {
                    category: Category::Host,
                    identity: "telegram:work:user1",
                    original: "Same name",
                },
            ],
        )
        .unwrap();
    assert_eq!(
        assigned.labels,
        ["PERSON_0003", "PERSON_0002", "PERSON_0001", "HOST_0001"]
    );
    let repeated = store
        .assign_pseudonyms(&project.project_id, assigned.project.revision, &[])
        .unwrap();
    assert_eq!(repeated.project, assigned.project);
    assert!(repeated.labels.is_empty());
}

#[test]
fn concurrent_allocations_have_one_winner_and_retry_extends_the_committed_map() {
    use std::sync::{Arc, Barrier};
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Concurrent edits").unwrap();
    let initial = store
        .assign_pseudonyms(&project.project_id, 0, &[person("first", "First")])
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = ["alice", "bob"]
        .into_iter()
        .map(|identity| {
            let store = store.clone();
            let project = initial.project.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                (
                    identity,
                    store.assign_pseudonyms(
                        &project.project_id,
                        project.revision,
                        &[person(identity, identity)],
                    ),
                )
            })
        })
        .collect();
    let mut winner = None;
    let mut loser = None;
    for thread in threads {
        let (identity, result) = thread.join().unwrap();
        match result {
            Ok(assignment) => {
                assert!(winner.is_none());
                winner = Some((identity, assignment.project));
            }
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
                loser = Some(identity);
            }
        }
    }
    let (winner_id, winner) = winner.unwrap();
    let reopened = ProjectStore::new(root.path());
    assert_eq!(reopened.open(&project.project_id).unwrap(), winner);
    let mapping = reopened
        .load_pseudonyms(&project.project_id, winner.pseudonyms.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        mapping.lookup(Category::Person, winner_id),
        Some("PERSON_0002")
    );
    assert!(mapping.lookup(Category::Person, loser.unwrap()).is_none());
    let retried = reopened
        .assign_pseudonyms(
            &project.project_id,
            winner.revision,
            &[person(loser.unwrap(), "Retry")],
        )
        .unwrap();
    assert_eq!(retried.labels, ["PERSON_0003"]);
}

#[test]
fn bounded_batches_fail_before_publication_with_value_free_errors() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Limits").unwrap();
    let identity = "PRIVATE".repeat(586);
    let original = "PRIVATE".repeat(2341);
    for input in [
        person("", "Original"),
        person("id", ""),
        person(&identity, "Original"),
        person("id", &original),
    ] {
        let error = store
            .assign_pseudonyms(&project.project_id, 0, &[input])
            .err()
            .unwrap();
        assert!(!error.to_string().contains("PRIVATE"));
        assert_eq!(store.open(&project.project_id).unwrap(), project);
    }
    let too_many: Vec<_> = (0..100_001).map(|_| person("id", "Original")).collect();
    assert!(store
        .assign_pseudonyms(&project.project_id, 0, &too_many)
        .is_err());
    // Raw batch size budget, including repeated identities.
    let original = "x".repeat(16 * 1024);
    let too_large: Vec<_> = (0..1024).map(|_| person("id", &original)).collect();
    assert!(store
        .assign_pseudonyms(&project.project_id, 0, &too_large)
        .is_err());
    // Escaped JSON can exceed the serialized budget while raw input fits it.
    let original = "\u{0001}".repeat(16 * 1024);
    let identities: Vec<_> = (0..200).map(|n| format!("identity{n}")).collect();
    let escaped: Vec<_> = identities.iter().map(|id| person(id, &original)).collect();
    assert!(store
        .assign_pseudonyms(&project.project_id, 0, &escaped)
        .is_err());
    assert_eq!(store.open(&project.project_id).unwrap(), project);
}

#[test]
fn private_files_validate_identity_version_digest_and_never_silently_reset() {
    use sha2::{Digest, Sha256};
    use tgsum_core::pseudonyms::MappingRef;
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Damaged storage").unwrap();
    let assigned = store
        .assign_pseudonyms(
            &project.project_id,
            0,
            &[person("PRIVATE_ID", "PRIVATE_ORIGINAL")],
        )
        .unwrap();
    let reference = assigned.project.pseudonyms.as_ref().unwrap();
    let directory = root.path().join(&project.project_id).join("pseudonyms");
    let path = directory.join(format!("{}.json", reference.id()));
    let saved = std::fs::read(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    for (field, value) in [
        ("schema_version", serde_json::json!(99)),
        ("project_id", serde_json::json!("project-other")),
        (
            "id",
            serde_json::json!("map-00000000000000000000000000000000"),
        ),
        ("generation", serde_json::json!(99)),
    ] {
        let mut data: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        data[field] = value;
        let bytes = serde_json::to_vec(&data).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut metadata = serde_json::to_value(reference).unwrap();
        metadata["sha256"] = serde_json::json!(format!("{:x}", Sha256::digest(&bytes)));
        let forged: MappingRef = serde_json::from_value(metadata).unwrap();
        assert!(store.load_pseudonyms(&project.project_id, &forged).is_err());
    }
    std::fs::write(&path, b"PRIVATE_CORRUPT_JSON").unwrap();
    let error = store
        .assign_pseudonyms(
            &project.project_id,
            assigned.project.revision,
            &[person("id", "new")],
        )
        .err()
        .unwrap();
    assert!(!error.to_string().contains("PRIVATE_CORRUPT"));
    assert!(store
        .reset_pseudonyms(&project.project_id, assigned.project.revision)
        .is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(store
        .reset_pseudonyms(&project.project_id, assigned.project.revision)
        .is_err());
    assert_eq!(store.open(&project.project_id).unwrap(), assigned.project);
    std::fs::write(&path, &saved).unwrap();
    assert_eq!(
        store
            .load_pseudonyms(&project.project_id, reference)
            .unwrap()
            .originals("PERSON_0001")
            .unwrap(),
        ["PRIVATE_ORIGINAL"]
    );
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("file.json");
        std::fs::write(&outside_file, &saved).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside_file, &path).unwrap();
        assert!(store
            .load_pseudonyms(&project.project_id, reference)
            .is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        std::os::unix::fs::symlink(outside.path(), &directory).unwrap();
        assert!(store
            .load_pseudonyms(&project.project_id, reference)
            .is_err());
        assert!(store
            .reset_pseudonyms(&project.project_id, assigned.project.revision)
            .is_err());
    }
}
