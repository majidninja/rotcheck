mod ignore;
mod manifest;
mod progress;
mod sha256;
mod update;
mod verify;
mod walk;

use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
rotcheck - detect missing, changed and silently corrupted files

USAGE:
    rotcheck hash <FILE>...    Print SHA-256 checksums (sha256sum format); - is stdin
    rotcheck list [DIR]        List the files a manifest of DIR would cover
        --only <PATH>          List only this file or subfolder (relative to DIR)
    rotcheck init [DIR]        Record checksums for DIR in DIR/.rotcheck.sha256
    rotcheck verify [DIR]      Check DIR against its manifest
        --strict               Also fail (exit 1) when there are new files
        --quiet                Print only the files that make it fail, no summary
        --fast                 Compare only size and mtime; don't read contents
        --only <PATH>          Check only this file or subfolder (relative to DIR)
    rotcheck update [DIR]      Accept added, edited and removed files into the manifest
        --accept-corrupted     Also re-record files that look corrupted
        --quiet                Print only the files that make it fail, no summary
        --only <PATH>          Update only this file or subfolder; keep the rest as is
    rotcheck help              Show this message; so does COMMAND --help
    rotcheck --version         Show version

Files and folders matched by DIR/.rotcheckignore (one pattern per line,
`*` within a name, trailing `/` for folders) are left out.
";

/// Let a closed output pipe end the program quietly, as it does for `ls`
/// or `sha256sum`. Rust ignores SIGPIPE by default, so `rotcheck list | head`
/// would otherwise panic with "failed printing to stdout: Broken pipe".
#[cfg(unix)]
fn default_sigpipe() {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13; // The same number on Linux, macOS and the BSDs.
    const SIG_DFL: usize = 0;
    // SAFETY: called once at startup, before any other threads exist.
    unsafe {
        signal(SIGPIPE, SIG_DFL);
    }
}

#[cfg(not(unix))]
fn default_sigpipe() {}

