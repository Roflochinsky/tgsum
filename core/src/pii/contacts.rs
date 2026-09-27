//! Contact shapes, never network identity resolution. Each category owns its
//! complete span and private canonical identity; partial malformed tails fail.

use std::io;
use std::sync::OnceLock;

use regex::Regex;
use url::Host;

use super::{
    invalid, opaque_spans, phone_protected_spans, username_protected_spans, word, Owner,
    PiiCategory, Replacement, MAX_FINDINGS,
};
use crate::pseudonyms::{MappingDraft, PseudonymCategory, PseudonymInput};
use crate::snapshot::SourceScope;

pub(super) fn email_matcher() -> &'static Regex {
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    EMAIL.get_or_init(|| {
        Regex::new(r#"(?P<local>"[^"\r\n]+"|[^\s<>()\[\]:;,@"\\]+)@(?P<domain>[^\s<>"'`,;(){}]+)"#)
            .expect("email candidate")
    })
}

pub(super) fn emails(value: &str, mapping: &mut MappingDraft) -> io::Result<Vec<Replacement>> {
    let opaque = opaque_spans(value)?;
    let mut contacts = Vec::new();
    for (attempt, found) in email_matcher().captures_iter(value).enumerate() {
        if attempt == MAX_FINDINGS {
            return Err(invalid("email candidates exceed limit"));
        }
        let whole = found.get(0).expect("email candidate");
        if whole.len() > 16 * 1024 {
            return Err(invalid("email candidate exceeds limit"));
        }
        if opaque
            .range(..whole.end())
            .next_back()
            .is_some_and(|(_, end)| *end > whole.start())
        {
            continue;
        }
        let local = found.name("local").expect("email local part");
        let domain = found.name("domain").expect("email domain");
        if value[..whole.start()]
            .chars()
            .next_back()
            .is_some_and(|c| word(c) || matches!(c, '@' | '\\'))
        {
            continue;
        }
        let mut local_text = local.as_str();
        let mut start = whole.start();
        let wrapper = local_text.chars().next().and_then(|c| match c {
            '\'' | '`' => Some((c, c)),
            '“' => Some(('“', '”')),
            '‘' => Some(('‘', '’')),
            '«' => Some(('«', '»')),
            _ => None,
        });
        if let Some((left, _)) = wrapper.filter(|(_, right)| {
            domain.as_str().ends_with(*right) || value[domain.end()..].starts_with(*right)
        }) {
            start += left.len_utf8();
            local_text = &local_text[left.len_utf8()..];
        }
        let raw_domain = domain
            .as_str()
            .trim_end_matches(['!', '?', ':', '»', '”', '’']);
        let raw_domain = raw_domain.strip_suffix('.').unwrap_or(raw_domain);
        let Some(canonical_domain) = email_domain(raw_domain) else {
            continue;
        };
        if !local_part(local_text) {
            continue;
        }
        let identity = serde_json::to_string(&("pii/1/email", local_text, canonical_domain))
            .map_err(|_| invalid("invalid email identity"))?;
        contacts.push((start..domain.start() + raw_domain.len(), identity));
    }
    let inputs: Vec<_> = contacts
        .iter()
        .map(|(range, identity)| PseudonymInput {
            category: PseudonymCategory::Email,
            identity,
            original: &value[range.clone()],
        })
        .collect();
    let labels = mapping.allocate(&inputs)?;
    Ok(contacts
        .into_iter()
        .zip(labels)
        .map(|((range, _), label)| (range, PiiCategory::Emails, Owner::Known(label)))
        .collect())
}

