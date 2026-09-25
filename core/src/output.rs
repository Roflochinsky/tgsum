//! Output writer: one `.md` per unit, `name.part-N.md` when split.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::format::{format_unit, Formatted};
use crate::model::ExtractedUnit;

/// Writes every unit into `out_dir` (created if missing) and returns the paths
/// written, in order.
///
/// Colliding names get ` (2)`, ` (3)`, … instead of overwriting each other;
/// the comparison ignores case, since macOS and Windows file systems do. Old
/// `<stem>.md` / `<stem>.part-N.md` files are removed before writing, so a
/// re-run that yields fewer parts leaves no stale ones behind.
pub fn write_units(
    units: &[ExtractedUnit],
    out_dir: &Path,
    max_tokens: usize,
) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(out_dir)?;
    let mut written = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for unit in units {
        let Formatted { filename, parts } = format_unit(unit, max_tokens);
        let base = filename.strip_suffix(".md").unwrap_or(&filename);
        let n = seen.entry(base.to_lowercase()).or_insert(0);
        *n += 1;
        let stem = if *n == 1 {
            base.to_owned()
        } else {
            format!("{base} ({n})")
        };
        clear_stem(out_dir, &stem);
        if let [only] = parts.as_slice() {
            let path = out_dir.join(format!("{stem}.md"));
            fs::write(&path, only)?;
            written.push(path);
        } else {
            for (i, content) in parts.iter().enumerate() {
                let path = out_dir.join(format!("{stem}.part-{}.md", i + 1));
                fs::write(&path, content)?;
                written.push(path);
            }
        }
    }
    Ok(written)
}

/// Removes `<stem>.md` and `<stem>.part-<N>.md` from `dir`.
fn clear_stem(dir: &Path, stem: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| is_stem_file(name, stem)) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `^<stem>(\.part-\d+)?\.md$`
fn is_stem_file(name: &str, stem: &str) -> bool {
    let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_suffix(".md")) else {
        return false;
    };
    rest.is_empty()
        || rest
            .strip_prefix(".part-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::is_stem_file;

    #[test]
    fn stem_file_pattern() {
        assert!(is_stem_file("C.md", "C"));
        assert!(is_stem_file("C.part-12.md", "C"));
        assert!(!is_stem_file("C.part-.md", "C"));
        assert!(!is_stem_file("C.part-1a.md", "C"));
        assert!(!is_stem_file("C (2).md", "C"));
        assert!(!is_stem_file("CC.md", "C"));
        assert!(!is_stem_file("C.txt", "C"));
    }
}
