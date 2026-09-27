use std::io::Cursor;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde_json::{json, Value};
use tgsum_core::registry::{
    load_importer, Compatibility, ObservedVersion, Operation, QualificationScope, Registry,
    ReviewState, ValidationMode, VersionRequirement,
};
use tgsum_core::snapshot::{SnapshotStore, SourceScope};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .into()
}
fn inventory() -> PathBuf {
    repo().join("docs/connectors/registry.json")
}
fn value() -> Value {
    serde_json::from_slice(&std::fs::read(inventory()).unwrap()).unwrap()
}
fn parse(value: &Value) -> Registry {
    Registry::parse(&serde_json::to_vec(value).unwrap()).unwrap()
}
fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}
fn observed() -> ObservedVersion<'static> {
    ObservedVersion {
        format_id: Some("telegram_desktop_json"),
        ..Default::default()
    }
}

#[test]
fn repository_inventory_binds_qualified_formats_to_real_normalizers() {
    let registry = parse(&value());
    let report = registry
        .validate_repository(
            &repo(),
            &inventory(),
            date("2026-09-27"),
            ValidationMode::Release,
        )
        .unwrap();
    assert!(
        report.passed(),
        "{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    for id in ["telegram_full_export", "telegram_single_export"] {
        let support = registry.technical_support(id, &observed(), date("2026-09-27"));
        assert_eq!(support.implemented_operations, [Operation::LocalImport]);
        assert_eq!(support.qualifications.len(), 1);
        assert_eq!(
            support.qualifications[0].scope,
            QualificationScope::FormatContract
        );
        assert_eq!(support.compatibility, Compatibility::Compatible);
    }
    for id in ["discord_bot", "telegram_desktop_ui", "nonexistent_profile"] {
        let support = registry.technical_support(id, &observed(), date("2026-09-27"));
        assert!(support.implemented_operations.is_empty());
        assert!(support.qualifications.is_empty());
        assert!(load_importer(id, &observed()).is_err());
    }
}