fn main() -> ExitCode {
    default_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let known = matches!(
        args.first().map(String::as_str),
        Some("hash" | "list" | "init" | "verify" | "update")
    );
    if known && asks_for_help(&args[1..]) {
        // `rotcheck verify --help` shows the usage instead of failing with
        // "unknown option". `rotcheck typo --help` still reports the typo.
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("hash") => cmd_hash(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
        Some("init") => cmd_init(&args[1..]),
        Some("verify") => cmd_verify(&args[1..]),
        Some("update") => cmd_update(&args[1..]),
        Some("help" | "-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("rotcheck {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("rotcheck: unknown command '{other}'\n\n{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// True if a command's arguments contain `-h` or `--help` before any `--`.
fn asks_for_help(args: &[String]) -> bool {
    args.iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "-h" || a == "--help")
}

fn hash_file<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let file = File::open(path)?;
    Ok(sha256::to_hex(&sha256::hash_reader(BufReader::new(file))?))
}

fn cmd_hash(paths: &[String]) -> ExitCode {
    // `hash` has no options. Anything that looks like one (other than `-`
    // itself) is a usage error rather than "no such file", unless it comes
    // after `--`, which ends options as usual.
    let mut files: Vec<&String> = Vec::new();
    let mut options_done = false;
    for arg in paths {
        if !options_done && arg == "--" {
            options_done = true;
        } else if !options_done && arg.starts_with('-') && arg != "-" {
            eprintln!("rotcheck hash: unknown option {arg} (use ./{arg} or -- {arg} for a file)");
            return ExitCode::from(2);
        } else {
            files.push(arg);
        }
    }
    if files.is_empty() {
        eprintln!("rotcheck hash: no files given");
        return ExitCode::from(2);
    }
    let mut ok = true;
    for path in files {
        // `-` means standard input, as with `sha256sum` (`./-` is the file).
        let result = if path == "-" {
            sha256::hash_reader(io::stdin().lock()).map(|d| sha256::to_hex(&d))
        } else {
            hash_file(path)
        };
        match result {
            // Two spaces: the same layout `sha256sum` prints and reads.
            Ok(hex) => println!("{}", manifest::checksum_line(&hex, path)),
            Err(e) => {
                eprintln!("rotcheck: {path}: {e}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The optional single DIR argument shared by several commands. `None`
/// (already printed) if there's more than one, or if DIR isn't a folder:
/// otherwise `verify photo.jpg` would say "no manifest at
/// photo.jpg/.rotcheck.sha256", which hides the real mistake.
fn dir_arg<'a>(cmd: &str, args: &'a [String]) -> Option<&'a str> {
    let dir = match args {
        [] => ".",
        [dir] => dir.as_str(),
        _ => {
            eprintln!("rotcheck {cmd}: expected at most one directory");
            return None;
        }
    };
    match std::fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => Some(dir),
        Ok(_) => {
            eprintln!("rotcheck {cmd}: {dir} is a file, not a folder");
            None
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("rotcheck {cmd}: no such folder: {dir}");
            None
        }
        Err(e) => {
            eprintln!("rotcheck {cmd}: {dir}: {e}");
            None
        }
    }
}

fn cmd_list(args: &[String]) -> ExitCode {
    let mut args = args.to_vec();
    let Ok(only) = take_only("list", &mut args) else {
        return ExitCode::from(2);
    };
    let Some(([], rest)) = split_flags("list", &args, []) else {
        return ExitCode::from(2);
    };
    let Some(dir) = dir_arg("list", &rest) else {
        return ExitCode::from(2);
    };
    let Ok(scope) = only_scope("list", only.as_deref(), dir) else {
        return ExitCode::from(2);
    };
    match walk::walk(Path::new(dir)) {
        Ok(files) => {
            let mut any = false;
            for f in files.iter().filter(|f| verify::in_scope(f, &scope)) {
                any = true;
                println!("{}", manifest::display_path(f));
            }
            if !any && !scope.is_empty() {
                eprintln!("rotcheck list: no files under '{scope}' in {dir}");
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("rotcheck: {dir}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_init(args: &[String]) -> ExitCode {
    let Some(([], rest)) = split_flags("init", args, []) else {
        return ExitCode::from(2);
    };
    let Some(dir) = dir_arg("init", &rest) else {
        return ExitCode::from(2);
    };
    let root = Path::new(dir);
    let manifest_path = root.join(walk::MANIFEST_NAME);
    if manifest_path.exists() {
        eprintln!(
            "rotcheck: {} already exists; refusing to overwrite it",
            manifest_path.display()
        );
        return ExitCode::FAILURE;
    }
    match build_entries(root).and_then(|entries| {
        manifest::save(&entries, &manifest_path)?;
        Ok(entries.len())
    }) {
        Ok(n) => {
            println!(
                "rotcheck: recorded {n} files in {}",
                manifest_path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("rotcheck: {dir}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Splits `--flags` from positional arguments. Returns which of `known` were
/// given (in the same order), or prints an error and returns `None` for an
/// unknown flag.
fn split_flags<const N: usize>(
    cmd: &str,
    args: &[String],
    known: [&str; N],
) -> Option<([bool; N], Vec<String>)> {
    let mut set = [false; N];
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            // Everything after `--` is a folder name, even if it starts
            // with a dash.
            rest.extend(iter.cloned());
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            rest.push(arg.clone());
            continue;
        }
        match known.iter().position(|k| k == arg) {
            Some(i) => set[i] = true,
            None => {
                eprintln!("rotcheck {cmd}: unknown option '{arg}'");
                return None;
            }
        }
    }
    Some((set, rest))
}

/// Takes `--NAME VALUE` or `--NAME=VALUE` out of `args`. `Err` (already
/// printed) if the value is missing.
fn take_value(cmd: &str, args: &mut Vec<String>, name: &str) -> Result<Option<String>, ()> {
    let prefix = format!("{name}=");
    // Anything after `--` is a folder name, not an option.
    let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let Some(i) = args[..end]
        .iter()
        .position(|a| a == name || a.starts_with(&prefix))
    else {
        return Ok(None);
    };
    let arg = args.remove(i);
    if let Some(value) = arg.strip_prefix(&prefix) {
        return Ok(Some(value.to_string()));
    }
    if i < args.len() {
        Ok(Some(args.remove(i)))
    } else {
        eprintln!("rotcheck {cmd}: {name} needs a path");
        Err(())
    }
}

/// Takes `--only PATH` out of `args`. `Err` (already printed) if the value is
/// missing or empty; an empty one would otherwise silently check everything.
fn take_only(cmd: &str, args: &mut Vec<String>) -> Result<Option<String>, ()> {
    let only = take_value(cmd, args, "--only")?;
    if only.as_deref() == Some("") {
        eprintln!("rotcheck {cmd}: --only needs a path (got '')");
        return Err(());
    }
    Ok(only)
}

/// Turns `--only PATH` into a scope relative to `dir` (empty: everything).
/// An absolute PATH is accepted if it lies inside `dir`. `Err` (already
/// printed) for a path that can't be inside `dir`: absolute elsewhere, or
/// containing `..`. Those would otherwise only fail with a puzzling
/// "nothing under ...".
fn only_scope(cmd: &str, only: Option<&str>, dir: &str) -> Result<String, ()> {
    let Some(only) = only else {
        return Ok(String::new());
    };
    let relative = if only.starts_with('/') || Path::new(only).is_absolute() {
        strip_dir(only, dir)
    } else {
        Some(only.to_string())
    };
    let problem = match relative {
        None => format!("takes a path inside DIR ('{dir}')"),
        Some(rel) => {
            let scope = verify::normalize_scope(&rel);
            if !scope.split('/').any(|part| part == "..") {
                return Ok(scope);
            }
            "takes a path inside DIR; '..' isn't allowed".to_string()
        }
    };
    eprintln!("rotcheck {cmd}: --only {problem} (got '{only}')");
    Err(())
}

/// The part of the absolute path `only` below `dir`, or `None` if it isn't
/// inside `dir`. Compares both as written and with symlinks resolved, so
/// `--only "$PWD/photos"` works whichever way the user got there.
fn strip_dir(only: &str, dir: &str) -> Option<String> {
    let only_forms = [Some(PathBuf::from(only)), std::fs::canonicalize(only).ok()];
    let dir_forms = [
        std::path::absolute(dir).ok(),
        std::fs::canonicalize(dir).ok(),
    ];
    for o in only_forms.iter().flatten() {
        for d in dir_forms.iter().flatten() {
            if let Ok(rest) = o.strip_prefix(d) {
                // Manifest paths always use '/'.
                let parts: Vec<_> = rest
                    .components()
                    .map(|c| c.as_os_str().to_str())
                    .collect::<Option<_>>()?;
                return Some(parts.join("/"));
            }
        }
    }
    None
}

fn cmd_verify(args: &[String]) -> ExitCode {
    let mut args = args.to_vec();
    let Ok(only) = take_only("verify", &mut args) else {
        return ExitCode::from(2);
    };
    let Some(([strict, quiet, fast], rest)) =
        split_flags("verify", &args, ["--strict", "--quiet", "--fast"])
    else {
        return ExitCode::from(2);
    };
    let Some(dir) = dir_arg("verify", &rest) else {
        return ExitCode::from(2);
    };
    let Ok(scope) = only_scope("verify", only.as_deref(), dir) else {
        return ExitCode::from(2);
    };
    let root = Path::new(dir);
    let manifest_path = root.join(walk::MANIFEST_NAME);
    let Some(entries) = load_manifest(dir, &manifest_path) else {
        return ExitCode::from(2);
    };
    let ignore = ignore::Ignore::load(root).unwrap_or_default();
    let mut now_ignored = 0;
    let progress = progress::Progress::new(!quiet, None);
    let mut report = |path: &str, status| {
        progress.tick();
        if status == verify::Status::Missing && ignore.covers(path) {
            now_ignored += 1;
        }
        let show = match status {
            verify::Status::Ok => false,
            // With --quiet only lines that affect the exit code are shown,
            // so a silent run is a clean run.
            verify::Status::New => strict || !quiet,
            _ => true,
        };
        if show {
            progress.clear();
            println!("{status:<10}  {}", manifest::display_path(path));
        }
    };
    let total = |n| progress.set_total(n);
    let result = if fast {
        verify::verify_fast(root, &entries, &scope, total, &mut report)
    } else {
        verify::verify(root, &entries, &scope, total, &mut report)
    };
    progress.clear();
    match result {
        Ok(s) if !scope.is_empty() && s == verify::Summary::default() => {
            // Most likely a typo; passing silently would hide that.
            eprintln!(
                "rotcheck verify: nothing in the manifest or on disk under '{}'",
                only.unwrap_or_default()
            );
            ExitCode::from(2)
        }
        Ok(s) => {
            let failed = s.has_problems() || (strict && s.new > 0);
            if quiet {
                return if failed {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                };
            }
            let mut line = format!(
                "rotcheck: {} ok, {} corrupted, {} modified, {} missing, {} new",
                s.ok, s.corrupted, s.modified, s.missing, s.new
            );
            if s.unreadable > 0 {
                line.push_str(&format!(", {} unreadable", s.unreadable));
            }
            println!("{line}");
            if fast {
                println!(
                    "rotcheck: fast check: only sizes and times were compared, \
                     not contents; run a full `rotcheck verify {dir}` to find corruption"
                );
            }
            if now_ignored > 0 {
                println!(
                    "rotcheck: {now_ignored} of the missing files match {}; \
                     run `rotcheck update {dir}` to drop them",
                    ignore::IGNORE_NAME
                );
            }
            if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("rotcheck: {dir}: {e}");
            ExitCode::from(2)
        }
    }
}

/// Reads the manifest, or prints why it couldn't and returns `None`.
fn load_manifest(dir: &str, manifest_path: &Path) -> Option<Vec<manifest::Entry>> {
    match std::fs::read_to_string(manifest_path).and_then(|t| manifest::parse(&t)) {
        Ok(entries) => Some(entries),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "rotcheck: no manifest at {}; run `rotcheck init {dir}` first",
                manifest_path.display()
            );
            None
        }
        Err(e) => {
            eprintln!("rotcheck: {}: {e}", manifest_path.display());
            None
        }
    }
}

fn cmd_update(args: &[String]) -> ExitCode {
    let mut args = args.to_vec();
    let Ok(only) = take_only("update", &mut args) else {
        return ExitCode::from(2);
    };
    let Some(([accept_corrupted, quiet], rest)) =
        split_flags("update", &args, ["--accept-corrupted", "--quiet"])
    else {
        return ExitCode::from(2);
    };
    let Some(dir) = dir_arg("update", &rest) else {
        return ExitCode::from(2);
    };
    let Ok(scope) = only_scope("update", only.as_deref(), dir) else {
        return ExitCode::from(2);
    };
    let root = Path::new(dir);
    let manifest_path = root.join(walk::MANIFEST_NAME);
    let Some(old) = load_manifest(dir, &manifest_path) else {
        return ExitCode::from(2);
    };
    let ignore = ignore::Ignore::load(root).unwrap_or_default();
    let mut now_ignored = 0;
    let progress = progress::Progress::new(!quiet, None);
    let result = update::update(
        root,
        &old,
        &scope,
        accept_corrupted,
        |n| progress.set_total(n),
        |path, status| {
            progress.tick();
            if status == verify::Status::Missing && ignore.covers(path) {
                now_ignored += 1;
            }
            let show = match status {
                verify::Status::Ok => false,
                // Like `verify --quiet`: only lines that affect the exit code.
                verify::Status::Corrupted => !(quiet && accept_corrupted),
                verify::Status::Unreadable => true,
                _ => !quiet,
            };
            if show {
                progress.clear();
                println!("{status:<10}  {}", manifest::display_path(path));
            }
        },
    );
    progress.clear();
    if let Ok((_, s)) = &result {
        if !scope.is_empty() && *s == verify::Summary::default() {
            // Most likely a typo; don't rewrite the manifest for nothing.
            eprintln!(
                "rotcheck update: nothing in the manifest or on disk under '{}'",
                only.unwrap_or_default()
            );
            return ExitCode::from(2);
        }
    }
    let result = result.and_then(|(entries, summary)| {
        manifest::save(&entries, &manifest_path)?;
        Ok(summary)
    });
    let s = match result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rotcheck: {dir}: {e}");
            return ExitCode::from(2);
        }
    };
    let mut line = format!(
        "rotcheck: manifest updated: {} added, {} updated, {} removed, {} unchanged",
        s.new, s.modified, s.missing, s.ok
    );
    if now_ignored > 0 {
        line.push_str(&format!(" ({now_ignored} of the removed are now ignored)"));
    }
    if s.corrupted > 0 {
        let what = if accept_corrupted { "accepted" } else { "kept" };
        line.push_str(&format!(", {} corrupted ({what})", s.corrupted));
    }
    if s.unreadable > 0 {
        line.push_str(&format!(", {} unreadable", s.unreadable));
    }
    if !quiet {
        println!("{line}");
    }
    let refused = s.corrupted > 0 && !accept_corrupted;
    if refused {
        eprintln!(
            "rotcheck: corrupted files keep their old checksum, so `verify` still flags them"
        );
        eprintln!(
            "rotcheck: restore them from a backup, or run `rotcheck update --accept-corrupted {dir}`"
        );
    }
    if refused || s.unreadable > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Hashes every file under `root`.
fn build_entries(root: &Path) -> io::Result<Vec<manifest::Entry>> {
    let files = walk::walk(root)?;
    let progress = progress::Progress::new(true, Some(files.len()));
    let mut entries = Vec::new();
    for rel in files {
        progress.tick();
        let entry = verify::current_entry(root, &rel)
            .map_err(|e| io::Error::new(e.kind(), format!("{rel}: {e}")))?;
        entries.push(entry);
    }
    Ok(entries)
}
