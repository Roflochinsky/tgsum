//! Explicit user terms. Configuration is private; diagnostic formatting omits
//! values. This module never learns aliases or resolves identities externally.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::OnceLock;
use std::{fmt, io};

use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::pseudonyms::{MappingDraft, PseudonymCategory, PseudonymInput};
use crate::sanitize::{MAX_FIELD_BYTES, MAX_FINDINGS};

pub const RULES_VERSION: &str = "terms/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTermsSummary {
    pub rules_version: String,
    pub replacements: usize,
}

pub(crate) struct TermFinding {
    pub input: Range<usize>,
    pub output: Range<usize>,
}
pub(crate) struct TermText {
    pub text: String,
    pub findings: Vec<TermFinding>,
}

pub(crate) struct TermDetector {
    terms: CustomTerms,
    identities: Vec<String>,
    matcher: AhoCorasick,
}

impl TermDetector {
    pub(crate) fn new(terms: CustomTerms) -> io::Result<Self> {
        terms.validate()?;
        let matcher = AhoCorasick::builder()
            .kind(Some(AhoCorasickKind::ContiguousNFA))
            .match_kind(MatchKind::Standard)
            .build(terms.entries.iter().map(|t| t.value.as_str()))
            .map_err(|_| invalid("cannot build sensitive-term matcher"))?;
        if matcher.memory_usage() > 64 * 1024 * 1024 {
            return Err(invalid("sensitive-term matcher exceeds limit"));
        }
        let identities = terms
            .entries
            .iter()
            .map(|t| format!("terms/1:{:x}", Sha256::digest(t.value.as_bytes())))
            .collect();
        Ok(Self {
            terms,
            identities,
            matcher,
        })
    }

    pub(crate) fn replace(
        &self,
        value: &str,
        generated: &[Range<usize>],
        mapping: &mut MappingDraft,
    ) -> io::Result<TermText> {
        if value.len() > MAX_FIELD_BYTES {
            return Err(invalid("sensitive-term field exceeds limit"));
        }
        let mut matches = Vec::new();
        for (attempt, found) in self.matcher.find_overlapping_iter(value).enumerate() {
            if attempt == MAX_FINDINGS {
                return Err(invalid("sensitive-term match attempts exceed limit"));
            }
            let range = found.start()..found.end();
            let index = found.pattern().as_usize();
            let protected = generated.partition_point(|r| r.end <= range.start);
            if generated
                .get(protected)
                .is_some_and(|r| r.start < range.end)
            {
                continue;
            }
            if self.terms.entries[index].boundary == TermBoundary::Word
                && (value[..range.start].chars().next_back().is_some_and(word)
                    || value[range.end..].chars().next().is_some_and(word))
            {
                continue;
            }
            matches.push((range, index));
        }
        matches.sort_unstable_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
        let mut end = 0;
        matches.retain(|(r, _)| {
            if r.start < end {
                false
            } else {
                end = r.end;
                true
            }
        });
        let inputs: Vec<_> = matches
            .iter()
            .map(|(r, index)| PseudonymInput {
                category: PseudonymCategory::CustomTerm,
                identity: &self.identities[*index],
                original: &value[r.clone()],
            })
            .collect();
        let labels = mapping.allocate(&inputs)?;
        let mut text = String::new();
        let mut findings = Vec::new();
        let mut cursor = 0;
        for ((input, _), label) in matches.into_iter().zip(labels) {
            text.push_str(&value[cursor..input.start]);
            let start = text.len();
            text.push_str(&label);
            cursor = input.end;
            findings.push(TermFinding {
                input,
                output: start..text.len(),
            });
        }
        text.push_str(&value[cursor..]);
        if text.len() > MAX_FIELD_BYTES {
            return Err(invalid("sensitive-term output exceeds field limit"));
        }
        Ok(TermText { text, findings })
    }
}

fn word(c: char) -> bool {
    static WORD: OnceLock<Regex> = OnceLock::new();
    let mut bytes = [0; 4];
    WORD.get_or_init(|| Regex::new(r"\A\w\z").expect("Unicode word character"))
        .is_match(c.encode_utf8(&mut bytes))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TermBoundary {
    #[default]
    Word,
    Substring,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomTerm {
    pub value: String,
    #[serde(default)]
    pub boundary: TermBoundary,
}

impl fmt::Debug for CustomTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomTerm")
            .field("boundary", &self.boundary)
            .finish_non_exhaustive()
    }
}

/// Serialized only in private Project configuration / local editing IPC.
#[derive(Clone, Default, PartialEq, Eq, Serialize)]
pub struct CustomTerms {
    pub entries: Vec<CustomTerm>,
}

impl fmt::Debug for CustomTerms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomTerms")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl<'de> Deserialize<'de> for CustomTerms {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            entries: Vec<CustomTerm>,
        }
        // Serde's unknown enum/field errors can quote supplied strings. They
        // must not escape through ProjectStore diagnostics or command IPC.
        let wire = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid sensitive-term dictionary"))?;
        let terms = Self {
            entries: wire.entries,
        };
        terms.validate().map_err(serde::de::Error::custom)?;
        Ok(terms)
    }
}

impl CustomTerms {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.entries.len() > 1024 {
            return Err(invalid("sensitive-term count exceeds limit"));
        }
        let mut bytes = 0;
        let mut values = BTreeSet::new();
        for term in &self.entries {
            if term.value.is_empty()
                || term.value.len() > 4096
                || term.value.trim() != term.value
                || term.value.chars().any(char::is_control)
            {
                return Err(invalid("sensitive term must be nonempty, bounded and without edge whitespace or controls"));
            }
            if !values.insert(&term.value) {
                return Err(invalid("duplicate sensitive term"));
            }
            bytes += term.value.len();
            if bytes > 64 * 1024 {
                return Err(invalid("sensitive-term dictionary exceeds byte limit"));
            }
        }
        Ok(())
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
