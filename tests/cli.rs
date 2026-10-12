//! End-to-end tests that run the real `rotcheck` binary on a temp folder.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

/// A fresh, empty directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rotcheck-cli-{tag}-{}", std::process::id()));
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

fn rotcheck(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rotcheck"))
        .args(args)
        .arg(dir)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn set_mtime(path: &Path, secs: u64) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn init_then_verify_is_clean() {
    let t = TempDir::new("clean");
    fs::write(t.0.join("a.txt"), "a").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());

    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        "rotcheck: 1 ok, 0 corrupted, 0 modified, 0 missing, 0 new\n"
    );
}

#[test]
fn verify_reports_each_kind_of_change() {
    let t = TempDir::new("changes");
    fs::create_dir(t.0.join("2024")).unwrap();
    fs::write(t.0.join("2024/beach.jpg"), "beach").unwrap();
    fs::write(t.0.join("2024/notes.txt"), "notes").unwrap();
    fs::write(t.0.join("old.txt"), "old").unwrap();
    fs::write(t.0.join("scan.png"), "scan").unwrap();
    set_mtime(&t.0.join("scan.png"), 1_700_000_000);
    assert!(rotcheck(&["init"], &t.0).status.success());

    fs::remove_file(t.0.join("old.txt")).unwrap();
    fs::write(t.0.join("2024/notes.txt"), "notes, and more").unwrap();
    fs::write(t.0.join("new.txt"), "new").unwrap();
    // Same size and mtime, different bytes: what bit rot looks like.
    fs::write(t.0.join("scan.png"), "scaN").unwrap();
    set_mtime(&t.0.join("scan.png"), 1_700_000_000);
    // A temp manifest left by a killed run must not show up as a new file.
    fs::write(t.0.join(".rotcheck.sha256.tmp"), "").unwrap();

    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout(&out),
        "MODIFIED    2024/notes.txt\n\
         MISSING     old.txt\n\
         CORRUPTED   scan.png\n\
         NEW         new.txt\n\
         rotcheck: 1 ok, 1 corrupted, 1 modified, 1 missing, 1 new\n"
    );
}

#[test]
fn new_files_alone_are_not_a_failure() {
    let t = TempDir::new("new-only");
    fs::write(t.0.join("a.txt"), "a").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());
    fs::write(t.0.join("b.txt"), "b").unwrap();

    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("NEW         b.txt\n"));
}

#[test]
fn verify_without_manifest_is_a_usage_error() {
    let t = TempDir::new("no-manifest");
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("rotcheck init"));
}

#[test]
fn verify_reads_plain_sha256sum_lists() {
    let t = TempDir::new("plain");
    fs::write(t.0.join("abc.txt"), "abc").unwrap();
    let list = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  abc.txt\n";
    fs::write(t.0.join(".rotcheck.sha256"), list).unwrap();
    assert_eq!(rotcheck(&["verify"], &t.0).status.code(), Some(0));

    fs::write(t.0.join("abc.txt"), "abd").unwrap();
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).starts_with("MODIFIED    abc.txt\n"));
}
