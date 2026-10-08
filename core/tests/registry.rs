use std::io::Cursor;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde_json::{json, Value};
use tgsum_core::registry::{
    load_importer, Compatibility, ObservedVersion, Operation, QualificationScope, Registry,
    ReviewState, ValidationMode, VersionRequirement,
};
use tgsum_core::snapshot::{SnapshotStore, SourceScope};

// Fixed for reproducibility; covers the latest recorded research review.
// Expiry and future-evidence cases deliberately use separate dates.
const INVENTORY_REVIEW_DATE: &str = "2026-10-08";

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
            date(INVENTORY_REVIEW_DATE),
            ValidationMode::Release,
        )
        .unwrap();
    assert!(
        report.passed(),
        "{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    for id in ["telegram_full_export", "telegram_single_export"] {
        let support = registry.technical_support(id, &observed(), date(INVENTORY_REVIEW_DATE));
        assert_eq!(support.implemented_operations, [Operation::LocalImport]);
        assert_eq!(support.qualifications.len(), 1);
        assert_eq!(
            support.qualifications[0].scope,
            QualificationScope::FormatContract
        );
        assert_eq!(support.compatibility, Compatibility::Compatible);
    }
    for id in [
        "discord_bot",
        "telegram_desktop_ui",
        "telegram_linux_notifications",
        "telegram_desktop_debug_logs",
        "nonexistent_profile",
    ] {
        let support = registry.technical_support(id, &observed(), date(INVENTORY_REVIEW_DATE));
        assert!(support.implemented_operations.is_empty());
        assert!(support.qualifications.is_empty());
        assert!(load_importer(id, &observed()).is_err());
    }
}

#[test]
fn future_policy_review_cannot_qualify_an_unimplemented_profile() {
    let mut input = value();
    let profile = input["connectors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "telegram_linux_notifications")
        .unwrap();
    profile["last_policy_reviewed_at"] = "2026-10-09".into();
    let report = parse(&input)
        .validate_repository(
            &repo(),
            &inventory(),
            date(INVENTORY_REVIEW_DATE),
            ValidationMode::Development,
        )
        .unwrap();
    assert!(!report.passed());
    assert!(report.findings.iter().any(|finding| {
        finding.code == "future_review" && finding.connector == "telegram_linux_notifications"
    }));
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
            date(INVENTORY_REVIEW_DATE),
            ValidationMode::Development,
        )
        .unwrap();
    assert!(!report.passed());
    assert!(report
        .findings
        .iter()
        .any(|f| f.code == "operation_not_implemented"));
    let support =
        registry.technical_support("discord_bot", &observed(), date(INVENTORY_REVIEW_DATE));
    assert!(support.implementation.is_none() && support.qualifications.is_empty());

    let mut stale = value();
    stale["connectors"][0]["implementation"]["revision"] = "old-revision".into();
    let stale = parse(&stale);
    let support = stale.technical_support(
        "telegram_full_export",
        &observed(),
        date(INVENTORY_REVIEW_DATE),
    );
    assert_eq!(support.implemented_operations, [Operation::LocalImport]);
    assert!(support.qualifications.is_empty());
    assert!(!stale
        .validate_repository(
            &repo(),
            &inventory(),
            date(INVENTORY_REVIEW_DATE),
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
        .technical_support(
            "telegram_full_export",
            &runtime,
            date(INVENTORY_REVIEW_DATE)
        )
        .qualifications
        .is_empty());
    assert!(!registry
        .validate_repository(
            &repo(),
            &inventory(),
            date(INVENTORY_REVIEW_DATE),
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
    let valid = command(true, INVENTORY_REVIEW_DATE);
    assert!(
        valid.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&valid.stdout),
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
                date(INVENTORY_REVIEW_DATE),
                ValidationMode::Release
            )
            .unwrap()
            .passed());
        if field == "verified_at" {
            assert!(registry
                .technical_support(
                    "telegram_full_export",
                    &observed(),
                    date(INVENTORY_REVIEW_DATE)
                )
                .qualifications
                .is_empty());
        }
    }
}

#[test]
fn review_reminder_cli_includes_upcoming_and_due_work_without_updating_dates() {
    let before = std::fs::read(inventory()).unwrap();
    let command = |today: &str, within: &str, release: bool| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_connector-registry"));
        command.arg("--repo").arg(repo()).args([
            "--today",
            today,
            "--reminders",
            "--within-days",
            within,
        ]);
        if release {
            command.arg("--release");
        }
        command.output().unwrap()
    };
    let upcoming = command("2026-10-20", "6", false);
    assert!(
        upcoming.status.success(),
        "{}",
        String::from_utf8_lossy(&upcoming.stderr)
    );
    let upcoming: Value = serde_json::from_slice(&upcoming.stdout).unwrap();
    let rows = upcoming["reminders"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r["days_until_due"] == 6));
    assert!(rows
        .iter()
        .all(|r| !r["owner"].as_str().unwrap().is_empty()));
    assert!(!rows
        .iter()
        .any(|r| r["connector"] == "telegram_full_export"));
    let before_due = command("2026-10-20", "5", false);
    let report: Value = serde_json::from_slice(&before_due.stdout).unwrap();
    assert!(report["reminders"].as_array().unwrap().is_empty());
    let expired = command("2027-01-01", "0", false);
    assert!(expired.status.success());
    let expired: Value = serde_json::from_slice(&expired.stdout).unwrap();
    let rows = expired["reminders"].as_array().unwrap();
    assert_eq!(rows.len(), value()["connectors"].as_array().unwrap().len());
    assert!(rows.iter().all(|r| r["state"] == "due"));
    assert_eq!(
        rows.iter()
            .filter(|r| r["shipping_implementation"] == true)
            .count(),
        2
    );
    assert_eq!(command("2027-01-01", "0", true).status.code(), Some(1));
    assert_eq!(command("2026-10-20", "-1", false).status.code(), Some(2));
    assert_eq!(command("2026-10-20", "366", false).status.code(), Some(2));
    assert_eq!(std::fs::read(inventory()).unwrap(), before);
}

#[test]
fn missing_successful_review_stays_unknown_even_with_a_future_deadline() {
    let mut input = value();
    input["connectors"][0]["last_policy_reviewed_at"] = Value::Null;
    input["connectors"][0]["review_level"] = "not_reviewed".into();
    let registry = parse(&input);
    let before = serde_json::to_vec(&registry).unwrap();
    let rows = registry.review_reminders(date(INVENTORY_REVIEW_DATE), 0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ReviewState::Unknown);
    assert!(rows[0].last_policy_reviewed_at.is_none());
    assert!(rows[0].shipping_implementation);
    assert_eq!(serde_json::to_vec(&registry).unwrap(), before);
    assert!(load_importer("telegram_full_export", &observed()).is_ok());
}
