//! Local OOXML rewriting. No Office process, macros, links or formulas execute.
//! Russian three-part names are shortened; this is NOT full anonymization.

use std::collections::BTreeSet;
use std::io::{self, Cursor, Read, Write};
use std::ops::Range;
use std::sync::OnceLock;

use quick_xml::{events::Event, Reader};
use regex::Regex;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const MAX_ENTRIES: usize = 2048;
const MAX_EXPANDED: u64 = 64 * 1024 * 1024;
const MAX_XML: u64 = 16 * 1024 * 1024;

pub struct OfficeDocument {
    pub bytes: Vec<u8>,
    pub replacements: usize,
}

fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn names() -> &'static Regex {
    static RULE: OnceLock<Regex> = OnceLock::new();
    RULE.get_or_init(|| {
        Regex::new(concat!(
            r"\b(?P<s>[А-ЯЁ][а-яё]+(?:-[А-ЯЁ][а-яё]+)?)",
            r"[ \t\u{a0}]+(?P<n>[А-ЯЁ][а-яё]+)",
            r"[ \t\u{a0}]+(?P<p>[А-ЯЁ][а-яё]*(?:вич|вна|тич|ична|ич))\b"
        ))
        .expect("built-in initials rule")
    })
}

/// Explicit surname-first Cyrillic FIO grammar, including hyphenated surnames.
/// Unknown names/orders are left intact, not advertised as successfully hidden.
pub fn abbreviate_names(text: &str) -> (String, usize) {
    let mut count = 0;
    let value = names().replace_all(text, |c: &regex::Captures<'_>| {
        count += 1;
        format!(
            "{} {}. {}.",
            &c["s"],
            c["n"].chars().next().unwrap(),
            c["p"].chars().next().unwrap()
        )
    });
    (value.into_owned(), count)
}

struct TextNode {
    xml: Range<usize>,
    text: String,
}

fn rewrite_group(nodes: &mut Vec<TextNode>, edits: &mut Vec<(Range<usize>, String)>) -> usize {
    let text: String = nodes.iter().map(|n| n.text.as_str()).collect();
    let matches: Vec<_> = names()
        .captures_iter(&text)
        .map(|c| {
            let matched = c.get(0).unwrap();
            (
                matched.range(),
                format!(
                    "{} {}. {}.",
                    &c["s"],
                    c["n"].chars().next().unwrap(),
                    c["p"].chars().next().unwrap()
                ),
            )
        })
        .collect();
    let mut offset = 0;
    for node in nodes.drain(..) {
        let end = offset + node.text.len();
        let mut replacement = String::new();
        let mut cursor = offset;
        for (range, value) in &matches {
            if range.start >= end || range.end <= offset {
                continue;
            }
            let start = range.start.max(offset);
            replacement.push_str(&node.text[cursor - offset..start - offset]);
            if range.start >= offset {
                replacement.push_str(value);
            }
            cursor = range.end.min(end);
        }
        replacement.push_str(&node.text[cursor - offset..]);
        if replacement != node.text {
            edits.push((
                node.xml,
                quick_xml::escape::escape(&replacement).into_owned(),
            ));
        }
        offset = end;
    }
    matches.len()
}

