mod common;

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use common::{big_unit, msg, unit};
use serde_json::json;
use tgsum_core::write_units;

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

fn part_numbers(dir: &Path, stem: &str) -> Vec<usize> {
    files(dir)
        .iter()
        .filter_map(|f| {
            f.strip_prefix(&format!("{stem}.part-"))?
                .strip_suffix(".md")?
                .parse()
                .ok()
        })
        .collect()
}

#[test]
fn one_file_per_single_part_unit_part_suffix_for_split_units() {
    let dir = tempfile::tempdir().unwrap();
    let units = [
        unit(
            "A",
            None,
            vec![msg(
                json!({ "id": 1, "type": "message", "date": "2026-06-18T09:00:00", "from": "X", "text": "hi" }),
            )],
        ),
        big_unit("B", 30, 'y'),
    ];
    let written = write_units(&units, dir.path(), 60).unwrap();
    let names = files(dir.path());
    assert!(names.contains(&"A.md".to_owned()));
    assert!(names.contains(&"B.part-1.md".to_owned()));
    assert_eq!(written.len(), names.len());
    assert!(fs::read_to_string(dir.path().join("A.md"))
        .unwrap()
        .contains("# Чат: A"));
}

#[test]
fn deduplicates_colliding_filenames_instead_of_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let units = [
        unit(
            "Dup",
            None,
            vec![msg(
                json!({ "id": 1, "date": "2026-06-18T09:00:00", "from": "X", "text": "first" }),
            )],
        ),
        unit(
            "Dup",
            None,
            vec![msg(
                json!({ "id": 2, "date": "2026-06-18T09:01:00", "from": "Y", "text": "second" }),
            )],
        ),
        unit(
            "dup",
            None,
            vec![msg(
                json!({ "id": 3, "date": "2026-06-18T09:02:00", "from": "Z", "text": "third" }),
            )],
        ),
    ];
    let written = write_units(&units, dir.path(), 100_000).unwrap();
    assert_eq!(files(dir.path()), ["Dup (2).md", "Dup.md", "dup (3).md"]);
    assert_eq!(written.iter().collect::<HashSet<_>>().len(), 3);
    assert!(fs::read_to_string(dir.path().join("Dup.md"))
        .unwrap()
        .contains("first"));
    assert!(fs::read_to_string(dir.path().join("Dup (2).md"))
        .unwrap()
        .contains("second"));
    assert!(fs::read_to_string(dir.path().join("dup (3).md"))
        .unwrap()
        .contains("third"));
}

#[test]
fn clears_stale_part_files_when_a_rerun_produces_fewer_parts() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("C (2).md"), "unrelated").unwrap();
    write_units(&[big_unit("C", 40, 'z')], dir.path(), 60).unwrap();
    let first = part_numbers(dir.path(), "C").len();
    assert!(first > 2);

    write_units(&[big_unit("C", 6, 'z')], dir.path(), 60).unwrap();
    let after = part_numbers(dir.path(), "C");
    assert!(after.len() < first);
    assert_eq!(after.iter().max().copied(), Some(after.len()));

    // Single-part rerun removes all parts; unrelated files survive.
    write_units(&[big_unit("C", 1, 'z')], dir.path(), 60).unwrap();
    assert_eq!(files(dir.path()), ["C (2).md", "C.md"]);
}

#[test]
fn creates_missing_output_folders() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a/b/c");
    write_units(&[big_unit("N", 1, 'q')], &out, 100_000).unwrap();
    assert_eq!(files(&out), ["N.md"]);
}
