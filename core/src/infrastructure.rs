//! Local, bounded infrastructure recognition. URL spans are parsed separately
//! and shielded from standalone rules. No DNS, filesystem or network probes.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::IpAddr;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};

use crate::pseudonyms::{PseudonymCategory, PseudonymInput, PseudonymMapping};
use crate::sanitize::{MAX_FIELD_BYTES, MAX_FINDINGS};

pub const RULES_VERSION: &str = "infrastructure/1";
const MAX_VALUE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfrastructureCategory {
    Ip,
    Host,
    Domain,
    Url,
    Username,
    Path,
    CloudResource,
}

impl InfrastructureCategory {
    fn mapping(self) -> PseudonymCategory {
        match self {
            Self::Ip => PseudonymCategory::Ip,
            Self::Host => PseudonymCategory::Host,
            Self::Domain => PseudonymCategory::Domain,
            Self::Url => PseudonymCategory::Url,
            Self::Username => PseudonymCategory::Username,
            Self::Path => PseudonymCategory::Path,
            Self::CloudResource => PseudonymCategory::CloudResource,
        }
    }
}

/// Configuration contains private names; serialize only into private settings.
/// All categories are off by default. Bare company domains require explicit scope.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InfrastructurePolicy {
    pub categories: BTreeSet<InfrastructureCategory>,
    pub internal_domains: Vec<String>,
    pub hostnames: Vec<String>,
}

pub struct InfrastructureDetector {
    policy: InfrastructurePolicy,
    domains: BTreeSet<String>,
    hosts: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InfrastructureFinding {
    pub category: InfrastructureCategory,
    pub input: Range<usize>,
    pub output: Range<usize>,
}

/// Text may contain other private data. Diagnostics should use findings only.
pub struct InfrastructureText {
    pub text: String,
    pub findings: Vec<InfrastructureFinding>,
}

struct Match {
    category: InfrastructureCategory,
    range: Range<usize>,
    identity: String,
}

/// Sensitive scan: originals and canonical identities are deliberately private
/// and are neither Debug nor Serialize. `inputs` feeds ProjectStore allocation.
pub struct InfrastructureScan<'a> {
    text: &'a str,
    matches: Vec<Match>,
    spans: BTreeMap<usize, usize>,
}

impl InfrastructureScan<'_> {
    pub fn inputs(&self) -> Vec<PseudonymInput<'_>> {
        self.matches
            .iter()
            .map(|m| PseudonymInput {
                category: m.category.mapping(),
                identity: &m.identity,
                original: &self.text[m.range.clone()],
            })
            .collect()
    }

    pub fn apply(&self, mapping: &PseudonymMapping) -> io::Result<InfrastructureText> {
        let mut text = String::new();
        let mut findings = Vec::new();
        let mut start = 0;
        for m in &self.matches {
            let replacement = mapping
                .lookup(m.category.mapping(), &m.identity)
                .ok_or_else(|| invalid("infrastructure mapping is incomplete"))?;
            text.push_str(&self.text[start..m.range.start]);
            let output_start = text.len();
            text.push_str(replacement);
            findings.push(InfrastructureFinding {
                category: m.category,
                input: m.range.clone(),
                output: output_start..text.len(),
            });
            start = m.range.end;
        }
        text.push_str(&self.text[start..]);
        if text.len() > MAX_FIELD_BYTES {
            return Err(invalid("infrastructure output exceeds field limit"));
        }
        Ok(InfrastructureText { text, findings })
    }
}

impl InfrastructureDetector {
    pub fn new(policy: InfrastructurePolicy) -> io::Result<Self> {
        if policy.internal_domains.len() + policy.hostnames.len() > 256
            || policy
                .internal_domains
                .iter()
                .chain(&policy.hostnames)
                .map(String::len)
                .sum::<usize>()
                > 32 * 1024
        {
            return Err(invalid("infrastructure policy exceeds name limits"));
        }
        let domains = policy
            .internal_domains
            .iter()
            .map(|name| {
                let normalized =
                    domain(name).ok_or_else(|| invalid("invalid configured internal domain"))?;
                if !normalized.contains('.') {
                    return Err(invalid("internal domain needs multiple labels"));
                }
                Ok(normalized)
            })
            .collect::<io::Result<_>>()?;
        let hosts = policy
            .hostnames
            .iter()
            .map(|name| domain(name).ok_or_else(|| invalid("invalid configured hostname")))
            .collect::<io::Result<_>>()?;
        Ok(Self {
            policy,
            domains,
            hosts,
        })
    }