pub(super) fn username_matcher() -> &'static Regex {
    static HANDLE: OnceLock<Regex> = OnceLock::new();
    HANDLE.get_or_init(|| Regex::new(r#"@[^\s<>"'`/\\:;,!?()\[\]{}]+"#).expect("handle candidate"))
}

pub(super) fn usernames(
    value: &str,
    scope: Option<&SourceScope>,
    mapping: &mut MappingDraft,
) -> io::Result<Vec<Replacement>> {
    let protected = username_protected_spans(value)?;
    let mut contacts = Vec::new();
    for (attempt, found) in username_matcher().find_iter(value).enumerate() {
        if attempt == MAX_FINDINGS || found.len() > 16 * 1024 {
            return Err(invalid("username candidates exceed limits"));
        }
        let raw = found.as_str().trim_end_matches(['»', '”', '’']);
        let raw = raw.strip_suffix('.').unwrap_or(raw);
        let range = found.start()..found.start() + raw.len();
        let handle = &raw[1..];
        if !(1..=32).contains(&handle.len())
            || !handle
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || protected
                .range(..range.end)
                .next_back()
                .is_some_and(|(_, end)| *end > range.start)
            || value[..range.start]
                .chars()
                .next_back()
                .is_some_and(|c| word(c) || matches!(c, '@' | '/' | '\\'))
            || value[range.end..]
                .chars()
                .next()
                .is_some_and(|c| word(c) || matches!(c, '@' | '/' | '\\'))
        {
            continue;
        }
        // A handle is not a person ID. Only Telegram case equivalence is known;
        // other namespaces retain exact ASCII spelling, without existence claims.
        let identity = scope
            .map(|scope| {
                let handle = if scope.platform == "telegram" {
                    handle.to_ascii_lowercase()
                } else {
                    handle.into()
                };
                serde_json::to_string(&(
                    "pii/1/username",
                    &scope.platform,
                    &scope.account_local_id,
                    handle,
                ))
                .map_err(|_| invalid("invalid username identity"))
            })
            .transpose()?;
        contacts.push((range, identity));
    }
    let inputs: Vec<_> = contacts
        .iter()
        .filter_map(|(range, identity)| {
            Some(PseudonymInput {
                category: PseudonymCategory::Username,
                identity: identity.as_deref()?,
                original: &value[range.clone()],
            })
        })
        .collect();
    let mut labels = mapping.allocate(&inputs)?.into_iter();
    Ok(contacts
        .into_iter()
        .map(|(range, identity)| {
            let owner = if identity.is_some() {
                Owner::Known(labels.next().expect("allocated username"))
            } else {
                Owner::Unresolved
            };
            (range, PiiCategory::Usernames, owner)
        })
        .collect())
}

pub(super) fn phones(value: &str, mapping: &mut MappingDraft) -> io::Result<Vec<Replacement>> {
    static PHONE: OnceLock<Regex> = OnceLock::new();
    let matcher = PHONE.get_or_init(|| {
        Regex::new(r"\+[0-9][0-9 \t\u{00A0}\u{202F}().-]*").expect("international phone candidate")
    });
    let protected = phone_protected_spans(value)?;
    let mut contacts = Vec::new();
    for (attempt, found) in matcher.find_iter(value).enumerate() {
        if attempt == MAX_FINDINGS {
            return Err(invalid("phone candidates exceed limit"));
        }
        if found.len() > 16 * 1024 {
            return Err(invalid("phone candidate exceeds limit"));
        }
        let mut raw = found
            .as_str()
            .trim_end_matches([' ', '\t', '\u{00a0}', '\u{202f}']);
        raw = raw.strip_suffix('.').unwrap_or(raw);
        let opens = raw.bytes().filter(|c| *c == b'(').count();
        let mut closes = raw.bytes().filter(|c| *c == b')').count();
        while closes > opens && raw.ends_with(')') {
            raw = &raw[..raw.len() - 1];
            closes -= 1;
        }
        let mut range = found.start()..found.start() + raw.len();
        let Some((extension, consumed)) = phone_extension(&value[range.end..]) else {
            continue;
        };
        range.end += consumed;
        if protected
            .range(..range.end)
            .next_back()
            .is_some_and(|(_, end)| *end > range.start)
            || value[..range.start]
                .chars()
                .next_back()
                .is_some_and(|c| word(c) || matches!(c, '+' | '@'))
            || value[range.end..]
                .chars()
                .next()
                .is_some_and(|c| word(c) || matches!(c, '+' | '@'))
        {
            continue;
        }
        let Some(digits) = phone_digits(raw) else {
            continue;
        };
        let identity = serde_json::to_string(&("pii/1/phone", digits, extension))
            .map_err(|_| invalid("invalid phone identity"))?;
        contacts.push((range, identity));
    }
    let inputs: Vec<_> = contacts
        .iter()
        .map(|(range, identity)| PseudonymInput {
            category: PseudonymCategory::Phone,
            identity,
            original: &value[range.clone()],
        })
        .collect();
    let labels = mapping.allocate(&inputs)?;
    Ok(contacts
        .into_iter()
        .zip(labels)
        .map(|((range, _), label)| (range, PiiCategory::Phones, Owner::Known(label)))
        .collect())
}

// An explicit malformed extension invalidates the entire candidate. Retaining
// just the base number would leave a sensitive endpoint suffix in the output.
fn phone_extension(tail: &str) -> Option<(Option<&str>, usize)> {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    let marker = MARKER.get_or_init(|| {
        Regex::new(
            r"(?i)\A[ \t\u{00a0}\u{202f}]*(?:;ext=|(?:extension|ext|доб)\b\.?[ \t]*|x[ \t]*)",
        )
        .expect("phone extension marker")
    });
    let Some(prefix) = marker.find(tail) else {
        return Some((None, 0));
    };
    let digits = &tail[prefix.end()..];
    let count = digits.bytes().take_while(u8::is_ascii_digit).count();
    let end = prefix.end() + count;
    if !(1..=10).contains(&count)
        || tail[end..].chars().next().is_some_and(word)
        || marker.is_match(&tail[end..])
    {
        return None;
    }
    Some((Some(&digits[..count]), end))
}

fn phone_digits(value: &str) -> Option<String> {
    static DATE: OnceLock<Regex> = OnceLock::new();
    if !matches!(value.as_bytes().get(1), Some(b'1'..=b'9'))
        || !value.ends_with(|c: char| c.is_ascii_digit() || c == ')')
        || DATE
            .get_or_init(|| {
                Regex::new(r"^\+[0-9]{4}[-.][0-9]{2}[-.][0-9]{2}$").expect("date shape guard")
            })
            .is_match(value)
    {
        return None;
    }
    let mut depth = 0;
    for c in value.bytes() {
        match c {
            b'(' if depth == 0 => depth = 1,
            b')' if depth == 1 => depth = 0,
            b'(' | b')' => return None,
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    (8..=15)
        .contains(&digits.len())
        .then(|| format!("+{digits}"))
}

fn local_part(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
    {
        return false;
    }
    value.chars().all(|c| {
        if !c.is_ascii() {
            return !c.is_whitespace() && !c.is_control();
        }
        c.is_ascii_alphanumeric() || ".!#$%&'*+-/=?^_`{|}~".contains(c)
    })
}

fn email_domain(value: &str) -> Option<String> {
    if value.is_empty() || value.contains(['%', '@', ':', '/', '\\', '[', ']']) {
        return None;
    }
    let Host::Domain(domain) = Host::parse(value).ok()? else {
        return None;
    };
    if !domain.contains('.')
        || domain.len() > 253
        || !domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return None;
    }
    Some(domain)
}