fn rewrite_xml(bytes: &[u8], cancelled: &impl Fn() -> bool) -> io::Result<(Vec<u8>, usize)> {
    let xml = std::str::from_utf8(bytes).map_err(|_| invalid("Office XML must be UTF-8"))?;
    let mut reader = Reader::from_str(xml);
    let mut nodes = Vec::new();
    let mut edits = Vec::new();
    let mut parents = Vec::<String>::new();
    let mut count = 0;
    loop {
        if cancelled() {
            return Err(crate::cancelled());
        }
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(invalid)?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(tag) => {
                let name = tag.local_name().as_ref().to_owned();
                if matches!(name.as_str(), "p" | "si" | "c") {
                    count += rewrite_group(&mut nodes, &mut edits);
                }
                if parents.len() >= 256 {
                    return Err(invalid("Office XML nesting exceeds limit"));
                }
                parents.push(name);
            }
            Event::End(tag) => {
                if matches!(tag.local_name().as_ref(), "p" | "si" | "c") {
                    count += rewrite_group(&mut nodes, &mut edits);
                }
                parents.pop();
            }
            Event::Text(text) => {
                let raw = text.as_ref();
                let decoded = quick_xml::escape::unescape(raw)
                    .map_err(invalid)?
                    .into_owned();
                if parents.last().is_some_and(|p| p == "t") {
                    nodes.push(TextNode {
                        xml: start..end,
                        text: decoded,
                    });
                } else if !decoded.trim().is_empty() {
                    count += rewrite_group(&mut nodes, &mut edits);
                    let (changed, replacements) = abbreviate_names(&decoded);
                    if replacements > 0 {
                        count += replacements;
                        edits.push((start..end, quick_xml::escape::escape(&changed).into_owned()));
                    }
                }
            }
            Event::GeneralRef(_) => {
                if parents.last().is_some_and(|p| p == "t") {
                    let decoded = quick_xml::escape::unescape(&xml[start..end])
                        .map_err(invalid)?
                        .into_owned();
                    nodes.push(TextNode {
                        xml: start..end,
                        text: decoded,
                    });
                }
            }
            Event::DocType(_) | Event::CData(_) => {
                return Err(invalid("Office XML DTD/CDATA is unsupported"))
            }
            Event::Eof => break,
            _ => (),
        }
    }
    if !parents.is_empty() {
        return Err(invalid("Office XML is incomplete"));
    }
    count += rewrite_group(&mut nodes, &mut edits);
    let mut output = xml.to_owned();
    edits.sort_by_key(|(range, _)| range.start);
    for (range, value) in edits.into_iter().rev() {
        output.replace_range(range, &value);
    }
    // Also covers author/name attributes without changing XML markup. Cyrillic
    // names contain none of the XML delimiter characters.
    let (output, attributes) = abbreviate_names(&output);
    Ok((output.into_bytes(), count + attributes))
}

pub fn process_office(
    bytes: &[u8],
    extension: &str,
    cancelled: impl Fn() -> bool,
) -> io::Result<OfficeDocument> {
    if !matches!(extension, "docx" | "xlsx") {
        return Err(invalid("only DOCX/XLSX are supported"));
    }
    let mut input = ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    if input.len() > MAX_ENTRIES {
        return Err(invalid("Office package has too many entries"));
    }
    let required = if extension == "docx" {
        "word/document.xml"
    } else {
        "xl/workbook.xml"
    };
    if input.by_name(required).is_err() || input.by_name("[Content_Types].xml").is_err() {
        return Err(invalid("Office package does not match its extension"));
    }
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let mut total = 0u64;
    let mut replacements = 0;
    let mut seen = BTreeSet::new();
    for i in 0..input.len() {
        if cancelled() {
            return Err(crate::cancelled());
        }
        let mut entry = input.by_index(i).map_err(invalid)?;
        let name = entry.name().to_owned();
        let lower = name.to_ascii_lowercase();
        if !seen.insert(name.clone())
            || name.contains('\\')
            || name.starts_with('/')
            || name.split('/').any(|s| s == ".." || s == ".")
            || lower.contains("vbaproject")
            || lower.contains("activex/")
            || lower.contains("embeddings/")
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(invalid(
                "Office package contains an unsupported or unsafe entry",
            ));
        }
        if entry.is_dir() {
            output
                .add_directory(name, SimpleFileOptions::default())
                .map_err(invalid)?;
            continue;
        }
        let limit = if lower.ends_with(".xml") {
            MAX_XML
        } else {
            MAX_EXPANDED
        };
        let limit = limit.min(MAX_EXPANDED.saturating_sub(total));
        if entry.size() > limit {
            return Err(invalid("Office expanded size exceeds limit"));
        }
        let mut content = Vec::new();
        (&mut entry)
            .take(limit + 1)
            .read_to_end(&mut content)
            .map_err(invalid)?;
        if content.len() as u64 > limit {
            return Err(invalid("Office expanded size exceeds limit"));
        }
        total += content.len() as u64;
        if lower.ends_with(".xml") {
            let rewritten = rewrite_xml(&content, &cancelled)?;
            content = rewritten.0;
            replacements += rewritten.1;
        }
        output
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(invalid)?;
        output.write_all(&content)?;
    }
    Ok(OfficeDocument {
        bytes: output.finish().map_err(invalid)?.into_inner(),
        replacements,
    })
}
