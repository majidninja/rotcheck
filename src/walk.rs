//! Recursive directory walk that produces the list of files a manifest covers.
//!
//! Paths are relative to the root, use `/` as the separator on every platform,
//! and come back sorted by byte order so manifests are stable and diffable.
//! Symlinks are never followed or listed: following them could escape the tree
//! or loop forever, and the target is checked wherever it really lives.

use std::fs;
use std::io;
use std::path::Path;

use crate::ignore::Ignore;

/// Name of the manifest file rotcheck keeps at the root of a checked tree.
pub const MANIFEST_NAME: &str = ".rotcheck.sha256";

/// Temp file `manifest::save` writes next to the manifest. One can be left
/// behind if a run is killed mid-write.
pub const MANIFEST_TMP_NAME: &str = ".rotcheck.sha256.tmp";

/// Lists all regular files under `root`, skipping symlinks and the manifest
/// file (and its temp file) at the top level. Files and folders matched by
/// the tree's `.rotcheckignore` are left out too.
pub fn walk(root: &Path) -> io::Result<Vec<String>> {
    let ignore = Ignore::load(root)?;
    let mut out = Vec::new();
    walk_dir(root, "", &ignore, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_dir(dir: &Path, prefix: &str, ignore: &Ignore, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file name is not valid UTF-8: {:?}", entry.path()),
            ));
        };
        let rel = format!("{prefix}{name}");
        // `file_type` comes from the directory entry and does not follow
        // symlinks, which is exactly what we want.
        let ft = entry.file_type()?;
        if ignore.is_ignored(&rel, ft.is_dir()) {
            continue;
        }
        if ft.is_dir() {
            walk_dir(&entry.path(), &format!("{rel}/"), ignore, out)?;
        } else if ft.is_file() {
            if prefix.is_empty() && (name == MANIFEST_NAME || name == MANIFEST_TMP_NAME) {
                continue;
            }
            out.push(rel);
        }
        // Symlinks, sockets, FIFOs and devices are skipped.
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fresh, empty directory under the system temp dir, removed on drop.
    pub(crate) struct TempDir(pub(crate) PathBuf);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("rotcheck-test-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, rel).unwrap();
    }

    #[test]
    fn empty_dir() {
        let t = TempDir::new("empty");
        assert!(walk(&t.0).unwrap().is_empty());
    }

    #[test]
    fn nested_sorted_with_forward_slashes() {
        let t = TempDir::new("nested");
        for rel in ["b.txt", "a/z.txt", "a/b/c.txt", "A.txt", "a.txt"] {
            touch(&t.0, rel);
        }
        fs::create_dir_all(t.0.join("emptydir")).unwrap();
        assert_eq!(
            walk(&t.0).unwrap(),
            ["A.txt", "a.txt", "a/b/c.txt", "a/z.txt", "b.txt"]
        );
    }

    #[test]
    fn skips_manifest_only_at_top_level() {
        let t = TempDir::new("manifest");
        touch(&t.0, MANIFEST_NAME);
        touch(&t.0, MANIFEST_TMP_NAME);
        touch(&t.0, &format!("sub/{MANIFEST_NAME}"));
        touch(&t.0, "keep");
        assert_eq!(
            walk(&t.0).unwrap(),
            ["keep".to_string(), format!("sub/{MANIFEST_NAME}")]
        );
    }

    #[test]
    fn honours_rotcheckignore() {
        let t = TempDir::new("ignore");
        for rel in [
            "keep.txt",
            "x.tmp",
            "sub/y.tmp",
            "cache/a",
            "cache/b/c",
            "sub/cache",
        ] {
            touch(&t.0, rel);
        }
        fs::write(t.0.join(crate::ignore::IGNORE_NAME), "*.tmp\ncache/\n").unwrap();
        assert_eq!(
            walk(&t.0).unwrap(),
            [crate::ignore::IGNORE_NAME, "keep.txt", "sub/cache"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks() {
        let t = TempDir::new("symlink");
        touch(&t.0, "real/file");
        std::os::unix::fs::symlink(t.0.join("real/file"), t.0.join("link")).unwrap();
        std::os::unix::fs::symlink(t.0.join("real"), t.0.join("dirlink")).unwrap();
        assert_eq!(walk(&t.0).unwrap(), ["real/file"]);
    }

    #[test]
    fn missing_root_is_an_error() {
        let t = TempDir::new("missing");
        assert!(walk(&t.0.join("nope")).is_err());
    }
}
