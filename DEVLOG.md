# rotcheck dev log

## Idea

rotcheck is an offline CLI that records SHA-256 checksums for a directory tree
and later reports which files are missing, new, modified (mtime changed) or
corrupted (content changed but mtime and size didn't, which points to bit rot).

Design rules:
- Zero crate dependencies (std only). SHA-256 is implemented in-tree.
- Manifest stays readable by `sha256sum -c`: lines like `<hex>  <relative path>`.
  Extra metadata (size, mtime) goes in `#` comment lines or a sidecar, so the
  file still works with standard tools.
- Exit codes are script-friendly: 0 = all good, 1 = problems found, 2 = usage error.

## Roadmap

1. [x] SHA-256 + `rotcheck hash <files>` (sha256sum-compatible output)
2. [x] Recursive directory walk (sorted, skip the manifest itself, don't follow symlinks)
3. [x] Manifest format + `rotcheck init <dir>` writes `<dir>/.rotcheck.sha256`
4. [x] `rotcheck verify <dir>`: missing / new / modified / corrupted report
5. [x] `rotcheck update <dir>`: accept changes
6. [x] Paths with newlines/backslashes (sha256sum's `\` escaping convention)
7. [ ] Progress output for big trees, `--quiet`, `--json`?
8. [ ] Integration tests with temp dirs; release builds / install docs

## 2026-10-09

- Picked the project and ran `cargo init` (crate `rotcheck`).
- Wrote a std-only SHA-256 (`src/sha256.rs`). Tests cover the NIST vectors
  (empty, "abc", 448-bit message, one million 'a') and incremental-update splits.
- Added `rotcheck hash <FILE>...`. Its output matches `sha256sum` byte for byte.
- Wrote the README and this roadmap.

**Next:** roadmap item 2: a recursive directory walker that returns sorted
relative paths with forward slashes, skips symlinks and the manifest file, and
has tests using a temp dir under `std::env::temp_dir()`.

## 2026-10-10

- Added `src/walk.rs`: recursive walk returning sorted, `/`-separated relative
  paths. Skips symlinks (files and dirs), special files, and `.rotcheck.sha256`
  at the top level only. Non-UTF-8 names are a hard error for now (revisit with
  roadmap item 6).
- Tests use a small self-cleaning `TempDir` helper under `std::env::temp_dir()`.
- Added `rotcheck list [DIR]` to preview what a manifest would cover; README updated.

**Next:** roadmap item 3: `rotcheck init <dir>` writing `<dir>/.rotcheck.sha256`.
Decide where size/mtime go. `#` comment lines (e.g. `# size=123 mtime=1700000000`)
are the idea, but first confirm that `sha256sum -c` really ignores them; if it
doesn't, use a sidecar file. Write the manifest atomically (temp file + rename).

## 2026-10-11

- Checked it: GNU `sha256sum -c --strict` skips `#` lines, so metadata goes in
  comments and no sidecar file is needed.
- Added `src/manifest.rs`. The format is a `# rotcheck manifest v1` header,
  then a `# size=N mtime=SECS` line before each `<hex>  <path>` line. The
  manifest is written atomically (`.tmp` + fsync + rename). `parse` also
  accepts plain `sha256sum` output (`*` binary marker too); those entries get
  size 0 and mtime 0. Paths with `\n`, `\r` or `\` are rejected until item 6.
- Added `rotcheck init [DIR]`. It refuses to overwrite an existing manifest.
  Metadata is read before hashing, so a file written during the hash shows
  up as "modified", not "corrupted".
- Roadmap item 3 done.

**Next:** roadmap item 4: `rotcheck verify [DIR]`. `manifest::parse` is ready
(it has `#[allow(dead_code)]` for now; remove that). Classify each entry:
missing, new (on disk but not in the manifest), modified (hash differs and
size or mtime differs), corrupted (hash differs, size and mtime the same).
Exit with 1 if anything is missing or corrupted. Decide whether "modified" or
"new" should also give exit 1, or need a flag. Possibly also skip a leftover
`.rotcheck.sha256.tmp` in the walker.

## 2026-10-12

- Added `src/verify.rs` and `rotcheck verify [DIR]`. Every manifest entry is
  re-hashed and classified as ok / missing / modified / corrupted /
  unreadable; files on disk that the manifest doesn't list are "new". Same
  hash with a different mtime counts as ok.
- Decisions: exit 1 for corrupted, modified, missing or unreadable. New files
  alone give exit 0 (a live folder gains files all the time). Exit 2 when the
  check can't run at all (no manifest, bad manifest, unreadable folder).
  Entries from a plain `sha256sum` list have no metadata, so a mismatch there
  is always "modified".
- Output: one `STATUS  path` line per non-ok file on stdout, manifest entries
  first, then new files, then a one-line summary.
- The walker now also skips a leftover `.rotcheck.sha256.tmp` at the top level.
- Started roadmap item 8: `tests/cli.rs` runs the real binary on temp dirs
  (5 tests). It uses `File::set_modified` to fake the "same mtime, different
  bytes" case, so it needs Rust 1.75+.
- Not done by hand: the sandbox blocked running the binary from a shell this
  session, so the end-to-end checks are the `tests/cli.rs` ones only.

**Next:** roadmap item 5: `rotcheck update [DIR]`. Rewrite the manifest to
match the disk: add new files, drop missing ones, re-record modified ones.
Speed-up idea: reuse the old hash when size and mtime are unchanged, but that
would silently skip corrupted files, so by default re-hash everything and
*refuse* to accept a corrupted file unless a flag (`--accept-corrupted`?) is
given. Print what changed, reusing `verify::Status`. Small follow-ups: a
`--strict` flag for `verify` (new files give exit 1), and rejecting manifest
paths that are absolute or contain `..` so a hand-edited manifest can't make
rotcheck read outside the folder.

## 2026-10-13

- Added `src/update.rs` and `rotcheck update [--accept-corrupted] [DIR]`.
  It re-hashes everything, then rewrites the manifest (atomically, sorted by
  path): new files added, missing dropped, modified re-recorded. Unchanged
  files get their size/mtime refreshed, which also fills in metadata for
  entries that came from a plain `sha256sum` list.
- Corrupted files keep their old entry unless `--accept-corrupted` is given,
  so `verify` keeps flagging them. Unreadable files keep their old entry too.
  Exit 1 if either kind was left behind, 0 otherwise, 2 if it couldn't run.
- Refactor: `verify::scan` is the shared loop. It hands the caller the
  freshly hashed `Entry` so `update` doesn't read files twice.
  `verify::current_entry` replaces the duplicated meta-then-hash code in
  `init` and `verify`. `load_manifest` in `main.rs` is shared by both commands.
- Tests: 3 unit tests in `update.rs`, 2 more in `tests/cli.rs` (19 unit + 7
  CLI now). As before, the binary was only exercised through `tests/cli.rs`,
  not by hand.
- Not done: no dry-run for `update`; the README tells people to run `verify`
  first. An unreadable *new* file is covered by code but not by a test.

**Next:** harden manifest parsing (small, do first): reject paths that are
absolute or contain `..` / empty components, with a test. They can't be read
today because `scan` only opens paths the walker returned, but a clear error
beats a confusing MISSING. Then `verify --strict` (new files give exit 1).
After that roadmap item 6: `sha256sum`'s backslash escaping (line starts with
`\`, and `\\`, `\n`, `\r` in the name) so odd file names stop blocking `init`.

## 2026-10-14

- Manifest parsing is stricter now: absolute paths and paths with empty, `.`
  or `..` parts are a parse error that names the line (exit 2), not a
  confusing MISSING. A leading `./` is stripped, so a list made with
  `find . -type f -exec sha256sum {} +` works as a manifest.
- Added `verify --strict`: new files also give exit 1.
- Refactor: `split_flags` in `main.rs` handles `--flags` for both `verify`
  and `update` (unknown flag gives exit 2).
- Tests: 2 unit tests in `manifest.rs`, 2 CLI tests (21 unit + 9 CLI).
  README updated (exit codes, `./` and `..` rules).

**Next:** roadmap item 6, `sha256sum`'s backslash escaping. When a name
contains `\`, `\n` or `\r`, sha256sum writes the line with a leading `\` and
escapes them as `\\`, `\n`, `\r`. Do it in `manifest::write_to` and `parse`,
remove `check_path`, and test a round trip plus parsing real
`sha256sum` output for such a name. Watch out: `clean_path` must run on the
*unescaped* path.

## 2026-10-15

- Roadmap item 6 done: the manifest uses `sha256sum`'s escaping. If a name
  contains `\`, `\n` or `\r`, the checksum line starts with `\` and the name
  is written with `\\`, `\n`, `\r`. `parse` reverses it, and `clean_path`
  runs on the *unescaped* name, so `..` can't be hidden behind an escape.
  Any other escape (or a trailing lone `\`) is a parse error. Without the
  leading `\`, a backslash in the name is taken literally, as sha256sum does.
  `check_path` is gone; `init` no longer refuses such names.
- Tests: 4 unit tests in `manifest.rs` (round trip, sha256sum-style lines
  incl. `*` binary marker, literal backslash, bad escapes) and 1 unix-only
  CLI test with real files named `back\slash.txt` and `new<LF>line.txt`
  (24 unit + 10 CLI). README updated.
- Not checked: the sandbox wouldn't let me create odd-named files from the
  shell, so I couldn't compare against real GNU `sha256sum` output. The test
  strings follow the documented coreutils format. Worth checking by hand:
  `sha256sum -c .rotcheck.sha256` in a folder with such a name.

**Next:** rotcheck's own output still prints names raw: `verify`/`update`
status lines, `list` and `hash`. A name with a newline would split a report
line. Use the same escaping there (make `escape_path` `pub`; for `hash`,
match sha256sum exactly with the leading `\`). Then roadmap item 7
(`--quiet`, progress for big trees).

## 2026-10-16

- rotcheck's output now escapes odd names too, so one file is always one
  line. New in `manifest.rs`: `display_path` (escapes `\`, `\n`, `\r` only
  when the name needs it) and `checksum_line` (sha256sum line with the
  leading `\` marker). `write_to` uses `checksum_line`. `hash` prints
  through `checksum_line`, so it matches `sha256sum` for such names.
  `list` and the `verify`/`update` status lines use `display_path` (no
  leading `\` marker there; since every `\` is escaped, the name can still
  be recovered).
- Tests: 1 unit test, and the unix-only CLI odd-names test now also covers
  the verify report (MODIFIED/NEW), `list` and `hash` (25 unit + 10 CLI).
  README updated.
- Note for later: the `rotcheck()` helper in `tests/cli.rs` always adds the
  dir as the last argument, so `hash` tests call the binary directly.

**Next:** roadmap item 7. Start with `--quiet` for `verify` (print only the
problem lines, no summary; or nothing at all, just the exit code?). Pick one
and write it down. Then a simple progress line on stderr for big trees (only
when stderr is a terminal; std has `IsTerminal`).

## 2026-10-17

- Roadmap item 7, part 1: `verify --quiet`. Decision: print only the lines
  that affect the exit code, and no summary line (like `sha256sum -c
  --quiet`). So `NEW` lines show only with `--strict`. A silent run means
  exit 0, which is what cron wants (it mails only when there is output).
  Errors (exit 2) still go to stderr.
- Test: 1 CLI test (new file alone is silent/exit 0, shown with `--strict`,
  missing file shown/exit 1) (25 unit + 11 CLI). README: cron example.

**Next:** progress line on stderr for big trees in `init`/`verify`/`update`
(only when stderr is a terminal, `std::io::IsTerminal`; never with
`--quiet`). Something like `\r[1234/5678] hashing...` redrawn at most a few
times a second, cleared at the end. Maybe `update --quiet` too.

## 2026-10-18

- Roadmap item 7, part 2: progress line. New `src/progress.rs`: a
  `Progress` counter that draws `\r ESC[K rotcheck: ...` on stderr at most
  every 100 ms, only when stderr is a terminal (`std::io::IsTerminal`).
  `clear()` wipes it before each status line so report lines stay clean;
  `Drop` clears it too. `init` knows the total (`hashing [n/total]`);
  `verify`/`update` show `checked n files` (no total up front, since new
  files are only found during the scan). `verify --quiet` disables it.
- Tests: 2 unit tests (line text; disabled counter never draws)
  (27 unit + 11 CLI). README: "Progress" section.
- Not checked: the sandbox blocked `script`, so I couldn't watch it in a
  real pty. Try `rotcheck init` on a big folder by hand.

**Next:** have `verify::scan` tell the caller the total (entries + new
files) before hashing, so verify/update can show `[n/total]` too. Then
maybe `update --quiet`, and look at roadmap item 8.