#[test]
fn expiry_and_ai_restrictions_require_release_review_but_preserve_local_import() {
    let registry = parse(&value());
    let future = date("2027-01-01");
    let report = registry
        .validate_repository(&repo(), &inventory(), future, ValidationMode::Development)
        .unwrap();
    assert!(report.passed());
    assert!(!registry
        .validate_repository(&repo(), &inventory(), future, ValidationMode::Release)
        .unwrap()
        .passed());
    let support = registry.technical_support("telegram_single_export", &observed(), future);
    assert_eq!(support.review, ReviewState::Due);
    assert_eq!(support.implemented_operations, [Operation::LocalImport]);
    let importer = load_importer("telegram_single_export", &observed()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    let scope = SourceScope::telegram("synthetic", "42");
    let raw = r#"{"id":42,"type":"personal_chat","messages":[{"id":1,"text":"Preserved local content"}]}"#;
    store
        .import(
            importer.as_ref(),
            "expired-review",
            &scope,
            &mut Cursor::new(raw),
        )
        .unwrap();
    assert_eq!(
        store.load("expired-review").unwrap().messages[0].text,
        "Preserved local content"
    );
}

#[test]
fn registry_flags_and_borrowed_implementation_cannot_create_a_connector_or_upgrade_evidence() {
    let mut input = value();
    let qualification = input["connectors"][0]["qualifications"].clone();
    let implementation = input["connectors"][0]["implementation"].clone();
    let fake = input["connectors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "discord_bot")
        .unwrap();
    fake["support_status"] = "supported".into();
    fake["enabled_operations"] = json!(["local_import", "acquisition", "cloud_inference"]);
    fake["implementation"] = implementation;
    fake["qualifications"] = qualification;
    let registry = parse(&input);
    let report = registry
        .validate_repository(
            &repo(),
            &inventory(),
            date("2026-09-27"),
            ValidationMode::Development,
        )
        .unwrap();
    assert!(!report.passed());
    assert!(report
        .findings
        .iter()
        .any(|f| f.code == "operation_not_implemented"));
    let support = registry.technical_support("discord_bot", &observed(), date("2026-09-27"));
    assert!(support.implementation.is_none() && support.qualifications.is_empty());

    let mut stale = value();
    stale["connectors"][0]["implementation"]["revision"] = "old-revision".into();
    let stale = parse(&stale);
    let support = stale.technical_support("telegram_full_export", &observed(), date("2026-09-27"));
    assert_eq!(support.implemented_operations, [Operation::LocalImport]);
    assert!(support.qualifications.is_empty());
    assert!(!stale
        .validate_repository(
            &repo(),
            &inventory(),
            date("2026-09-27"),
            ValidationMode::Development
        )
        .unwrap()
        .passed());

    let mut hidden = value();
    hidden["connectors"][0]["enabled_operations"] = json!([]);
    hidden["connectors"][0]["support_status"] = "research".into();
    assert!(!parse(&hidden)
        .validate_repository(
            &repo(),
            &inventory(),
            date("2027-01-01"),
            ValidationMode::Release
        )
        .unwrap()
        .passed());

    let mut forged = value();
    forged["connectors"][0]["qualifications"][0]["scope"] = "real_client".into();
    forged["connectors"][0]["qualifications"][0]["client"] =
        json!({"version":"7.2.9","os":"linux"});
    let runtime = ObservedVersion {
        format_id: Some("telegram_desktop_json"),
        client_version: Some("7.2.9"),
        os: Some("linux"),
    };
    let registry = parse(&forged);
    assert!(registry
        .technical_support("telegram_full_export", &runtime, date("2026-09-27"))
        .qualifications
        .is_empty());
    assert!(!registry
        .validate_repository(
            &repo(),
            &inventory(),
            date("2026-09-27"),
            ValidationMode::Release
        )
        .unwrap()
        .passed());
}

#[test]
fn version_compatibility_requires_exact_observed_format_and_client_os_pair() {
    let requirement = VersionRequirement {
        format_id: "synthetic_export",
        clients: &[("1.2.3", "linux"), ("1.2.4", "windows")],
    };
    let mut input = ObservedVersion {
        format_id: Some("synthetic_export"),
        ..Default::default()
    };
    assert_eq!(requirement.check(&input), Compatibility::Unknown);
    input.client_version = Some("1.2.3");
    input.os = Some("linux");
    assert_eq!(requirement.check(&input), Compatibility::Compatible);
    input.os = Some("windows");
    assert_eq!(requirement.check(&input), Compatibility::Unsupported);
    input.client_version = Some("1.2.4");
    assert_eq!(requirement.check(&input), Compatibility::Compatible);
    input.format_id = Some("different_format");
    assert_eq!(requirement.check(&input), Compatibility::Unsupported);
    assert!(load_importer("telegram_single_export", &input).is_err());
    assert!(load_importer("telegram_single_export", &ObservedVersion::default()).is_err());
}

#[test]
fn strict_schema_rejects_missing_unknown_duplicate_and_invalid_review_fields() {
    let mut invalids = Vec::new();
    let mut v = value();
    let duplicate = v["connectors"][0].clone();
    v["connectors"].as_array_mut().unwrap().push(duplicate);
    invalids.push(v);
    let mut v = value();
    v["schema_version"] = 99.into();
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["unexpected"] = true.into();
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]
        .as_object_mut()
        .unwrap()
        .remove("implementation");
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["next_review_due_at"] = "2026-02-30".into();
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["next_review_due_at"] = "2028-01-01".into();
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["owner"] = " ".into();
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["sources"] = json!(["file:///local"]);
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["enabled_operations"] = json!(["local_import", "local_import"]);
    invalids.push(v);
    let mut v = value();
    v["connectors"][0]["qualifications"][0]["scope"] = "real_client".into();
    invalids.push(v);
    for input in invalids {
        assert!(Registry::parse(&serde_json::to_vec(&input).unwrap()).is_err());
    }
}

#[test]
fn release_command_has_reproducible_exit_codes_and_never_rewrites_inventory() {
    let bytes = std::fs::read(inventory()).unwrap();
    let command = |release: bool, date: &str| {
        let mut process = std::process::Command::new(env!("CARGO_BIN_EXE_connector-registry"));
        process.arg("--repo").arg(repo()).args(["--today", date]);
        if release {
            process.arg("--release");
        }
        process.output().unwrap()
    };
    let valid = command(true, "2026-09-27");
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    let output: Value = serde_json::from_slice(&valid.stdout).unwrap();
    assert!(output["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["profile_id"] == "telegram_single_export"
            && c["qualifications"][0]["scope"] == "format_contract"));
    assert_eq!(command(true, "2027-01-01").status.code(), Some(1));
    assert!(command(false, "2027-01-01").status.success());
    assert_eq!(command(false, "2026-02-30").status.code(), Some(2));
    assert_eq!(std::fs::read(inventory()).unwrap(), bytes);
}

#[test]
fn missing_or_escaped_artifacts_and_future_evidence_cannot_pass_release_validation() {
    for (field, bad) in [
        ("evidence", "../../../outside.md"),
        ("evidence", "../missing-document.md"),
        ("verified_at", "2030-01-01"),
    ] {
        let mut input = value();
        input["connectors"][0]["qualifications"][0][field] = bad.into();
        let registry = parse(&input);
        assert!(!registry
            .validate_repository(
                &repo(),
                &inventory(),
                date("2026-09-27"),
                ValidationMode::Release
            )
            .unwrap()
            .passed());
        if field == "verified_at" {
            assert!(registry
                .technical_support("telegram_full_export", &observed(), date("2026-09-27"))
                .qualifications
                .is_empty());
        }
    }
}
