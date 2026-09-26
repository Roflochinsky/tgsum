//! Deterministic, local minimum-secret rules. No credential validation, network,
//! or filesystem access. Findings carry offsets and categories, never originals.
//! Uncertain candidates require review unless explicitly selected for redaction.

use std::cmp::Reverse;
use std::io;
use std::ops::Range;
use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;
use serde::Serialize;

pub const RULES_VERSION: &str = "minimum-secrets/1";
pub const REPLACEMENT: &str = "[REDACTED_SECRET]";
/// Fail explicitly on an oversized field; never publish a truncated scan.
pub const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FINDINGS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretRule {
    PrivateKey,
    IncompletePrivateKey,
    Authorization,
    GithubToken,
    CredentialAssignment,
    UrlCredentials,
    JwtCandidate,
    GenericKeyAssignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingAction {
    Redacted,
    NeedsReview,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReviewPolicy {
    #[default]
    KeepForReview,
    RedactCandidates,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: SecretRule,
    pub confidence: Confidence,
    pub action: FindingAction,
    /// UTF-8 byte offsets in the original field, not JavaScript UTF-16 indices.
    pub input: Range<usize>,
    /// UTF-8 byte offsets in the returned text (replacement or review candidate).
    pub output: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SanitizationReport {
    pub rules_version: &'static str,
    pub redacted: usize,
    pub needs_review: usize,
    pub findings: Vec<Finding>,
}

/// Text may still contain review candidates or secrets outside these rules.
/// Deliberately lacks Debug/Serialize: diagnostics should use `report` only.
pub struct SanitizedText {
    pub text: String,
    pub report: SanitizationReport,
}

struct Detected {
    rule: SecretRule,
    confidence: Confidence,
    range: Range<usize>,
}

struct Rules {
    private_key: Regex,
    authorization: Regex,
    github: Regex,
    assignment: Regex,
    url: Regex,
    jwt: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let regex = |pattern| Regex::new(pattern).expect("built-in secret rule must compile");
        Rules {
            private_key: regex(r"-----BEGIN (?P<label>(?:RSA |DSA |EC |ENCRYPTED |OPENSSH )?PRIVATE KEY)-----"),
            authorization: regex(r#"(?im)(?:^|[\s{"'`])(?:proxy-)?authorization["']?[ \t]*[:=][ \t]*["']?(?:bearer|basic)[ \t]+(?P<value>[A-Za-z0-9._~+/-]+=*)"#),
            // Prefix recognition, not a promise that every match is a valid key.
            // ghs_ is opaque and can contain a long JWT, including dots.
            github: regex(r"\b(?:ghs_[A-Za-z0-9._-]{36,}|(?:gh[pour]_|github_pat_)[A-Za-z0-9._-]{20,})"),
            assignment: regex(r#"(?im)(?:^|[^A-Za-z0-9_])["']?(?P<name>(?:[A-Za-z0-9]+[_-])*(?:password|passwd|pwd|secret|token|api[_-]?key|client[_-]?secret|access[_-]?key|session[_-]?id|key))["']?[ \t]*[:=][ \t]*(?:"(?P<double>(?:\\[^\r\n]|[^"\\\r\n])+\\?)"?|'(?P<single>(?:\\[^\r\n]|[^'\\\r\n])+\\?)'?|(?P<bare>[^\s,;&`"'<>}{\[\]]+))"#),
            url: regex(r#"[A-Za-z][A-Za-z0-9+.-]*://(?P<authority>[^\s/?#<>"'`\\]+)"#),
            jwt: regex(r"[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]*){2,}"),
        }
    })
}

/// Redact known contexts and return uncertain findings for a local preview.
/// A caller must handle `needs_review` before publishing an analysis bundle.
pub fn sanitize(text: &str, policy: ReviewPolicy) -> io::Result<SanitizedText> {
    if text.len() > MAX_FIELD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "text field exceeds the 16 MiB sanitization limit",
        ));
    }
    let rules = rules();
    let mut detected = Vec::new();
    let mut add = |rule, confidence, range| {
        if detected.len() == MAX_FINDINGS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "text field exceeds the sanitization findings limit",
            ));
        }
        detected.push(Detected {
            rule,
            confidence,
            range,
        });
        Ok(())
    };

    let mut key_end = 0;
    for capture in rules.private_key.captures_iter(text) {
        let start = capture.get(0).expect("whole match");
        if start.start() < key_end {
            continue;
        }
        let end_marker = format!("-----END {}-----", &capture["label"]);
        let closing = text[start.end()..].find(&end_marker);
        let end = closing.map_or(text.len(), |offset| start.end() + offset + end_marker.len());
        key_end = end;
        // An unfinished key hides the remainder of this field, not just BEGIN.
        add(
            if closing.is_some() {
                SecretRule::PrivateKey
            } else {
                SecretRule::IncompletePrivateKey
            },
            Confidence::High,
            start.start()..end,
        )?;
    }
    for capture in rules.authorization.captures_iter(text) {
        let value = capture.name("value").expect("credential capture");
        if !placeholder(value.as_str()) {
            add(SecretRule::Authorization, Confidence::High, value.range())?;
        }
    }
    for token in rules.github.find_iter(text) {
        add(SecretRule::GithubToken, Confidence::High, token.range())?;
    }
    for capture in rules.assignment.captures_iter(text) {
        let value = ["double", "single", "bare"]
            .iter()
            .find_map(|name| capture.name(name))
            .expect("one assignment value");
        if placeholder(value.as_str())
            || (capture.name("bare").is_some()
                && value.as_str() == "$"
                && text[value.end()..].starts_with('{'))
        {
            continue;
        }
        // The generic word 'key' is also common in ordinary data structures.
        let name = capture["name"].to_ascii_lowercase().replace('-', "_");
        let generic = (name == "key" || name.ends_with("_key"))
            && !name.ends_with("api_key")
            && !name.ends_with("access_key")
            && !name.ends_with("secret_key")
            && !name.ends_with("private_key");
        add(
            if generic {
                SecretRule::GenericKeyAssignment
            } else {
                SecretRule::CredentialAssignment
            },
            if generic {
                Confidence::Medium
            } else {
                Confidence::High
            },
            value.range(),
        )?;
    }
    for capture in rules.url.captures_iter(text) {
        let authority = capture.name("authority").expect("URL authority");
        let Some((userinfo, host)) = authority.as_str().rsplit_once('@') else {
            continue;
        };
        if !host.is_empty()
            && userinfo
                .split_once(':')
                .is_some_and(|(_, password)| !password.is_empty())
        {
            add(
                SecretRule::UrlCredentials,
                Confidence::High,
                authority.start()..authority.start() + userinfo.len(),
            )?;
        }
    }
    for candidate in rules.jwt.find_iter(text) {
        let mut token = candidate.as_str();
        // A sentence-ending dot after a three/five-part token is punctuation.
        if matches!(token.split('.').count(), 4 | 6) && token.ends_with('.') {
            token = &token[..token.len() - 1];
        }
        if is_jwt_candidate(token) {
            add(
                SecretRule::JwtCandidate,
                Confidence::Medium,
                candidate.start()..candidate.start() + token.len(),
            )?;
        }
    }

    // One replacement per overlapping region. A weaker nested heuristic must
    // not reveal a known credential or cause offsets to refer to changed text.
    detected.sort_by_key(|d| (d.range.start, Reverse(d.range.end)));
    let mut merged: Vec<Detected> = Vec::new();
    for detection in detected {
        if let Some(last) = merged
            .last_mut()
            .filter(|last| detection.range.start < last.range.end)
        {
            last.range.end = last.range.end.max(detection.range.end);
            if last.confidence == Confidence::Medium && detection.confidence == Confidence::High {
                last.confidence = Confidence::High;
                last.rule = detection.rule;
            }
        } else {
            merged.push(detection);
        }
    }
    let mut output = String::with_capacity(text.len());
    let mut report = SanitizationReport {
        rules_version: RULES_VERSION,
        redacted: 0,
        needs_review: 0,
        findings: Vec::new(),
    };
    let mut cursor = 0;
    for detection in merged {
        output.push_str(&text[cursor..detection.range.start]);
        let start = output.len();
        let action = if detection.confidence == Confidence::High
            || policy == ReviewPolicy::RedactCandidates
        {
            output.push_str(REPLACEMENT);
            report.redacted += 1;
            FindingAction::Redacted
        } else {
            output.push_str(&text[detection.range.clone()]);
            report.needs_review += 1;
            FindingAction::NeedsReview
        };
        cursor = detection.range.end;
        report.findings.push(Finding {
            rule: detection.rule,
            confidence: detection.confidence,
            action,
            input: detection.range,
            output: start..output.len(),
        });
    }
    output.push_str(&text[cursor..]);
    Ok(SanitizedText {
        text: output,
        report,
    })
}

fn placeholder(value: &str) -> bool {
    value == REPLACEMENT
        || (value.starts_with("${") && value.ends_with('}'))
        || value.starts_with("{{")
        || (value.starts_with('<') && value.ends_with('>'))
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "null" | "none" | "true" | "false"
        )
}

fn is_jwt_candidate(token: &str) -> bool {
    let mut parts = token.split('.');
    let header = parts.next().unwrap_or_default();
    let count = 1 + parts.count();
    if !matches!(count, 3 | 5) || header.len() > 16_384 {
        return false;
    }
    let Ok(header) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(header) else {
        return false;
    };
    let Ok(header) = serde_json::from_slice::<serde_json::Value>(&header) else {
        return false;
    };
    header.get("alg").is_some_and(|alg| alg.is_string())
        && (count == 3 || header.get("enc").is_some_and(|enc| enc.is_string()))
}
