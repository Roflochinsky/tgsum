use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use chrono::NaiveDate;
use serde::Serialize;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    Development,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub connector: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ValidationReport {
    pub findings: Vec<Finding>,
    pub connectors: Vec<TechnicalSupport>,
}

impl ValidationReport {
    pub fn passed(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    fn finding(
        &mut self,
        connector: &str,
        severity: Severity,
        code: &'static str,
        message: impl Into<String>,
    ) {
        self.findings.push(Finding {
            severity,
            connector: connector.into(),
            code,
            message: message.into(),
        });
    }
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains('\0')
}
fn identifier(value: &str) -> bool {
    value.len() >= 2
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Registry {
    pub(super) fn validate_structure(&self) -> io::Result<()> {
        if self.connectors.is_empty() {
            return Err(invalid("empty connector registry"));
        }
        let mut ids = BTreeSet::new();
        for entry in &self.connectors {
            if !identifier(&entry.id) || !ids.insert(&entry.id) {
                return Err(invalid("invalid or duplicate connector ID"));
            }
            if [
                &entry.platform,
                &entry.access_method,
                &entry.credential_type,
                &entry.authorization_scope,
                &entry.selected_scope,
                &entry.owner,
                &entry.evidence,
            ]
            .iter()
            .any(|s| !nonempty(s))
                || entry.open_questions.iter().any(|s| !nonempty(s))
            {
                return Err(invalid(format!("{}: required text is empty", entry.id)));
            }
            if entry.sources.is_empty()
                || entry.sources.iter().any(|source| {
                    url::Url::parse(source).map_or(true, |url| {
                        !matches!(url.scheme(), "http" | "https")
                            || url.host_str().is_none()
                            || !url.username().is_empty()
                            || url.password().is_some()
                    })
                })
            {
                return Err(invalid(format!(
                    "{}: primary sources must be HTTP(S) URLs",
                    entry.id
                )));
            }
            let due = parse_date(&entry.next_review_due_at)?;
            if let Some(last) = &entry.last_policy_reviewed_at {
                let last = parse_date(last)?;
                let cadence = if matches!(
                    entry.access_method.as_str(),
                    "manual_archive" | "local_copy"
                ) {
                    90
                } else {
                    30
                };
                let distance = (due - last).num_days();
                if distance <= 0 || distance > cadence {
                    return Err(invalid(format!(
                        "{}: review interval must be 1..={cadence} days",
                        entry.id
                    )));
                }
            }
            if entry.review_level != ReviewLevel::NotReviewed
                && entry.last_policy_reviewed_at.is_none()
            {
                return Err(invalid(format!(
                    "{}: reviewed entry lacks successful review date",
                    entry.id
                )));
            }
            let mut operations = Vec::new();
            for operation in &entry.enabled_operations {
                if operations.contains(operation) {
                    return Err(invalid("duplicate enabled operation"));
                }
                operations.push(*operation);
            }
            if let Some(implementation) = &entry.implementation {
                if !identifier(&implementation.id) || !nonempty(&implementation.revision) {
                    return Err(invalid("invalid implementation reference"));
                }
            }
            for (index, q) in entry.qualifications.iter().enumerate() {
                parse_date(&q.verified_at)?;
                if !identifier(&q.format_id)
                    || !nonempty(&q.implementation_revision)
                    || !nonempty(&q.evidence)
                    || q.checks.is_empty()
                    || q.checks.iter().any(|c| !nonempty(c))
                    || entry.qualifications[..index].contains(q)
                {
                    return Err(invalid("invalid or duplicate qualification record"));
                }
                match (q.scope, &q.client) {
                    (QualificationScope::FormatContract, None) => {}
                    (
                        QualificationScope::SyntheticClient | QualificationScope::RealClient,
                        Some(client),
                    ) if nonempty(&client.version)
                        && matches!(client.os.as_str(), "linux" | "windows" | "macos") => {}
                    _ => {
                        return Err(invalid(
                            "qualification must name its exact scope/client version/OS",
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    /// Read-only repository validation. URLs are checked syntactically, never
    /// fetched; an unavailable source cannot silently advance a review date.
    /// Evidence existence is necessary, not proof that its claims are true:
    /// maintainers must review the recorded outcomes before changing support.
    pub fn validate_repository(
        &self,
        repo: &Path,
        inventory: &Path,
        today: NaiveDate,
        mode: ValidationMode,
    ) -> io::Result<ValidationReport> {
        self.validate_structure()?;
        let repo = repo.canonicalize()?;
        let inventory = inventory.canonicalize()?;
        if !inventory.starts_with(&repo) {
            return Err(invalid("inventory must be inside the repository"));
        }
        let base = inventory
            .parent()
            .ok_or_else(|| invalid("inventory has no parent"))?;
        let compiled = compiled_connectors();
        let mut report = ValidationReport::default();
        for entry in &self.connectors {
            let code = compiled.iter().find(|c| c.profile_id == entry.id);
            let descriptor = code.map(CompiledConnector::descriptor);
            let observed = ObservedVersion {
                format_id: descriptor.map(|d| d.format_id),
                ..Default::default()
            };
            let technical = self.technical_support(&entry.id, &observed, today);
            let matches_implementation = match (&entry.implementation, descriptor) {
                (None, None) => true,
                (Some(i), Some(d)) => {
                    i.id == d.id
                        && i.revision == d.revision
                        && entry.platform.eq_ignore_ascii_case(d.platform)
                }
                _ => false,
            };
            if !matches_implementation {
                report.finding(
                    &entry.id,
                    Severity::Error,
                    "implementation_mismatch",
                    "Inventory implementation/platform/revision does not match compiled code",
                );
            }
            if entry
                .enabled_operations
                .iter()
                .any(|op| !technical.implemented_operations.contains(op))
            {
                report.finding(
                    &entry.id,
                    Severity::Error,
                    "operation_not_implemented",
                    "Claimed operation has no compiled implementation for this profile",
                );
            }
            if technical
                .implemented_operations
                .iter()
                .any(|op| !entry.enabled_operations.contains(op))
            {
                report.finding(
                    &entry.id,
                    Severity::Error,
                    "implemented_operation_undeclared",
                    "Compiled operations cannot be hidden by removing inventory release claims",
                );
            }
            if matches!(
                entry.support_status,
                SupportStatus::Excluded | SupportStatus::Deferred
            ) && !entry.enabled_operations.is_empty()
            {
                report.finding(
                    &entry.id,
                    Severity::Error,
                    "status_operation_conflict",
                    "Excluded/deferred profile claims enabled operations",
                );
            }
            for q in &entry.qualifications {
                if !matches_implementation
                    || !descriptor.is_some_and(|d| {
                        q.implementation_revision == d.revision && q.format_id == d.format_id
                    })
                {
                    report.finding(&entry.id, Severity::Error, "qualification_mismatch", "Qualification does not bind the compiled implementation revision and format");
                }
                if parse_date(&q.verified_at)? > today {
                    report.finding(
                        &entry.id,
                        Severity::Error,
                        "future_qualification",
                        "Qualification date is in the future",
                    );
                }
                if let Some(client) = &q.client {
                    if !code.is_some_and(|code| {
                        code.versions
                            .clients
                            .contains(&(client.version.as_str(), client.os.as_str()))
                    }) {
                        report.finding(&entry.id, Severity::Error, "client_not_qualified", "Client qualification does not match a code-owned compatible version/OS pair");
                    }
                }
            }
            if entry.support_status == SupportStatus::Supported
                && (entry.enabled_operations.is_empty() || technical.qualifications.is_empty())
            {
                report.finding(&entry.id, Severity::Error, "support_without_qualification", "Supported claim requires enabled implemented operations and matching scoped qualification");
            }
            for path in std::iter::once(&entry.evidence).chain(
                entry
                    .qualifications
                    .iter()
                    .flat_map(|q| std::iter::once(&q.evidence).chain(q.checks.iter())),
            ) {
                if Path::new(path).is_absolute()
                    || base
                        .join(path)
                        .canonicalize()
                        .map_or(true, |p| !p.starts_with(&repo) || !p.is_file())
                {
                    report.finding(
                        &entry.id,
                        Severity::Error,
                        "evidence_missing",
                        format!("Missing or outside-repository evidence: {path}"),
                    );
                }
            }
            if entry
                .last_policy_reviewed_at
                .as_ref()
                .is_some_and(|last| parse_date(last).is_ok_and(|d| d > today))
            {
                report.finding(
                    &entry.id,
                    Severity::Error,
                    "future_review",
                    "Successful review date is in the future",
                );
            }
            if technical.review != ReviewState::Current {
                let severity =
                    if mode == ValidationMode::Release && !entry.enabled_operations.is_empty() {
                        Severity::Error
                    } else {
                        Severity::Warning
                    };
                report.finding(&entry.id, severity, "review_required", "Connector review is due or unknown; review before release. Local import/data are unchanged.");
            }
            report.connectors.push(technical);
        }
        for code in compiled {
            if self.entry(code.profile_id).is_none() {
                report.finding(
                    code.profile_id,
                    Severity::Error,
                    "profile_missing",
                    "Compiled profile is missing from inventory",
                );
            }
        }
        Ok(report)
    }
}
