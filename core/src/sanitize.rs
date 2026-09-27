//! Deterministic local secret rules. No credential validation, network,
//! or filesystem access. Findings carry offsets and categories, never originals.
//! Uncertain candidates require review unless explicitly selected for redaction.

use std::cmp::Reverse;
use std::io;
use std::ops::Range;
use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;
use serde::Serialize;

pub const RULES_VERSION: &str = "secrets/2";
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
    ProviderToken,
    ProviderTokenCandidate,
    TelegramBotToken,
    CookieValue,
    ContextualEntropy,
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
    provider: Regex,
    provider_candidate: Regex,
    telegram_path: Regex,
    telegram_candidate: Regex,
    cookie: Regex,
    entropy_context: Regex,
    assignment: Regex,
    url: Regex,
    jwt: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let regex = |pattern| Regex::new(pattern).expect("built-in secret rule must compile");
        Rules {
            private_key: regex(r"-----BEGIN (?P<label>(?:RSA |DSA |EC |ENCRYPTED |OPENSSH )?PRIVATE KEY|PGP PRIVATE KEY BLOCK)-----"),
            authorization: regex(r#"(?im)(?:^|[\s{"'`])(?:proxy-)?authorization["']?[ \t]*[:=][ \t]*["']?(?:bearer|basic)[ \t]+(?P<value>[A-Za-z0-9._~+/-]+=*)"#),
            // Prefix recognition, not a promise that every match is a valid key.
            // ghs_ is opaque and can contain a long JWT, including dots.
            github: regex(r"\b(?:ghs_[A-Za-z0-9._-]{36,}|(?:gh[pour]_|github_pat_)[A-Za-z0-9._-]{20,})"),
            // Prefixes from primary docs; suffix bounds are local heuristics,
            // not validity checks. Match rotated Slack tokens including xoxe.
            provider: regex(concat!(
                r"\b(?:gl(?:pat|oas|dt|rt|rtr|cbt|ptt|ft|imt|agent|wt|soat|ffct)-|",
                r"xox[bp]-|xapp-|xwfp-|xoxe(?:\.xox[bp])?-|",
                r"sk-ant-(?:api03|api01|admin01|oat01)-)[A-Za-z0-9_-]{20,}"
            )),
            provider_candidate: regex(r"\b(?:(?:sk-|AIza)[A-Za-z0-9_-]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16,})"),
            telegram_path: regex(r"(?i:https://api\.telegram\.org/)(?:file/)?bot(?P<value>[0-9]+:[A-Za-z0-9_-]+)"),
            telegram_candidate: regex(r"\b[0-9]{5,}:[A-Za-z0-9_-]{20,}"),
            cookie: regex(r"(?im)(?:^|[ \t])(?P<kind>set-cookie|cookie)[ \t]*:[ \t]*(?P<pairs>[^\r\n]+)"),
            entropy_context: regex(r"(?i)\b(?:key|token|secret|password)[ \t]+(?:is[ \t]+)?(?P<value>[A-Za-z0-9_+/=-]{20,})"),
            // Quoted .env/DSN values can span lines or contain doubled quotes.
            // An unfinished quoted credential hides the remaining field.
            assignment: regex(r#"(?im)(?:^|[^A-Za-z0-9_])["']?(?P<name>(?:[A-Za-z0-9]+[_-])*(?:password|passwd|pwd|secret|token|api[_-]?key|client[_-]?secret|access[_-]?key|(?:access|refresh|session|id)[_-]?token|session[_-]?id|key))["']?[ \t]*[:=][ \t]*(?:"(?P<double>(?:\\[\s\S]|""|[^"\\])+\\?)"?|'(?P<single>(?:\\[\s\S]|''|[^'\\])+\\?)'?|(?P<bare>[^\s,;&`"'<>}{\[\]]+))"#),
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
    for (pattern, rule, confidence) in [
        (&rules.provider, SecretRule::ProviderToken, Confidence::High),
        (
            &rules.provider_candidate,
            SecretRule::ProviderTokenCandidate,
            Confidence::Medium,
        ),
        (
            &rules.telegram_candidate,
            SecretRule::TelegramBotToken,
            Confidence::Medium,
        ),
    ] {
        for token in pattern.find_iter(text) {
            add(rule, confidence, token.range())?;
        }
    }
    for capture in rules.telegram_path.captures_iter(text) {
        add(
            SecretRule::TelegramBotToken,
            Confidence::High,
            capture.name("value").expect("bot token").range(),
        )?;
    }
    for capture in rules.cookie.captures_iter(text) {
        let pairs = capture.name("pairs").expect("cookie pairs");
        let mut offset = pairs.start();
        for segment in pairs.as_str().split_inclusive(';') {
            let pair = segment.trim_end_matches(';');
            if let Some((name, value)) = pair.split_once('=') {
                let start = offset + name.len() + 1 + value.len() - value.trim_start().len();
                let value = value.trim();
                let (start, value) =
                    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
                        (start + 1, &value[1..value.len() - 1])
                    } else {
                        (start, value)
                    };
                if !value.is_empty() && !placeholder(value) {
                    let confidence = if sensitive_cookie_name(name.trim()) {
                        Some(Confidence::High)
                    } else if entropy_candidate(value) {
                        Some(Confidence::Medium)
                    } else {
                        None
                    };
                    if let Some(confidence) = confidence {
                        add(
                            SecretRule::CookieValue,
                            confidence,
                            start..start + value.len(),
                        )?;
                    }
                }
            }
            // Set-Cookie has ONE pair; following fields are attributes.
            if capture["kind"].eq_ignore_ascii_case("set-cookie") {
                break;
            }
            offset += segment.len();
        }
    }
    for capture in rules.entropy_context.captures_iter(text) {
        let value = capture.name("value").expect("contextual candidate");
        if entropy_candidate(value.as_str()) && !placeholder(value.as_str()) {
            add(
                SecretRule::ContextualEntropy,
                Confidence::Medium,
                value.range(),
            )?;
        }
    }
    for capture in rules.assignment.captures_iter(text) {
        let value = ["double", "single", "bare"]
            .iter()
            .find_map(|name| capture.name(name))
            .expect("one assignment value");
        if placeholder(value.as_str())
            || (capture.name("bare").is_some()
                && matches!(
                    value.as_str().to_ascii_lowercase().as_str(),
                    "null" | "none" | "true" | "false"
                ))
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
}

fn sensitive_cookie_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let name = lower
        .strip_prefix("__secure-")
        .or_else(|| lower.strip_prefix("__host-"))
        .unwrap_or(&lower);
    matches!(
        name,
        "session"
            | "sid"
            | "sessionid"
            | "session_id"
            | "jsessionid"
            | "phpsessid"
            | "connect.sid"
            | "_gitlab_session"
            | "auth"
            | "auth_token"
            | "access_token"
            | "csrf"
            | "csrftoken"
            | "xsrf-token"
    )
}

/// A review heuristic, never credential validation. Only callers with an
/// explicit local label/header use it; arbitrary hashes are not scanned.
fn entropy_candidate(value: &str) -> bool {
    if value.len() < 20 {
        return false;
    }
    let mut counts = [0_usize; 128];
    for byte in value.bytes() {
        if !byte.is_ascii_alphanumeric() && !b"_+/=-".contains(&byte) {
            return false;
        }
        counts[usize::from(byte)] += 1;
    }
    let entropy = counts
        .iter()
        .filter(|&&n| n > 0)
        .map(|&n| {
            let p = n as f64 / value.len() as f64;
            -p * p.log2()
        })
        .sum::<f64>();
    entropy >= 3.5
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
