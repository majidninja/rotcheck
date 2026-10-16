mod manifest;
mod sha256;
mod update;
mod verify;
mod walk;

use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
rotcheck - detect missing, changed and silently corrupted files

USAGE:
    rotcheck hash <FILE>...    Print SHA-256 checksums (sha256sum format)
    rotcheck list [DIR]        List the files a manifest of DIR would cover
    rotcheck init [DIR]        Record checksums for DIR in DIR/.rotcheck.sha256
    rotcheck verify [DIR]      Check DIR against its manifest
        --strict               Also fail (exit 1) when there are new files
    rotcheck update [DIR]      Accept added, edited and removed files into the manifest
        --accept-corrupted     Also re-record files that look corrupted
    rotcheck help              Show this message
    rotcheck --version         Show version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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

fn hash_file<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let file = File::open(path)?;
    Ok(sha256::to_hex(&sha256::hash_reader(BufReader::new(file))?))
}

fn cmd_hash(paths: &[String]) -> ExitCode {
    if paths.is_empty() {
        eprintln!("rotcheck hash: no files given");
        return ExitCode::from(2);
    }
    let mut ok = true;
    for path in paths {
        match hash_file(path) {
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

/// The optional single DIR argument shared by several commands.
fn dir_arg<'a>(cmd: &str, args: &'a [String]) -> Option<&'a str> {
    match args {
        [] => Some("."),
        [dir] => Some(dir.as_str()),
        _ => {
            eprintln!("rotcheck {cmd}: expected at most one directory");
            None
        }
    }
}

fn cmd_list(args: &[String]) -> ExitCode {
    let Some(dir) = dir_arg("list", args) else {
        return ExitCode::from(2);
    };
    match walk::walk(Path::new(dir)) {
        Ok(files) => {
            for f in files {
                println!("{}", manifest::display_path(&f));
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
    let Some(dir) = dir_arg("init", args) else {
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
    for arg in args {
        if !arg.starts_with("--") {
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

fn cmd_verify(args: &[String]) -> ExitCode {
    let Some(([strict], rest)) = split_flags("verify", args, ["--strict"]) else {
        return ExitCode::from(2);
    };
    let Some(dir) = dir_arg("verify", &rest) else {
        return ExitCode::from(2);
    };
    let root = Path::new(dir);
    let manifest_path = root.join(walk::MANIFEST_NAME);
    let Some(entries) = load_manifest(dir, &manifest_path) else {
        return ExitCode::from(2);
    };
    let result = verify::verify(root, &entries, |path, status| {
        if status != verify::Status::Ok {
            println!("{status:<10}  {}", manifest::display_path(path));
        }
    });
    match result {
        Ok(s) => {
            let mut line = format!(
                "rotcheck: {} ok, {} corrupted, {} modified, {} missing, {} new",
                s.ok, s.corrupted, s.modified, s.missing, s.new
            );
            if s.unreadable > 0 {
                line.push_str(&format!(", {} unreadable", s.unreadable));
            }
            println!("{line}");
            if s.has_problems() || (strict && s.new > 0) {
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
    let Some(([accept_corrupted], rest)) = split_flags("update", args, ["--accept-corrupted"])
    else {
        return ExitCode::from(2);
    };
    let Some(dir) = dir_arg("update", &rest) else {
        return ExitCode::from(2);
    };
    let root = Path::new(dir);
    let manifest_path = root.join(walk::MANIFEST_NAME);
    let Some(old) = load_manifest(dir, &manifest_path) else {
        return ExitCode::from(2);
    };
    let result = update::update(root, &old, accept_corrupted, |path, status| {
        if status != verify::Status::Ok {
            println!("{status:<10}  {}", manifest::display_path(path));
        }
    })
    .and_then(|(entries, summary)| {
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
    if s.corrupted > 0 {
        let what = if accept_corrupted { "accepted" } else { "kept" };
        line.push_str(&format!(", {} corrupted ({what})", s.corrupted));
    }
    if s.unreadable > 0 {
        line.push_str(&format!(", {} unreadable", s.unreadable));
    }
    println!("{line}");
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
    let mut entries = Vec::new();
    for rel in walk::walk(root)? {
        let entry = verify::current_entry(root, &rel)
            .map_err(|e| io::Error::new(e.kind(), format!("{rel}: {e}")))?;
        entries.push(entry);
    }
    Ok(entries)
}
