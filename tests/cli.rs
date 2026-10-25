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

#[test]
fn update_accepts_changes_and_refuses_corruption() {
    let t = TempDir::new("update");
    fs::write(t.0.join("notes.txt"), "notes").unwrap();
    fs::write(t.0.join("old.txt"), "old").unwrap();
    fs::write(t.0.join("scan.png"), "scan").unwrap();
    set_mtime(&t.0.join("scan.png"), 1_700_000_000);
    assert!(rotcheck(&["init"], &t.0).status.success());

    fs::remove_file(t.0.join("old.txt")).unwrap();
    fs::write(t.0.join("notes.txt"), "notes, and more").unwrap();
    fs::write(t.0.join("new.txt"), "new").unwrap();
    fs::write(t.0.join("scan.png"), "scaN").unwrap();
    set_mtime(&t.0.join("scan.png"), 1_700_000_000);

    let out = rotcheck(&["update"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout(&out),
        "MODIFIED    notes.txt\n\
         MISSING     old.txt\n\
         CORRUPTED   scan.png\n\
         NEW         new.txt\n\
         rotcheck: manifest updated: 1 added, 1 updated, 1 removed, 0 unchanged, 1 corrupted (kept)\n"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("--accept-corrupted"));

    // Everything but the corrupted file is settled now.
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout(&out),
        "CORRUPTED   scan.png\n\
         rotcheck: 2 ok, 1 corrupted, 0 modified, 0 missing, 0 new\n"
    );

    let out = rotcheck(&["update", "--accept-corrupted"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).ends_with("2 unchanged, 1 corrupted (accepted)\n"));
    assert_eq!(rotcheck(&["verify"], &t.0).status.code(), Some(0));
}

#[test]
fn update_needs_a_manifest_and_known_options() {
    let t = TempDir::new("update-errors");
    assert_eq!(rotcheck(&["update"], &t.0).status.code(), Some(2));
    fs::write(t.0.join(".rotcheck.sha256"), "").unwrap();
    let out = rotcheck(&["update", "--frobnicate"], &t.0);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(t.0.join(".rotcheck.sha256")).unwrap(), b"");
}

#[test]
fn strict_verify_fails_on_new_files() {
    let t = TempDir::new("strict");
    fs::write(t.0.join("a.txt"), "a").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());
    fs::write(t.0.join("b.txt"), "b").unwrap();

    assert_eq!(rotcheck(&["verify"], &t.0).status.code(), Some(0));
    let out = rotcheck(&["verify", "--strict"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("NEW         b.txt\n"));
    assert_eq!(
        rotcheck(&["verify", "--bogus"], &t.0).status.code(),
        Some(2)
    );
}

#[test]
fn manifest_pointing_outside_the_folder_is_rejected() {
    let t = TempDir::new("escape");
    let line = format!("{}  ../secret\n", "ab".repeat(32));
    fs::write(t.0.join(".rotcheck.sha256"), line).unwrap();

    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("line 1"));
}

#[cfg(unix)]
#[test]
fn odd_file_names_survive_init_and_verify() {
    let t = TempDir::new("odd-names");
    fs::write(t.0.join("back\\slash.txt"), "a").unwrap();
    fs::write(t.0.join("new\nline.txt"), "b").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());

    let manifest = fs::read_to_string(t.0.join(".rotcheck.sha256")).unwrap();
    assert!(manifest.contains("  back\\\\slash.txt\n"));
    assert!(manifest.contains("  new\\nline.txt\n"));

    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        "rotcheck: 2 ok, 0 corrupted, 0 modified, 0 missing, 0 new\n"
    );

    // Reports keep one file per line, using the same escapes.
    fs::write(t.0.join("new\nline.txt"), "changed").unwrap();
    fs::write(t.0.join("added\r.txt"), "c").unwrap();
    let out = stdout(&rotcheck(&["verify"], &t.0));
    assert!(out.contains("MODIFIED    new\\nline.txt\n"), "{out}");
    assert!(out.contains("NEW         added\\r.txt\n"), "{out}");

    let out = stdout(&rotcheck(&["list"], &t.0));
    assert_eq!(out, "added\\r.txt\nback\\\\slash.txt\nnew\\nline.txt\n");

    let out = Command::new(env!("CARGO_BIN_EXE_rotcheck"))
        .arg("hash")
        .arg(t.0.join("back\\slash.txt"))
        .output()
        .unwrap();
    let out = stdout(&out);
    assert!(
        out.starts_with('\\') && out.ends_with("/back\\\\slash.txt\n"),
        "{out}"
    );
}

