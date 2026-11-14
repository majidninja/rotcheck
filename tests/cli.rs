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

#[cfg(unix)]
#[test]
fn closed_output_pipe_does_not_panic() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    let tmp = TempDir::new("pipe");
    for i in 0..2000 {
        fs::write(tmp.0.join(format!("file-with-a-longish-name-{i}")), "x").unwrap();
    }
    // Like `rotcheck list DIR | head -0`: the reader is gone before we write.
    let mut child = Command::new(env!("CARGO_BIN_EXE_rotcheck"))
        .args(["list"])
        .arg(&tmp.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked"), "stderr: {err}");
    assert_eq!(out.status.signal(), Some(13), "status: {:?}", out.status);
}

#[test]
fn hash_dash_reads_stdin() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rotcheck"))
        .args(["hash", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"abc").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  -\n"
    );
}

#[test]
fn hash_rejects_unknown_options_but_honours_double_dash() {
    let t = TempDir::new("hashopts");
    fs::write(t.0.join("-x"), "abc").unwrap();
    // Run inside the folder: the `rotcheck` helper appends a DIR argument.
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rotcheck"))
            .args(args)
            .current_dir(&t.0)
            .output()
            .unwrap()
    };

    let out = run(&["hash", "-x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty());

    let out = run(&["hash", "--", "-x"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  -x\n"
    );

    let out = run(&["hash", "--"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn folder_commands_reject_dash_options_and_honour_double_dash() {
    let t = TempDir::new("dashdir");
    let odd = t.0.join("-odd");
    fs::create_dir(&odd).unwrap();
    fs::write(odd.join("a.txt"), "a").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rotcheck"))
            .args(args)
            .current_dir(&t.0)
            .output()
            .unwrap()
    };

    // A folder name starting with a dash is an unknown option...
    for cmd in ["init", "verify", "update", "list"] {
        assert_eq!(run(&[cmd, "-odd"]).status.code(), Some(2), "{cmd}");
        assert_eq!(run(&[cmd, "--bogus", "-odd"]).status.code(), Some(2));
    }
    assert!(!odd.join(".rotcheck.sha256").exists());

    // ...unless it comes after `--`.
    assert!(run(&["init", "--", "-odd"]).status.success());
    assert!(odd.join(".rotcheck.sha256").exists());
    assert!(run(&["verify", "--quiet", "--", "-odd"]).status.success());
    assert!(run(&["update", "--quiet", "--", "-odd"]).status.success());
    let out = run(&["list", "--", "-odd"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "a.txt\n");
    // An option after `--` is a folder name, not an option.
    assert_eq!(
        run(&["verify", "--", "--only=x"]).status.code(),
        Some(2),
        "no folder named --only=x"
    );
}

#[test]
fn command_help_flag_prints_usage() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rotcheck"))
            .args(args)
            .output()
            .unwrap()
    };
    for args in [
        &["verify", "--help"][..],
        &["update", "-h"],
        &["hash", "--help"],
        &["list", "--only", "x", "--help"],
    ] {
        let out = run(args);
        assert!(out.status.success(), "{args:?}");
        assert!(stdout(&out).starts_with("rotcheck - "), "{args:?}");
    }
    // After `--` it's a file name, not a request for help.
    assert_eq!(run(&["hash", "--", "--help"]).status.code(), Some(1));
    // A mistyped command is still an error, even with --help after it.
    let out = run(&["verfy", "--help"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command 'verfy'"));
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
fn fast_verify_skips_contents() {
    let t = TempDir::new("fast");
    fs::write(t.0.join("notes.txt"), "notes").unwrap();
    fs::write(t.0.join("scan.png"), "scan").unwrap();
    set_mtime(&t.0.join("scan.png"), 1_700_000_000);
    assert!(rotcheck(&["init"], &t.0).status.success());

    fs::write(t.0.join("notes.txt"), "notes, and more").unwrap();
    fs::write(t.0.join("scan.png"), "scaN").unwrap();
    set_mtime(&t.0.join("scan.png"), 1_700_000_000);

    // The rotten scan.png passes: only a full verify reads contents.
    let out = rotcheck(&["verify", "--fast"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.starts_with(
        "MODIFIED    notes.txt\n\
         rotcheck: 1 ok, 0 corrupted, 1 modified, 0 missing, 0 new\n\
         rotcheck: fast check: "
    ));

    let out = rotcheck(&["verify", "--fast", "--quiet"], &t.0);
    assert_eq!(stdout(&out), "MODIFIED    notes.txt\n");
}

#[test]
fn verify_only_checks_one_subfolder() {
    let t = TempDir::new("only");
    fs::create_dir(t.0.join("photos")).unwrap();
    fs::create_dir(t.0.join("music")).unwrap();
    fs::write(t.0.join("photos/a.jpg"), "a").unwrap();
    fs::write(t.0.join("music/b.ogg"), "b").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());
    fs::remove_file(t.0.join("music/b.ogg")).unwrap();

    for only in ["photos", "./photos/", "--only=photos"] {
        let args: &[&str] = if only.starts_with("--") {
            &["verify", only]
        } else {
            &["verify", "--only", only]
        };
        let out = rotcheck(args, &t.0);
        assert_eq!(out.status.code(), Some(0), "{only}");
        assert_eq!(
            stdout(&out),
            "rotcheck: 1 ok, 0 corrupted, 0 modified, 0 missing, 0 new\n"
        );
    }

    let out = rotcheck(&["verify", "--only", "music"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).starts_with("MISSING     music/b.ogg\n"));

    // A typo shouldn't look like a clean run.
    let out = rotcheck(&["verify", "--only", "photo"], &t.0);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn update_only_touches_one_subfolder() {
    let t = TempDir::new("update-only");
    fs::create_dir(t.0.join("photos")).unwrap();
    fs::create_dir(t.0.join("music")).unwrap();
    fs::write(t.0.join("photos/a.jpg"), "a").unwrap();
    fs::write(t.0.join("music/b.ogg"), "b").unwrap();
    assert!(rotcheck(&["init"], &t.0).status.success());
    fs::remove_file(t.0.join("music/b.ogg")).unwrap();
    fs::write(t.0.join("photos/c.jpg"), "c").unwrap();

    let out = rotcheck(&["update", "--only", "photos/"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        "NEW         photos/c.jpg\n\
         rotcheck: manifest updated: 1 added, 0 updated, 0 removed, 1 unchanged\n"
    );
    // The missing music file is still in the manifest.
    let out = rotcheck(&["verify"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).starts_with("MISSING     music/b.ogg\n"));

    // A typo leaves the manifest alone.
    let before = fs::read(t.0.join(".rotcheck.sha256")).unwrap();
    let out = rotcheck(&["update", "--only", "photo"], &t.0);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(t.0.join(".rotcheck.sha256")).unwrap(), before);
}

#[test]
fn list_only_shows_one_subfolder() {
    let t = TempDir::new("list-only");
    fs::create_dir(t.0.join("photos")).unwrap();
    fs::create_dir(t.0.join("photos2")).unwrap();
    fs::write(t.0.join("photos/a.jpg"), "a").unwrap();
    fs::write(t.0.join("photos2/b.jpg"), "b").unwrap();
    fs::write(t.0.join("top.txt"), "t").unwrap();

    let out = rotcheck(&["list", "--only=./photos/"], &t.0);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "photos/a.jpg\n");

    let out = rotcheck(&["list", "--only", "top.txt"], &t.0);
    assert_eq!(stdout(&out), "top.txt\n");

    let out = rotcheck(&["list", "--only", "photo"], &t.0);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");

    let out = rotcheck(&["list", "--bogus"], &t.0);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn dir_must_be_an_existing_folder() {
    let t = TempDir::new("dir-not-folder");
    let file = t.0.join("photo.jpg");
    fs::write(&file, "p").unwrap();
    let missing = t.0.join("nope");
    for cmd in ["list", "init", "verify", "update"] {
        let out = rotcheck(&[cmd], &file);
        assert_eq!(out.status.code(), Some(2), "{cmd} on a file");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("is a file, not a folder"), "{cmd}: {err}");

        let out = rotcheck(&[cmd], &missing);
        assert_eq!(out.status.code(), Some(2), "{cmd} on a missing folder");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("no such folder"), "{cmd}: {err}");
    }
    // Nothing was created by the failed `init`.
    assert!(!missing.exists());
}

#[test]
fn only_rejects_paths_outside_dir() {
    let t = TempDir::new("only-outside");
    fs::create_dir(t.0.join("photos")).unwrap();
    fs::write(t.0.join("photos/a.jpg"), "a").unwrap();
    assert_eq!(rotcheck(&["init"], &t.0).status.code(), Some(0));
    let manifest = fs::read(t.0.join(".rotcheck.sha256")).unwrap();

    let outside = t.0.parent().unwrap().display().to_string();
    for cmd in ["list", "verify", "update"] {
        for bad in ["", outside.as_str(), "/", "../photos", "photos/../photos"] {
            let out = rotcheck(&[cmd, "--only", bad], &t.0);
            assert_eq!(out.status.code(), Some(2), "{cmd} --only '{bad}'");
            assert_eq!(stdout(&out), "", "{cmd} --only '{bad}'");
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(err.contains("--only"), "{cmd} --only '{bad}': {err}");
        }
    }
    // A name that merely starts with dots is fine.
    fs::write(t.0.join("..notes"), "n").unwrap();
    let out = rotcheck(&["list", "--only", "..notes"], &t.0);
    assert_eq!(stdout(&out), "..notes\n");
    assert_eq!(fs::read(t.0.join(".rotcheck.sha256")).unwrap(), manifest);
}

#[test]
fn only_accepts_absolute_path_inside_dir() {
    let t = TempDir::new("only-absolute");
    fs::create_dir_all(t.0.join("dir/photos")).unwrap();
    fs::write(t.0.join("dir/photos/a.jpg"), "a").unwrap();
    fs::write(t.0.join("dir/b.txt"), "b").unwrap();
    let dir = t.0.join("dir");
    assert_eq!(rotcheck(&["init"], &dir).status.code(), Some(0));

    let abs = dir.join("photos/").display().to_string();
    let out = rotcheck(&["list", "--only", &abs], &dir);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "photos/a.jpg\n");

    let out = rotcheck(&["verify", "--only", &abs], &dir);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("1 ok, 0 corrupted"),
        "{}",
        stdout(&out)
    );

    // A file that's only in the manifest (no longer on disk) works too.
    fs::remove_file(t.0.join("dir/photos/a.jpg")).unwrap();
    let out = rotcheck(&["verify", "--only", &abs], &dir);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("photos/a.jpg"), "{}", stdout(&out));
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

    // -q is the same as --quiet.
    let out = rotcheck(&["verify", "-q"], &t.0);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "MISSING     b.txt\n");

    // Commands without --quiet don't take -q either.
    let out = rotcheck(&["list", "-q"], &t.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown option '-q'"));
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

    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_rotcheck"))
            .arg(flag)
            .output()
            .unwrap();
        assert_eq!(stdout(&out), format!("rotcheck {version}\n"));
    }
}

#[test]
fn man_page_matches_version_and_flags() {
    // The man page ships in the .deb/.rpm, so a version bump or a new flag
    // has to reach it too.
    let man = include_str!("../man/rotcheck.1");
    let th = man.lines().find(|l| l.starts_with(".TH ")).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        th.contains(&format!("\"rotcheck {version}\"")),
        "man/rotcheck.1 .TH line doesn't say rotcheck {version}: {th}"
    );

    let help = Command::new(env!("CARGO_BIN_EXE_rotcheck"))
        .arg("help")
        .output()
        .unwrap();
    // Long flags (`--quiet`) and short ones (`-q,`); a lone `-` is stdin.
    let text = stdout(&help);
    let flags: Vec<&str> = text
        .split_whitespace()
        .map(|w| w.trim_end_matches(','))
        .filter(|w| w.starts_with('-') && w.len() > 1)
        .collect();
    for short in ["-q", "-h", "-V"] {
        assert!(flags.contains(&short), "help text doesn't mention {short}");
    }
    for flag in flags {
        let roff = flag.replace('-', "\\-");
        // Not followed by more of a word, so `\-h` isn't found in `\-hash`
        // or `\-\-accept` in `\-\-accept\-corrupted`.
        let found = man.match_indices(&roff).any(|(i, _)| {
            !man[i + roff.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '\\')
        });
        assert!(found, "man/rotcheck.1 doesn't mention {flag}");
    }
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
