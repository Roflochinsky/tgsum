//! Bounded local participant replacement. Native identity and display aliases
//! are separate; ambiguous prose never picks a person by encounter order.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::ops::Range;
use std::sync::OnceLock;

use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::pseudonyms::{MappingDraft, PseudonymCategory, PseudonymInput, PseudonymMapping};
use crate::sanitize::{sanitize, ReviewPolicy, MAX_FIELD_BYTES, MAX_FINDINGS, REPLACEMENT};
use crate::snapshot::CanonicalMessage;

pub const RULES_VERSION: &str = "pii/1";
const PERSON_NAMESPACE: &str = "pii/1/person";
const UNKNOWN: &str = "[REDACTED_PERSON]";
const MAX_ALIASES: usize = 10_000;
const MAX_ALIAS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiCategory {
    Participants,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PiiPolicy {
    pub categories: BTreeSet<PiiCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiSummary {
    pub rules_version: String,
    pub categories: BTreeSet<PiiCategory>,
    pub replacements: usize,
    pub by_category: BTreeMap<PiiCategory, usize>,
    pub ambiguous: usize,
    pub unresolved: usize,
}

impl PiiSummary {
    pub(crate) fn new(policy: &PiiPolicy) -> Self {
        Self {
            rules_version: RULES_VERSION.into(),
            categories: policy.categories.clone(),
            replacements: 0,
            by_category: BTreeMap::new(),
            ambiguous: 0,
            unresolved: 0,
        }
    }

    pub(crate) fn record(&mut self, findings: &[PiiFinding]) {
        for finding in findings {
            self.replacements += 1;
            *self.by_category.entry(finding.category).or_default() += 1;
            self.ambiguous += usize::from(finding.ambiguous);
            self.unresolved += usize::from(finding.unresolved);
        }
    }
}

pub(crate) struct PiiFinding {
    pub category: PiiCategory,
    pub input: Range<usize>,
    pub output: Range<usize>,
    ambiguous: bool,
    unresolved: bool,
}

pub(crate) struct PiiText {
    pub text: String,
    pub findings: Vec<PiiFinding>,
}

#[derive(Clone, PartialEq, Eq)]
enum Owner {
    Known(String), // Existing private Project label, never a guessed identity.
    Unresolved,
    Ambiguous,
}

pub(crate) struct PiiDetector {
    aliases: BTreeMap<String, Owner>,
    alias_bytes: usize,
    matcher: Option<AhoCorasick>,
    owners: Vec<Owner>,
}

impl PiiDetector {
    pub(crate) fn new() -> Self {
        Self {
            aliases: BTreeMap::new(),
            alias_bytes: 0,
            matcher: None,
            owners: Vec::new(),
        }
    }

    pub(crate) fn observe(
        &mut self,
        message: &CanonicalMessage,
        mapping: &mut MappingDraft,
        secrets: ReviewPolicy,
    ) -> io::Result<()> {
        let name = message
            .sender_name
            .as_deref()
            .map(|s| sanitize(s, secrets).map(|r| r.text))
            .transpose()?;
        if let Some(identity) = participant_identity(message)? {
            // Missing names still get a stable header label; the raw native ID
            // is private provenance and is explicitly excluded from prose aliases.
            let original = name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(message.sender_id.as_deref().expect("identity has ID"));
            mapping.allocate(&[PseudonymInput {
                category: PseudonymCategory::Person,
                identity: &identity,
                original,
            }])?;
        } else if let Some(name) = name {
            self.alias(&name, Owner::Unresolved)?;
        }
        Ok(())
    }

    pub(crate) fn finish_discovery(&mut self, mapping: &PseudonymMapping) -> io::Result<()> {
        // Project-wide historical aliases are conservative: a collision in any
        // retained namespace cannot silently become a unique person elsewhere.
        for (identity, label, aliases) in mapping.category_entries(PseudonymCategory::Person) {
            let Ok(parts) = serde_json::from_str::<[String; 4]>(identity) else {
                continue;
            };
            if parts[0] != PERSON_NAMESPACE {
                continue;
            }
            for alias in aliases {
                if alias != &parts[3] {
                    self.alias(alias, Owner::Known(label.into()))?;
                }
            }
        }
        if self.aliases.is_empty() {
            return Ok(());
        }
        let matcher = AhoCorasick::builder()
            .kind(Some(AhoCorasickKind::ContiguousNFA))
            .match_kind(MatchKind::Standard)
            .build(self.aliases.keys())
            .map_err(|_| invalid("participant matcher exceeds resource limits"))?;
        if matcher.memory_usage() > 64 * 1024 * 1024 {
            return Err(invalid("participant matcher exceeds resource limits"));
        }
        self.owners = self.aliases.values().cloned().collect();
        self.matcher = Some(matcher);
        Ok(())
    }

    fn alias(&mut self, name: &str, owner: Owner) -> io::Result<()> {
        if name.is_empty() || name.chars().any(char::is_control) || name.contains(REPLACEMENT) {
            return Ok(());
        }
        if let Some(existing) = self.aliases.get_mut(name) {
            if existing != &owner {
                *existing = Owner::Ambiguous;
            }
            return Ok(());
        }
        if self.aliases.len() == MAX_ALIASES
            || self.alias_bytes.saturating_add(name.len()) > MAX_ALIAS_BYTES
        {
            return Err(invalid("participant alias dictionary exceeds limits"));
        }
        self.alias_bytes += name.len();
        self.aliases.insert(name.into(), owner);
        Ok(())
    }

    pub(crate) fn replace(
        &self,
        value: &str,
        sender: Option<&CanonicalMessage>,
        mapping: &PseudonymMapping,
    ) -> io::Result<PiiText> {
        if value.len() > MAX_FIELD_BYTES {
            return Err(invalid("PII field exceeds limit"));
        }
        if let Some(message) = sender {
            let owner = match participant_identity(message)? {
                Some(identity) => Owner::Known(
                    mapping
                        .lookup(PseudonymCategory::Person, &identity)
                        .ok_or_else(|| invalid("participant mapping is incomplete"))?
                        .into(),
                ),
                None if message.sender_name.is_some() => Owner::Unresolved,
                None => {
                    return Ok(PiiText {
                        text: value.into(),
                        findings: Vec::new(),
                    })
                }
            };
            return apply(value, [(0..value.len(), owner)]);
        }
        let Some(matcher) = &self.matcher else {
            return Ok(PiiText {
                text: value.into(),
                findings: Vec::new(),
            });
        };
        let protected = protected_spans(value)?;
        let mut replacements = Vec::new();
        for (attempt, found) in matcher.find_overlapping_iter(value).enumerate() {
            if attempt == MAX_FINDINGS {
                return Err(invalid("PII match attempts exceed limit"));
            }
            let range = found.start()..found.end();
            if !boundary(value, &range)
                || protected
                    .range(..range.end)
                    .next_back()
                    .is_some_and(|(_, end)| *end > range.start)
            {
                continue;
            }
            replacements.push((range, self.owners[found.pattern().as_usize()].clone()));
        }
        // Filter boundaries/containers before selecting longest valid matches.
        // Otherwise an invalid longer alias could hide a valid shorter one.
        replacements.sort_unstable_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
        let mut end = 0;
        apply(
            value,
            replacements.into_iter().filter(|(range, _)| {
                if range.start < end {
                    return false;
                }
                end = range.end;
                true
            }),
        )
    }
}

fn participant_identity(message: &CanonicalMessage) -> io::Result<Option<String>> {
    let Some(id) = message
        .sender_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    else {
        return Ok(None);
    };
    let source = &message.key.source;
    let identity = serde_json::to_string(&(
        PERSON_NAMESPACE,
        &source.platform,
        &source.account_local_id,
        id,
    ))
    .map_err(|_| invalid("invalid participant identity"))?;
    if identity.len() > 4096 {
        return Err(invalid("participant identity exceeds limit"));
    }
    Ok(Some(identity))
}

fn apply(
    value: &str,
    replacements: impl IntoIterator<Item = (Range<usize>, Owner)>,
) -> io::Result<PiiText> {
    let mut text = String::new();
    let mut findings = Vec::new();
    let mut cursor = 0;
    for (input, owner) in replacements {
        text.push_str(&value[cursor..input.start]);
        let start = text.len();
        text.push_str(match &owner {
            Owner::Known(label) => label,
            _ => UNKNOWN,
        });
        cursor = input.end;
        findings.push(PiiFinding {
            category: PiiCategory::Participants,
            input,
            output: start..text.len(),
            ambiguous: owner == Owner::Ambiguous,
            unresolved: owner == Owner::Unresolved,
        });
    }
    text.push_str(&value[cursor..]);
    if text.len() > MAX_FIELD_BYTES {
        return Err(invalid("PII output exceeds field limit"));
    }
    Ok(PiiText { text, findings })
}

fn boundary(value: &str, range: &Range<usize>) -> bool {
    !value[..range.start].chars().next_back().is_some_and(word)
        && !value[range.end..].chars().next().is_some_and(word)
}

/// Other category containers are indivisible for participant alias matching.
/// These are lexical guards, not assertions that the URL/contact/path is valid.
fn protected_spans(value: &str) -> io::Result<BTreeMap<usize, usize>> {
    static RULES: OnceLock<Vec<Regex>> = OnceLock::new();
    let rules =
        RULES.get_or_init(|| {
            [
        r#"(?i)\b(?:[a-z][a-z0-9+.-]{0,31}://|mailto:|tel:)[^\s<>"'`]+"#,
        r#"(?:"[^"\r\n]+"|[\p{L}\p{M}\p{N}._%+!#$&*/=?^`{|}~-]+)@[\p{L}\p{M}\p{N}_.:\[\]-]+"#,
        r"@[\p{L}\p{M}\p{N}_]+",
        r#"(?:~?/|[A-Za-z]:[\\/]|\\\\)[^\s<>"'`]+"#,
        r#""(?:~?/|[A-Za-z]:[\\/]|\\\\)[^"\r\n]+"|'(?:~?/|[A-Za-z]:[\\/]|\\\\)[^'\r\n]+'"#,
    ].into_iter().map(|r| Regex::new(r).expect("PII container guard")).collect()
        });
    let mut ranges = Vec::new();
    for rule in rules {
        for found in rule.find_iter(value) {
            if found.len() > 16 * 1024 || ranges.len() == MAX_FINDINGS {
                return Err(invalid("PII protected spans exceed limits"));
            }
            ranges.push(found.range());
        }
    }
    ranges.sort_unstable_by_key(|r| r.start);
    let mut merged = BTreeMap::<usize, usize>::new();
    for range in ranges {
        match merged.last_entry() {
            Some(mut entry) if *entry.get() >= range.start => {
                let end = (*entry.get()).max(range.end);
                entry.insert(end);
            }
            _ => {
                merged.insert(range.start, range.end);
            }
        }
    }
    Ok(merged)
}

fn word(c: char) -> bool {
    static WORD: OnceLock<Regex> = OnceLock::new();
    let mut bytes = [0; 4];
    WORD.get_or_init(|| Regex::new(r"\A\w\z").expect("Unicode word class"))
        .is_match(c.encode_utf8(&mut bytes))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
