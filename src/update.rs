//! Brings a manifest in line with what is on disk.
//!
//! New files are added, missing ones dropped and edited ones re-recorded.
//! Files that look corrupted are the exception: their old checksum is kept
//! unless the caller explicitly accepts them, so `update` can't quietly
//! bless bit rot.

use std::io;
use std::path::Path;

use crate::manifest::Entry;
use crate::verify::{self, Status, Summary};

/// Returns the entries for the new manifest, sorted by path, and a summary of
/// what was found. `report` is called once per file, as in `verify`; a new
/// file that can't be read is reported as unreadable and left out. `total`
/// gets the number of files up front, as in `verify`. Only paths in `scope`
/// (see [`verify::in_scope`]; empty means everything) are looked at; entries
/// outside it are kept as they are.
pub fn update<T: FnOnce(usize), F: FnMut(&str, Status)>(
    root: &Path,
    old: &[Entry],
    scope: &str,
    accept_corrupted: bool,
    total: T,
    mut report: F,
) -> io::Result<(Vec<Entry>, Summary)> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut outside = std::collections::BTreeSet::new();
    for e in old.iter().filter(|e| !verify::in_scope(&e.path, scope)) {
        if outside.insert(e.path.as_str()) {
            entries.push(e.clone()); // listed twice: the first line wins
        }
    }
    let mut unreadable_new = 0;
    let mut summary = verify::scan(root, old, scope, total, |path, status, current| {
        let mut status = status;
        let previous = || old.iter().find(|e| e.path == path).cloned();
        let keep = match status {
            // Same content: take the fresh size and mtime along. This also
            // fills in metadata for entries from a plain `sha256sum` list.
            Status::Ok | Status::Modified => current,
            Status::Corrupted if accept_corrupted => current,
            Status::Corrupted | Status::Unreadable => previous(),
            Status::Missing => None,
            Status::New => match verify::current_entry(root, path) {
                Ok(entry) => Some(entry),
                Err(_) => {
                    status = Status::Unreadable;
                    unreadable_new += 1;
                    None
                }
            },
        };
        entries.extend(keep);
        report(path, status);
    })?;
    summary.new -= unreadable_new;
    summary.unreadable += unreadable_new;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((entries, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;
    use crate::walk::tests::TempDir;
    use std::fs;

    /// A tree with one file of each kind: untouched, edited, deleted, added
    /// and corrupted (same size and recorded mtime, different bytes).
    fn changed_tree(tag: &str) -> (TempDir, Vec<Entry>) {
        let tmp = TempDir::new(tag);
        let root = &tmp.0;
        for name in ["same.txt", "gone.txt", "edited.txt", "rotten.bin"] {
            fs::write(root.join(name), name).unwrap();
        }
        let mut old = crate::build_entries(root).unwrap();
        fs::remove_file(root.join("gone.txt")).unwrap();
        fs::write(root.join("edited.txt"), "a longer text than before").unwrap();
        fs::write(root.join("added.txt"), "hi").unwrap();
        fs::write(root.join("rotten.bin"), "rotten.biN").unwrap();
        let (_, mtime) = manifest::file_meta(&root.join("rotten.bin")).unwrap();
        let rotten = old.iter_mut().find(|e| e.path == "rotten.bin").unwrap();
        rotten.mtime = mtime;
        (tmp, old)
    }

    fn find(list: &[Entry], path: &str) -> Entry {
        list.iter().find(|e| e.path == path).cloned().unwrap()
    }

    #[test]
    fn accepts_changes_but_keeps_corrupted_checksum() {
        let (tmp, old) = changed_tree("update-keep");
        let (new, summary) = update(&tmp.0, &old, "", false, |_| {}, |_, _| {}).unwrap();
        let paths: Vec<&str> = new.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["added.txt", "edited.txt", "rotten.bin", "same.txt"]);
        assert_eq!(find(&new, "rotten.bin"), find(&old, "rotten.bin"));
        assert_ne!(find(&new, "edited.txt").hash, find(&old, "edited.txt").hash);
        assert_eq!(find(&new, "edited.txt").size, 25);
        assert_eq!((summary.new, summary.modified, summary.missing), (1, 1, 1));
        assert_eq!((summary.ok, summary.corrupted), (1, 1));

        // The corrupted file is still flagged afterwards; nothing else is.
        let after = verify::verify(&tmp.0, &new, "", |_| {}, |_, _| {}).unwrap();
        assert_eq!((after.ok, after.corrupted), (3, 1));
    }

    #[test]
    fn accept_corrupted_records_the_new_content() {
        let (tmp, old) = changed_tree("update-accept");
        let (new, _) = update(&tmp.0, &old, "", true, |_| {}, |_, _| {}).unwrap();
        let after = verify::verify(&tmp.0, &new, "", |_| {}, |_, _| {}).unwrap();
        assert_eq!(after.ok, 4);
        assert_eq!(after.new, 0);
        assert!(!after.has_problems());
    }

    #[test]
    fn fills_in_metadata_for_plain_lists() {
        let tmp = TempDir::new("update-plain");
        fs::write(tmp.0.join("a"), "abc").unwrap();
        let mut old = crate::build_entries(&tmp.0).unwrap();
        old[0].size = 0;
        old[0].mtime = 0;
        let (new, summary) = update(&tmp.0, &old, "", false, |_| {}, |_, _| {}).unwrap();
        assert_eq!(summary.ok, 1);
        assert_eq!(new[0].size, 3);
        assert_ne!(new[0].mtime, 0);
    }

    #[test]
    fn scope_leaves_other_entries_alone() {
        let tmp = TempDir::new("update-scope");
        let root = &tmp.0;
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("top.txt"), "top").unwrap();
        fs::write(root.join("sub/a.txt"), "a").unwrap();
        let old = crate::build_entries(root).unwrap();
        fs::write(root.join("top.txt"), "top, edited").unwrap();
        fs::write(root.join("sub/a.txt"), "a, edited").unwrap();
        fs::write(root.join("sub/b.txt"), "b").unwrap();
        fs::write(root.join("new.txt"), "n").unwrap();
        let (new, summary) = update(root, &old, "sub", false, |_| {}, |_, _| {}).unwrap();
        let paths: Vec<&str> = new.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["sub/a.txt", "sub/b.txt", "top.txt"]);
        assert_eq!(find(&new, "top.txt"), find(&old, "top.txt"));
        assert_ne!(find(&new, "sub/a.txt").hash, find(&old, "sub/a.txt").hash);
        assert_eq!((summary.new, summary.modified, summary.ok), (1, 1, 0));
    }
}
