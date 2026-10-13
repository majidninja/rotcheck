//! Compares a directory tree against its manifest.
//!
//! Every file the manifest lists is hashed again. A file whose content changed
//! is **modified** if its size or mtime changed too (somebody wrote to it) and
//! **corrupted** if both still match the manifest: nothing admitted to
//! touching the file, yet the bytes are different.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::Path;

use crate::manifest::{self, Entry};
use crate::walk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Listed in the manifest but not on disk.
    Missing,
    /// On disk but not in the manifest.
    New,
    /// Content differs and so does the size or mtime.
    Modified,
    /// Content differs although size and mtime are unchanged.
    Corrupted,
    /// The file is there but could not be read.
    Unreadable,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Status::Ok => "OK",
            Status::Missing => "MISSING",
            Status::New => "NEW",
            Status::Modified => "MODIFIED",
            Status::Corrupted => "CORRUPTED",
            Status::Unreadable => "UNREADABLE",
        })
    }
}

/// How many files ended up with each status.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub ok: usize,
    pub missing: usize,
    pub new: usize,
    pub modified: usize,
    pub corrupted: usize,
    pub unreadable: usize,
}

impl Summary {
    fn count(&mut self, status: Status) {
        *match status {
            Status::Ok => &mut self.ok,
            Status::Missing => &mut self.missing,
            Status::New => &mut self.new,
            Status::Modified => &mut self.modified,
            Status::Corrupted => &mut self.corrupted,
            Status::Unreadable => &mut self.unreadable,
        } += 1;
    }

    /// True if a file the manifest covers is gone, different or unreadable.
    /// New files don't count: they are reported, but nothing was lost.
    pub fn has_problems(&self) -> bool {
        self.missing + self.modified + self.corrupted + self.unreadable > 0
    }
}

/// Decides the status of a file that exists on disk, given what the manifest
/// recorded and what is there now.
pub fn classify(entry: &Entry, size: u64, mtime: i64, hash: &str) -> Status {
    if entry.hash == hash {
        // Same content. A changed mtime alone (`touch`, a restore from
        // backup) is not worth reporting.
        Status::Ok
    } else if entry.size == size && entry.mtime == mtime {
        Status::Corrupted
    } else {
        // Also where entries from a plain `sha256sum` list end up: they carry
        // no metadata, so there is nothing to tell the two cases apart.
        Status::Modified
    }
}

/// Checks `root` against `entries`, calling `report` once per file: manifest
/// entries in manifest order first, then new files in path order.
pub fn verify<F: FnMut(&str, Status)>(
    root: &Path,
    entries: &[Entry],
    mut report: F,
) -> io::Result<Summary> {
    scan(root, entries, |path, status, _| report(path, status))
}

/// Like [`verify`], but also hands `report` what is on disk right now for
/// every file that was hashed (ok, modified or corrupted), so `update` does
/// not have to read those files a second time.
pub fn scan<F: FnMut(&str, Status, Option<Entry>)>(
    root: &Path,
    entries: &[Entry],
    mut report: F,
) -> io::Result<Summary> {
    let on_disk: BTreeSet<String> = walk::walk(root)?.into_iter().collect();
    let mut known = BTreeSet::new();
    let mut summary = Summary::default();
    for entry in entries {
        if !known.insert(entry.path.as_str()) {
            continue; // listed twice; the first line wins
        }
        let (status, current) = if on_disk.contains(&entry.path) {
            match current_entry(root, &entry.path) {
                Ok(now) => (classify(entry, now.size, now.mtime, &now.hash), Some(now)),
                Err(_) => (Status::Unreadable, None),
            }
        } else {
            (Status::Missing, None)
        };
        summary.count(status);
        report(&entry.path, status, current);
    }
    for path in on_disk.iter().filter(|p| !known.contains(p.as_str())) {
        summary.count(Status::New);
        report(path, Status::New, None);
    }
    Ok(summary)
}

/// Reads and hashes the file at `rel` under `root`. Metadata is read before
/// hashing so a file written to mid-hash looks modified (not corrupted) on
/// the next verify.
pub fn current_entry(root: &Path, rel: &str) -> io::Result<Entry> {
    let full = root.join(rel);
    let (size, mtime) = manifest::file_meta(&full)?;
    let hash = crate::hash_file(&full)?;
    Ok(Entry {
        path: rel.to_string(),
        hash,
        size,
        mtime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::tests::TempDir;
    use std::fs;

    fn entry(hash: &str, size: u64, mtime: i64) -> Entry {
        Entry {
            path: "f".to_string(),
            hash: hash.to_string(),
            size,
            mtime,
        }
    }

    #[test]
    fn classify_cases() {
        let e = entry("aa", 10, 100);
        assert_eq!(classify(&e, 10, 100, "aa"), Status::Ok);
        assert_eq!(classify(&e, 10, 999, "aa"), Status::Ok);
        assert_eq!(classify(&e, 10, 100, "bb"), Status::Corrupted);
        assert_eq!(classify(&e, 10, 101, "bb"), Status::Modified);
        assert_eq!(classify(&e, 11, 100, "bb"), Status::Modified);
    }

    #[test]
    fn verify_reports_every_kind() {
        let tmp = TempDir::new("verify");
        let root = &tmp.0;
        fs::create_dir(root.join("sub")).unwrap();
        for name in ["same.txt", "gone.txt", "edited.txt", "sub/rotten.bin"] {
            fs::write(root.join(name), name).unwrap();
        }
        let mut entries = crate::build_entries(root).unwrap();

        fs::remove_file(root.join("gone.txt")).unwrap();
        fs::write(root.join("edited.txt"), "a longer text than before").unwrap();
        fs::write(root.join("fresh.txt"), "hi").unwrap();
        // Same length, different bytes. Then record whatever mtime the file
        // has now, as if the timestamp had never moved.
        let rotten = root.join("sub/rotten.bin");
        fs::write(&rotten, "sub/rotten.biN").unwrap();
        let (_, mtime) = manifest::file_meta(&rotten).unwrap();
        entries
            .iter_mut()
            .find(|e| e.path == "sub/rotten.bin")
            .unwrap()
            .mtime = mtime;

        let mut seen = Vec::new();
        let summary = verify(root, &entries, |p, s| seen.push((p.to_string(), s))).unwrap();
        assert_eq!(
            seen,
            [
                ("edited.txt".to_string(), Status::Modified),
                ("gone.txt".to_string(), Status::Missing),
                ("same.txt".to_string(), Status::Ok),
                ("sub/rotten.bin".to_string(), Status::Corrupted),
                ("fresh.txt".to_string(), Status::New),
            ]
        );
        assert_eq!(
            summary,
            Summary {
                ok: 1,
                missing: 1,
                new: 1,
                modified: 1,
                corrupted: 1,
                unreadable: 0,
            }
        );
        assert!(summary.has_problems());
    }

    #[test]
    fn untouched_tree_is_clean() {
        let tmp = TempDir::new("verify-clean");
        fs::write(tmp.0.join("a"), "a").unwrap();
        let entries = crate::build_entries(&tmp.0).unwrap();
        let summary = verify(&tmp.0, &entries, |_, s| assert_eq!(s, Status::Ok)).unwrap();
        assert_eq!(summary.ok, 1);
        assert!(!summary.has_problems());
    }
}