    pub fn scan<'a>(&self, text: &'a str) -> io::Result<InfrastructureScan<'a>> {
        if text.len() > MAX_FIELD_BYTES {
            return Err(invalid("infrastructure field exceeds 16 MiB"));
        }
        let rules = rules();
        let mut scan = InfrastructureScan {
            text,
            matches: Vec::new(),
            spans: BTreeMap::new(),
        };
        if self.policy.categories.is_empty() {
            return Ok(scan);
        }
        let mut protected = BTreeMap::new();
        for candidate in rules.arn.find_iter(text) {
            if !token_boundary(text, &candidate.range()) {
                continue;
            }
            let raw = trim_punctuation(candidate.as_str());
            self.add_cloud(&mut scan, candidate.start()..candidate.start() + raw.len())?;
            protect(&mut protected, candidate.range())?;
        }
        for candidate in rules.url.find_iter(text) {
            if overlaps(&protected, &candidate.range()) {
                continue;
            }
            if candidate.len() > MAX_VALUE_BYTES {
                return Err(invalid("URL candidate exceeds infrastructure limit"));
            }
            if protected.len() == MAX_FINDINGS {
                return Err(invalid("infrastructure spans exceed limit"));
            }
            // Even malformed/unsupported scheme URLs shield their nested spans.
            protected.insert(candidate.start(), candidate.end());
            let raw = trim_punctuation(candidate.as_str());
            let Ok(parsed) = Url::parse(raw) else {
                continue;
            };
            if self.enabled(InfrastructureCategory::Url) && self.internal_url(&parsed) {
                scan.add(
                    InfrastructureCategory::Url,
                    candidate.start()..candidate.start() + raw.len(),
                    parsed.as_str(),
                )?;
            }
        }
        for capture in rules.quoted_path.captures_iter(text) {
            let value = capture
                .name("double")
                .or_else(|| capture.name("single"))
                .expect("quoted path");
            if overlaps(&protected, &value.range()) {
                continue;
            }
            self.add_path(&mut scan, value.range(), true)?;
            protect(&mut protected, value.range())?;
        }
        for value in rules.path.find_iter(text) {
            if overlaps(&protected, &value.range()) || !path_boundary(text, value.start()) {
                continue;
            }
            let raw = trim_punctuation(value.as_str());
            let range = value.start()..value.start() + raw.len();
            let contextual = rules
                .path_context
                .is_match(context_prefix(text, value.start()));
            self.add_path(&mut scan, range, contextual)?;
            protect(&mut protected, value.range())?;
        }
        for capture in rules.ssh.captures_iter(text) {
            let whole = capture.get(0).expect("SSH match");
            if overlaps(&protected, &whole.range()) {
                continue;
            }
            let user = capture.name("user").expect("SSH username");
            let host = capture.name("host").expect("SSH host");
            let raw_host = host.as_str().trim_matches(['[', ']']);
            let Some(host_identity) = domain(raw_host).or_else(|| ip_identity(raw_host)) else {
                continue;
            };
            if self.enabled(InfrastructureCategory::Username) {
                let identity = serde_json::to_string(&("ssh", host_identity, user.as_str()))
                    .expect("string tuple serializes");
                scan.add(InfrastructureCategory::Username, user.range(), &identity)?;
            }
            self.add_host(&mut scan, host.range(), true)?;
            protect(&mut protected, user.start()..host.end())?;
        }
        for capture in rules.username.captures_iter(text) {
            let value = capture
                .name("double")
                .or_else(|| capture.name("single"))
                .or_else(|| capture.name("value"))
                .expect("username context");
            if overlaps(&protected, &value.range()) {
                continue;
            }
            if self.enabled(InfrastructureCategory::Username) {
                let identity = serde_json::to_string(&("unscoped", value.as_str()))
                    .expect("string tuple serializes");
                scan.add(InfrastructureCategory::Username, value.range(), &identity)?;
            }
            protect(&mut protected, value.range())?;
        }
        // Email-shaped text belongs to PII unless explicitly in a login context.
        for value in rules.email.find_iter(text) {
            if !overlaps(&protected, &value.range()) {
                protect(&mut protected, value.range())?;
            }
        }
        for capture in rules.domain_context.captures_iter(text) {
            let value = capture.name("value").expect("domain context");
            if overlaps(&protected, &value.range()) {
                continue;
            }
            if self.enabled(InfrastructureCategory::Domain) {
                if let Some(name) = domain(value.as_str()) {
                    scan.add(InfrastructureCategory::Domain, value.range(), &name)?;
                }
            }
            protect(&mut protected, value.range())?;
        }
        for capture in rules.host_context.captures_iter(text) {
            let value = capture.name("value").expect("host context");
            if !overlaps(&protected, &value.range()) {
                self.add_host(&mut scan, value.range(), true)?;
                protect(&mut protected, value.range())?;
            }
        }
        if self.enabled(InfrastructureCategory::Ip) {
            for pattern in [&rules.ipv6, &rules.ipv4] {
                for candidate in pattern.find_iter(text) {
                    let range = candidate.range();
                    if overlaps(&protected, &range)
                        || !token_boundary(text, &range)
                        || scan.overlaps(&range)
                    {
                        continue;
                    }
                    if let Some(identity) = ip_identity(candidate.as_str()) {
                        scan.add(InfrastructureCategory::Ip, range, &identity)?;
                    }
                }
            }
        }
        if self.enabled(InfrastructureCategory::Host)
            || self.enabled(InfrastructureCategory::Domain)
        {
            for value in rules.name.find_iter(text) {
                if overlaps(&protected, &value.range())
                    || scan.overlaps(&value.range())
                    || !token_boundary(text, &value.range())
                {
                    continue;
                }
                self.add_host(&mut scan, value.range(), false)?;
            }
        }
        scan.matches.sort_by_key(|m| m.range.start);
        Ok(scan)
    }

    fn enabled(&self, category: InfrastructureCategory) -> bool {
        self.policy.categories.contains(&category)
    }

    fn add_path(
        &self,
        scan: &mut InfrastructureScan<'_>,
        range: Range<usize>,
        contextual: bool,
    ) -> io::Result<()> {
        if range.len() > MAX_VALUE_BYTES {
            return Err(invalid("path candidate exceeds infrastructure limit"));
        }
        if self.add_cloud(scan, range.clone())? {
            return Ok(());
        }
        if self.enabled(InfrastructureCategory::Path) {
            if let Some(identity) = path_identity(&scan.text[range.clone()], contextual) {
                scan.add(InfrastructureCategory::Path, range, &identity)?;
            }
        }
        Ok(())
    }

    fn add_cloud(
        &self,
        scan: &mut InfrastructureScan<'_>,
        range: Range<usize>,
    ) -> io::Result<bool> {
        if range.len() > MAX_VALUE_BYTES {
            return Err(invalid("cloud candidate exceeds infrastructure limit"));
        }
        let Some(identity) = cloud_identity(&scan.text[range.clone()]) else {
            return Ok(false);
        };
        if self.enabled(InfrastructureCategory::CloudResource) {
            scan.add(InfrastructureCategory::CloudResource, range, &identity)?;
        }
        Ok(true)
    }

    fn add_host(
        &self,
        scan: &mut InfrastructureScan<'_>,
        range: Range<usize>,
        contextual: bool,
    ) -> io::Result<()> {
        let value = &scan.text[range.clone()];
        let bracketed = value.starts_with('[') && value.ends_with(']');
        let ip_range = if bracketed {
            range.start + 1..range.end - 1
        } else {
            range.clone()
        };
        if let Some(ip) = ip_identity(&scan.text[ip_range.clone()]) {
            if self.enabled(InfrastructureCategory::Ip) {
                scan.add(InfrastructureCategory::Ip, ip_range, &ip)?;
            }
            return Ok(());
        }
        let Some(name) = domain(value) else {
            return Ok(());
        };
        let kind = if self.domains.contains(&name) || name == "home.arpa" {
            InfrastructureCategory::Domain
        } else {
            InfrastructureCategory::Host
        };
        let known = self.hosts.contains(&name)
            || name == "localhost"
            || (name.contains('.') && self.internal_domain(&name));
        if self.enabled(kind) && (contextual || known) {
            scan.add(kind, range, &name)?;
        }
        Ok(())
    }

    fn internal_url(&self, url: &Url) -> bool {
        match url.host() {
            Some(Host::Domain(host)) => self.internal_domain(host),
            Some(Host::Ipv4(ip)) => private_ipv4(ip),
            Some(Host::Ipv6(ip)) => {
                ip.is_unique_local()
                    || ip.is_loopback()
                    || ip.is_unicast_link_local()
                    || ip.is_unspecified()
                    || ip.to_ipv4_mapped().is_some_and(private_ipv4)
            }
            None => false,
        }
    }

    fn internal_domain(&self, host: &str) -> bool {
        let host = host.strip_suffix('.').unwrap_or(host);
        self.hosts.contains(host)
            || ["localhost", "local", "home.arpa", "internal"]
                .into_iter()
                .chain(self.domains.iter().map(String::as_str))
                .any(|suffix| {
                    host == suffix
                        || host
                            .strip_suffix(suffix)
                            .is_some_and(|prefix| prefix.ends_with('.'))
                })
    }
}

