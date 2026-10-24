//! `.rotcheckignore`: patterns for files and folders a manifest leaves out.
//!
//! The file sits at the root of the checked tree and holds one pattern per
//! line. Blank lines and lines starting with `#` are skipped. `*` matches any
//! run of characters except `/`; every other character matches itself.
//!
//! - A pattern without a `/` matches a file or folder name at any depth
//!   (`*.tmp`, `.DS_Store`, `node_modules`).
//! - A pattern with a `/` matches the whole path from the root (`cache/*`,
//!   `photos/raw`). A leading `/` is allowed and means the same thing.
//! - A trailing `/` makes the pattern match folders only (`build/`).
//!
//! An ignored folder is not descended into, so everything inside it is
//! ignored too.

use std::fs;
use std::io;
use std::path::Path;

/// Name of the ignore file at the root of a checked tree.
pub const IGNORE_NAME: &str = ".rotcheckignore";

struct Pattern {
    /// Path segments of the pattern; a single segment if it had no `/`.
    segments: Vec<String>,
    anchored: bool,
    dirs_only: bool,
}

#[derive(Default)]
pub struct Ignore {
    patterns: Vec<Pattern>,
}

impl Ignore {
    /// Reads `root/.rotcheckignore`. A missing file means nothing is ignored.
    pub fn load(root: &Path) -> io::Result<Ignore> {
        match fs::read_to_string(root.join(IGNORE_NAME)) {
            Ok(text) => Ok(Ignore::parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Ignore::default()),
            Err(e) => Err(io::Error::new(e.kind(), format!("{IGNORE_NAME}: {e}"))),
        }
    }

    pub fn parse(text: &str) -> Ignore {
        let mut patterns = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let dirs_only = line.ends_with('/');
            let line = line.trim_end_matches('/');
            let anchored = line.contains('/');
            let line = line.trim_start_matches('/');
            if line.is_empty() {
                continue;
            }
            patterns.push(Pattern {
                segments: line.split('/').map(str::to_string).collect(),
                anchored,
                dirs_only,
            });
        }
        Ignore { patterns }
    }

    /// Whether the file or folder at relative path `rel` (with `/`
    /// separators) is ignored.
    pub fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        let segments: Vec<&str> = rel.split('/').collect();
        let name = segments[segments.len() - 1];
        self.patterns.iter().any(|p| {
            if p.dirs_only && !is_dir {
                return false;
            }
            if p.anchored {
                p.segments.len() == segments.len()
                    && p.segments.iter().zip(&segments).all(|(p, s)| glob(p, s))
            } else {
                glob(&p.segments[0], name)
            }
        })
    }
}

/// Matches `text` against `pattern`, where `*` matches any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Classic greedy match with backtracking to the last `*`.
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_cases() {
        assert!(glob("*.tmp", "a.tmp"));
        assert!(glob("*.tmp", ".tmp"));
        assert!(!glob("*.tmp", "a.tmpx"));
        assert!(glob("a*b*c", "aXXbYYc"));
        assert!(!glob("a*b*c", "aXXbYY"));
        assert!(glob("*", ""));
        assert!(glob("exact", "exact"));
        assert!(!glob("exact", "exactly"));
        assert!(glob("ü*", "über"));
    }

    #[test]
    fn name_patterns_match_at_any_depth() {
        let ig = Ignore::parse("# comment\n\n*.tmp\n.DS_Store\n");
        assert!(ig.is_ignored("x.tmp", false));
        assert!(ig.is_ignored("a/b/x.tmp", false));
        assert!(ig.is_ignored("a/.DS_Store", false));
        assert!(!ig.is_ignored("a/x.txt", false));
        assert!(!ig.is_ignored("# comment", false));
    }

    #[test]
    fn path_patterns_are_anchored_to_the_root() {
        let ig = Ignore::parse("cache/*\n/photos/raw\n");
        assert!(ig.is_ignored("cache/x", false));
        assert!(!ig.is_ignored("cache", true));
        assert!(!ig.is_ignored("sub/cache/x", false));
        assert!(ig.is_ignored("photos/raw", true));
        assert!(!ig.is_ignored("old/photos/raw", true));
    }

    #[test]
    fn trailing_slash_matches_folders_only() {
        let ig = Ignore::parse("build/\n");
        assert!(ig.is_ignored("build", true));
        assert!(ig.is_ignored("sub/build", true));
        assert!(!ig.is_ignored("build", false));
    }
}
