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
    rotcheck help             Show this message
    rotcheck --version         Show version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hash") => cmd_hash(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
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

fn hash_file(path: &str) -> io::Result<String> {
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

fn cmd_list(args: &[String]) -> ExitCode {
    let dir = match args {
        [] => ".",
        [dir] => dir.as_str(),
        _ => {
            eprintln!("rotcheck list: expected at most one directory");
            return ExitCode::from(2);
        }
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
