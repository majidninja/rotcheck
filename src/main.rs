mod manifest;
mod sha256;
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
    rotcheck help             Show this message
    rotcheck --version         Show version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hash") => cmd_hash(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
        Some("init") => cmd_init(&args[1..]),
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
            Ok(hex) => println!("{hex}  {path}"),
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
                println!("{f}");
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

/// Hashes every file under `root`. Metadata is read before hashing so a file
/// written to mid-hash looks modified (not corrupted) on the next verify.
fn build_entries(root: &Path) -> io::Result<Vec<manifest::Entry>> {
    let mut entries = Vec::new();
    for rel in walk::walk(root)? {
        let full = root.join(&rel);
        let with_path = |e: io::Error| io::Error::new(e.kind(), format!("{rel}: {e}"));
        let (size, mtime) = manifest::file_meta(&full).map_err(with_path)?;
        let hash = hash_file(&full).map_err(with_path)?;
        entries.push(manifest::Entry {
            path: rel,
            hash,
            size,
            mtime,
        });
    }
    Ok(entries)
}