impl InfrastructureScan<'_> {
    fn add(
        &mut self,
        category: InfrastructureCategory,
        range: Range<usize>,
        canonical: &str,
    ) -> io::Result<()> {
        if self.matches.len() == MAX_FINDINGS || range.len() > MAX_VALUE_BYTES {
            return Err(invalid("infrastructure matches exceed limits"));
        }
        let identity = if canonical.len() > 4000 {
            format!("infra/1:sha256:{:x}", Sha256::digest(canonical.as_bytes()))
        } else {
            format!("infra/1:value:{canonical}")
        };
        self.matches.push(Match {
            category,
            range: range.clone(),
            identity,
        });
        self.spans.insert(range.start, range.end);
        Ok(())
    }
    fn overlaps(&self, range: &Range<usize>) -> bool {
        overlaps(&self.spans, range)
    }
}

struct Rules {
    url: Regex,
    ipv4: Regex,
    ipv6: Regex,
    name: Regex,
    host_context: Regex,
    domain_context: Regex,
    username: Regex,
    ssh: Regex,
    email: Regex,
    path: Regex,
    quoted_path: Regex,
    path_context: Regex,
    arn: Regex,
}
fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        domain_context: Regex::new(r#"(?i)\b(?:domain|dns_domain|search_domain)[ \t]*[=:][ \t]*["']?(?P<value>[\p{L}\p{N}_.-]+)"#).expect("domain context rule"),
        arn: Regex::new(r#"\barn:[^\s<>"'`]+"#).expect("ARN candidate rule"),
        path: Regex::new(r#"(?:~?/|[A-Za-z]:[\\/]|\\\\)[^\s<>"'`]+"#).expect("path candidate rule"),
        quoted_path: Regex::new(r#""(?P<double>(?:~?/|[A-Za-z]:[\\/]|\\\\)[^"\r\n]+)"|'(?P<single>(?:~?/|[A-Za-z]:[\\/]|\\\\)[^'\r\n]+)'"#).expect("quoted path rule"),
        path_context: Regex::new(r"(?i)\b(?:path|file|dir|directory|logfile)[ \t]*[:=][ \t]*$").expect("path context rule"),
        name: Regex::new(r"[\p{L}\p{N}][\p{L}\p{N}_.-]*").expect("host candidate rule"),
        host_context: Regex::new(r#"(?i)\b(?:host|hostname|server|db_host|database_host)[ \t]*[=:][ \t]*["']?(?P<value>\[[^\]\s]+\]|[\p{L}\p{N}_.%-]+)"#).expect("host context rule"),
        username: Regex::new(r#"(?i)\b(?:user|username|login|db_user|database_user)[ \t]*[=:][ \t]*(?:"(?P<double>(?:\\[^\r\n]|[^"\\\r\n])+)"|'(?P<single>(?:\\[^\r\n]|[^'\\\r\n])+)'|(?P<value>[\p{L}\p{N}_][\p{L}\p{N}_.@\\-]*))"#).expect("username context rule"),
        ssh: Regex::new(r"\b(?:ssh|scp|sftp)[ \t]+(?P<user>[\p{L}\p{N}_][\p{L}\p{N}_.-]*)@(?P<host>\[[^\]\s]+\]|[\p{L}\p{N}_.-]+)").expect("SSH context rule"),
        email: Regex::new(r"[\p{L}\p{N}._%+-]+@[\p{L}\p{N}.-]+").expect("email guard rule"),
        url: Regex::new(r#"(?i)\b[a-z][a-z0-9+.-]{0,31}://[^\s<>"'`]+"#)
            .expect("URL candidate rule"),
        ipv4: Regex::new(r"[0-9]{1,3}(?:\.[0-9]{1,3}){3}").expect("IPv4 candidate rule"),
        ipv6: Regex::new(r#"(?i)(?:[0-9a-f]*:){2,}[0-9a-f:.]*(?:%[^\s\]\[(){}<>"'`,;]*)?"#)
            .expect("IPv6 candidate rule"),
    })
}

fn private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
}

fn domain(value: &str) -> Option<String> {
    if value.len() > 1024 || value.contains(['%', '/', ':', '@', '?', '#', '\\']) {
        return None;
    }
    let Host::Domain(name) = Host::parse(value).ok()? else {
        return None;
    };
    let name = name.strip_suffix('.').unwrap_or(&name);
    if name.is_empty()
        || name.len() > 253
        || name.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

fn cloud_identity(value: &str) -> Option<String> {
    if value.contains(['*', '?', '#', '\\']) {
        return None;
    }
    if value.starts_with("arn:") {
        let parts: Vec<_> = value.splitn(6, ':').collect();
        if parts.len() != 6
            || !(parts[1] == "aws" || parts[1].starts_with("aws-"))
            || parts[1..4].iter().any(|p| {
                !p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            || parts[2].is_empty()
            || (!parts[4].is_empty()
                && (parts[4].len() != 12 || !parts[4].bytes().all(|b| b.is_ascii_digit())))
            || parts[5].is_empty()
        {
            return None;
        }
        return Some(format!("aws:{value}"));
    }
    if let Some(rest) = value.strip_prefix("//") {
        let (service, resource) = rest.split_once('/')?;
        let service = domain(service)?;
        if !service.ends_with(".googleapis.com")
            || resource.split('/').count() < 2
            || resource
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return None;
        }
        return Some(format!("gcp:{service}/{resource}"));
    }
    let mut parts: Vec<_> = value
        .strip_prefix('/')?
        .split('/')
        .map(str::to_owned)
        .collect();
    if parts
        .iter()
        .any(|part| matches!(part.as_str(), "" | "." | "..") || part.contains('%'))
    {
        return None;
    }
    let mut index = 0;
    if parts.first()?.eq_ignore_ascii_case("subscriptions") {
        if !guid(parts.get(1)?) {
            return None;
        }
        parts[0] = "subscriptions".into();
        parts[1].make_ascii_lowercase();
        index = 2;
        if parts
            .get(index)
            .is_some_and(|p| p.eq_ignore_ascii_case("resourceGroups"))
        {
            parts.get(index + 1)?;
            parts[index] = "resourcegroups".into();
            index += 2;
        }
        if index == parts.len() {
            return Some(format!("azure:/{}", parts.join("/")));
        }
    }
    if !parts.get(index)?.eq_ignore_ascii_case("providers")
        || parts.len() < index + 4
        || (parts.len() - index) % 2 != 0
    {
        return None;
    }
    parts[index] = "providers".into();
    let namespace = parts.get_mut(index + 1)?;
    if !namespace.contains('.')
        || !namespace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
    {
        return None;
    }
    namespace.make_ascii_lowercase();
    Some(format!("azure:/{}", parts.join("/")))
}

fn guid(value: &str) -> bool {
    let groups: Vec<_> = value.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn path_boundary(text: &str, start: usize) -> bool {
    !text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '/' | '\\' | '.'))
}

fn context_prefix(text: &str, end: usize) -> &str {
    let mut start = end.saturating_sub(128);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..end]
}

fn path_identity(value: &str, contextual: bool) -> Option<String> {
    // Device and drive-relative paths have different semantics and are not
    // silently treated as ordinary absolute paths.
    if value.starts_with(r"\\?\") || value.starts_with(r"\\.\") {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        return Some(format!(
            "drive:{}{}",
            (bytes[0] as char).to_ascii_uppercase(),
            value[1..].replace('\\', "/")
        ));
    }
    if let Some(rest) = value.strip_prefix(r"\\") {
        let normalized = rest.replace('\\', "/");
        let (server, rest) = normalized.split_once('/')?;
        if rest.is_empty() || rest.starts_with('/') {
            return None;
        }
        let server = domain(server).or_else(|| ip_identity(server))?;
        return Some(format!("unc:{server}/{rest}"));
    }
    let known_root = [
        "/Users",
        "/home",
        "/etc",
        "/var",
        "/opt",
        "/srv",
        "/tmp",
        "/mnt",
        "/media",
        "/run",
        "/root",
        "/workspace",
        "/Volumes",
    ]
    .iter()
    .any(|root| {
        value == *root
            || value
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('/'))
    });
    if value.starts_with("~/")
        || (value.starts_with('/') && !value.starts_with("//") && (known_root || contextual))
    {
        return Some(format!("posix:{value}"));
    }
    None
}

fn ip_identity(value: &str) -> Option<String> {
    let (address, zone) = value
        .split_once('%')
        .map_or((value, None), |(a, z)| (a, Some(z)));
    let ip: IpAddr = address.parse().ok()?;
    if let Some(zone) = zone {
        if !ip.is_ipv6()
            || zone.is_empty()
            || zone.len() > 64
            || !zone
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~'))
        {
            return None;
        }
        return Some(format!("{ip}%{zone}"));
    }
    Some(ip.to_string())
}

fn token_boundary(text: &str, range: &Range<usize>) -> bool {
    let part = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '%');
    !text[..range.start].chars().next_back().is_some_and(part)
        && !text[range.end..].chars().next().is_some_and(part)
}
fn overlaps(ranges: &BTreeMap<usize, usize>, range: &Range<usize>) -> bool {
    ranges
        .range(..range.end)
        .next_back()
        .is_some_and(|(_, end)| *end > range.start)
}

fn protect(ranges: &mut BTreeMap<usize, usize>, range: Range<usize>) -> io::Result<()> {
    if ranges.len() == MAX_FINDINGS {
        return Err(invalid("infrastructure spans exceed limit"));
    }
    ranges.insert(range.start, range.end);
    Ok(())
}
fn trim_punctuation(mut value: &str) -> &str {
    let mut excess = [
        value
            .matches(')')
            .count()
            .saturating_sub(value.matches('(').count()),
        value
            .matches(']')
            .count()
            .saturating_sub(value.matches('[').count()),
        value
            .matches('}')
            .count()
            .saturating_sub(value.matches('{').count()),
    ];
    loop {
        let Some(last) = value.chars().next_back() else {
            return value;
        };
        let closing = match last {
            ')' => Some(0),
            ']' => Some(1),
            '}' => Some(2),
            _ => None,
        };
        if matches!(last, '.' | ',' | ';') {
            value = &value[..value.len() - last.len_utf8()];
        } else if let Some(index) = closing.filter(|index| excess[*index] > 0) {
            excess[index] -= 1;
            value = &value[..value.len() - last.len_utf8()];
        } else {
            return value;
        }
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
