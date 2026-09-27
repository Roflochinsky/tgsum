//! Repository-only offline governance command. This binary is not a background
//! updater and is not invoked by local import/analysis.
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, NaiveDate};
use tgsum_core::registry::{Registry, ValidationMode};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("connector-registry: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> io::Result<bool> {
    let mut args = std::env::args().skip(1);
    let mut root = PathBuf::from(".");
    let mut inventory = None;
    let mut today = None;
    let mut mode = ValidationMode::Development;
    while let Some(arg) = args.next() {
        let value = |args: &mut std::iter::Skip<std::env::Args>| {
            args.next().ok_or_else(|| invalid("missing option value"))
        };
        match arg.as_str() {
            "--repo" => root = PathBuf::from(value(&mut args)?),
            "--inventory" => inventory = Some(PathBuf::from(value(&mut args)?)),
            "--today" => {
                let value = value(&mut args)?;
                let date = NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(invalid)?;
                if date.to_string() != value {
                    return Err(invalid("date must use YYYY-MM-DD"));
                }
                today = Some(date);
            }
            "--release" => mode = ValidationMode::Release,
            "--help" | "-h" => {
                println!("Usage: connector-registry [--repo DIR] [--inventory FILE] [--today YYYY-MM-DD] [--release]\n\nRead-only local validation, JSON report on stdout. FILE is relative to DIR.\nExit 0: valid (may have research reminders); 1: validation findings; 2: invalid input.\nRelease mode requires current review of enabled shipping profiles.\nNo network requests, data deletion or local-import runtime gating.");
                return Ok(true);
            }
            _ => return Err(invalid(format!("unknown argument: {arg}"))),
        }
    }
    let today = match today {
        Some(date) => date,
        None => {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(invalid)?
                .as_secs();
            DateTime::from_timestamp(i64::try_from(seconds).map_err(invalid)?, 0)
                .ok_or_else(|| invalid("system date out of range"))?
                .date_naive()
        }
    };
    let inventory =
        root.join(inventory.unwrap_or_else(|| PathBuf::from("docs/connectors/registry.json")));
    // Bound the read before serde; symlink/containment of referenced evidence is
    // checked by repository validation. No response body is fetched from URLs.
    let mut bytes = Vec::new();
    std::fs::File::open(&inventory)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let registry = Registry::parse(&bytes)?;
    let report = registry.validate_repository(&root, &inventory, today, mode)?;
    serde_json::to_writer_pretty(io::stdout().lock(), &report).map_err(invalid)?;
    println!();
    Ok(report.passed())
}

fn invalid(value: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, value.to_string())
}
