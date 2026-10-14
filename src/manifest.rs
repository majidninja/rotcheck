//! The manifest file: a `sha256sum`-compatible checksum list with extra
//! metadata in `#` comment lines.
//!
//! ```text
//! # rotcheck manifest v1
//! # size=3 mtime=1700000000
//! 98ea6e4f216f2fb4b69fff9b3a44842c38686ca685f3f55dc48c5d3fb1107be4  notes.txt
//! ```
//!
//! GNU `sha256sum -c` (even with `--strict`) skips lines starting with `#`, so
//! the file can still be checked with standard tools. Each `# size=… mtime=…`
//! line describes the checksum line right after it.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

pub const HEADER: &str = "# rotcheck manifest v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Relative path with `/` separators.
    pub path: String,
    /// Lowercase hex SHA-256.
    pub hash: String,
    pub size: u64,
    /// Modification time in whole seconds since the Unix epoch.
    pub mtime: i64,
}

/// Size and mtime (whole seconds since the epoch) of a file.
pub fn file_meta(path: &Path) -> io::Result<(u64, i64)> {
    let meta = fs::metadata(path)?;
    let mtime = match meta.modified()?.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    Ok((meta.len(), mtime))
}

/// Paths containing these can't be written as one plain `sha256sum` line.
/// Escaping support is roadmap item 6; until then they are rejected.
fn check_path(path: &str) -> io::Result<()> {
    if path.contains(['\n', '\r', '\\']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("file names with newlines or backslashes are not supported yet: {path:?}"),
        ));
    }
    Ok(())
}

pub fn write_to<W: Write>(entries: &[Entry], mut w: W) -> io::Result<()> {
    writeln!(w, "{HEADER}")?;
    for e in entries {
        check_path(&e.path)?;
        writeln!(w, "# size={} mtime={}", e.size, e.mtime)?;
        writeln!(w, "{}  {}", e.hash, e.path)?;
    }
    w.flush()
}

/// Writes the manifest to `path` via a temp file and rename, so an
/// interrupted run never leaves a half-written manifest behind.
pub fn save(entries: &[Entry], path: &Path) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    let result = (|| {
        let file = File::create(tmp)?;
        write_to(entries, BufWriter::new(&file))?;
        file.sync_all()?;
        fs::rename(tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn bad(line_no: usize, msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("line {line_no}: {msg}"))
}

fn parse_meta(rest: &str) -> Option<(u64, i64)> {
    let mut size = None;
    let mut mtime = None;
    for field in rest.split_whitespace() {
        if let Some(v) = field.strip_prefix("size=") {
            size = Some(v.parse().ok()?);
        } else if let Some(v) = field.strip_prefix("mtime=") {
            mtime = Some(v.parse().ok()?);
        }
    }
    Some((size?, mtime?))
}

/// Checks that a manifest path stays inside the folder and returns it in the
/// form the walker produces. A leading `./` (as in `find . | xargs sha256sum`
/// output) is dropped; absolute paths and `.`, `..` or empty components are
/// refused so a hand-edited manifest can't point outside the folder.
fn clean_path(path: &str) -> Result<String, &'static str> {
    let mut path = path;
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    if path.starts_with('/') {
        return Err("absolute paths are not allowed");
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("path must be relative, without empty, '.' or '..' parts");
    }
    Ok(path.to_string())
}

/// Parses manifest text. Checksum lines without a preceding metadata comment
/// (for example a plain `sha256sum` output) get size 0 and mtime 0.
pub fn parse(text: &str) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut pending: Option<(u64, i64)> = None;
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if rest.trim_start().starts_with("size=") {
                pending = Some(parse_meta(rest).ok_or_else(|| bad(line_no, "bad metadata"))?);
            }
            continue;
        }
        let (hash, path) = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .ok_or_else(|| bad(line_no, "expected '<sha256>  <path>'"))?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad(line_no, "not a SHA-256 hex digest"));
        }
        let path = clean_path(path).map_err(|msg| bad(line_no, msg))?;
        let (size, mtime) = pending.take().unwrap_or((0, 0));
        entries.push(Entry {
            path,
            hash: hash.to_ascii_lowercase(),
            size,
            mtime,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> Entry {
        Entry {
            path: path.to_string(),
            hash: "ab".repeat(32),
            size: 42,
            mtime: 1_700_000_000,
        }
    }

    #[test]
    fn round_trip() {
        let entries = vec![entry("a.txt"), entry("dir/b c.jpg")];
        let mut buf = Vec::new();
        write_to(&entries, &mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.starts_with(HEADER));
        assert_eq!(parse(&text).unwrap(), entries);
    }

    #[test]
    fn plain_sha256sum_output_parses() {
        let text = format!("{}  x\n{} *y\n", "AB".repeat(32), "cd".repeat(32));
        let got = parse(&text).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].hash, "ab".repeat(32));
        assert_eq!(got[1].path, "y");
        assert_eq!((got[0].size, got[0].mtime), (0, 0));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("nonsense\n").is_err());
        assert!(parse("abc  file\n").is_err());
    }

    #[test]
    fn rejects_paths_outside_the_folder() {
        let h = "ab".repeat(32);
        for bad_path in [
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a//b",
            "a/./b",
            "a/",
            ".",
        ] {
            let text = format!("{h}  {bad_path}\n");
            assert!(parse(&text).is_err(), "accepted {bad_path:?}");
        }
    }

    #[test]
    fn strips_leading_dot_slash() {
        let text = format!("{}  ./dir/a.txt\n", "ab".repeat(32));
        assert_eq!(parse(&text).unwrap()[0].path, "dir/a.txt");
    }

    #[test]
    fn rejects_unescapable_paths() {
        let mut buf = Vec::new();
        assert!(write_to(&[entry("a\nb")], &mut buf).is_err());
    }
}