#[test]
fn verify_quiet_prints_only_failures() {
    let t = TempDir::new("quiet");
    fs::write(t.0.join("a.txt"), "a").unwrap();
    fs::write(t.0.join("b.txt"), "b").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());

    // A new file alone: nothing printed, exit 0. With --strict it counts.
    fs::write(t.0.join("new.txt"), "new").unwrap();
    let out = rotcheck(&["verify", "--quiet"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "");
    let out = rotcheck(&["verify", "--quiet", "--strict"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "NEW         new.txt\n");

    fs::remove_file(t.0.join("b.txt")).unwrap();
    let out = rotcheck(&["verify", "--quiet"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "MISSING     b.txt\n");
}

#[test]
fn update_quiet_is_silent_on_success() {
    let t = TempDir::new("update-quiet");
    fs::write(t.0.join("a.txt"), "a").unwrap();
    fs::write(t.0.join("b.txt"), "b").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());

    // Added and removed files are accepted without a word.
    fs::write(t.0.join("new.txt"), "new").unwrap();
    fs::remove_file(t.0.join("b.txt")).unwrap();
    let out = rotcheck(&["update", "--quiet"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "");
    assert!(out.stderr.is_empty());

    // The manifest really was updated.
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    let out = stdout(&out);
    assert!(out.contains("2 ok, 0 corrupted"), "{out}");
}

#[test]
fn report_paths_start_at_column_13() {
    // The README promises `cut -c13-` gives the path for every status word.
    let t = TempDir::new("columns");
    fs::write(t.0.join("gone.txt"), "x").unwrap();
    fs::write(t.0.join("edit.txt"), "x").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());
    fs::remove_file(t.0.join("gone.txt")).unwrap();
    fs::write(t.0.join("edit.txt"), "longer").unwrap();
    fs::write(t.0.join("fresh.txt"), "x").unwrap();

    let out = rotcheck(&["verify", "--strict", "--quiet"], &t.0);
    let out = stdout(&out);
    let paths: Vec<&str> = out.lines().map(|l| &l[12..]).collect();
    assert_eq!(paths, ["edit.txt", "gone.txt", "fresh.txt"]);
}

#[test]
fn changelog_top_entry_matches_version() {
    // Bumping the version in Cargo.toml without a CHANGELOG entry (or the
    // other way round) fails here.
    let changelog = include_str!("../CHANGELOG.md");
    let top = changelog
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .find(|h| *h != "Unreleased")
        .expect("CHANGELOG.md has no version heading");
    let version = top.split(' ').next().unwrap();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));

    let out = Command::new(env!("CARGO_BIN_EXE_rotcheck"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(stdout(&out), format!("rotcheck {version}\n"));
}

#[test]
fn rotcheckignore_leaves_files_out() {
    let t = TempDir::new("ignore");
    fs::create_dir_all(t.0.join("cache")).unwrap();
    fs::write(t.0.join("keep.txt"), "k").unwrap();
    fs::write(t.0.join("scratch.tmp"), "s").unwrap();
    fs::write(t.0.join("cache/blob"), "b").unwrap();
    fs::write(t.0.join(".rotcheckignore"), "*.tmp\ncache/\n").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());

    // Changes to ignored files don't show up in verify.
    fs::write(t.0.join("scratch.tmp"), "changed").unwrap();
    fs::write(t.0.join("cache/new"), "n").unwrap();
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        "rotcheck: 2 ok, 0 corrupted, 0 modified, 0 missing, 0 new\n"
    );
}

#[test]
fn newly_ignored_entries_are_explained() {
    let t = TempDir::new("ignore-later");
    fs::create_dir_all(t.0.join("cache")).unwrap();
    fs::write(t.0.join("keep.txt"), "k").unwrap();
    fs::write(t.0.join("cache/blob"), "b").unwrap();
    fs::write(t.0.join("gone.txt"), "g").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());

    // Ignore the cache after the fact, and delete a file for real.
    fs::write(t.0.join(".rotcheckignore"), "cache/\n").unwrap();
    fs::remove_file(t.0.join("gone.txt")).unwrap();
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("MISSING     cache/blob\n"), "{text}");
    let dir = t.0.display();
    assert!(
        text.ends_with(&format!(
            "1 of the missing files match .rotcheckignore; run `rotcheck update {dir}` to drop them\n"
        )),
        "{text}"
    );

    let out = rotcheck(&["update"], &t.0);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("2 removed, 1 unchanged (1 of the removed are now ignored)"),
        "{text}"
    );
}
