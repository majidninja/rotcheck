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
7. [x] Progress output for big trees, `--quiet` (`--json` dropped, see 2026-10-21)
8. [x] Integration tests with temp dirs; release builds / install docs

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

## 2026-10-19

- Progress now shows `[n/total]` for `verify` and `update` too, not just
  `init`. `verify::scan` (and so `verify`/`update`) takes a new `total`
  callback, called once after the walk and before any hashing, with the
  number of `report` calls to come (unique manifest entries + new files).
- `Progress` keeps its state in `Cell`s and its methods take `&self`, so
  the `total` closure and the `report` closure can both borrow it. New
  `Progress::set_total`.
- Tests: `verify_reports_every_kind` checks the announced total equals the
  number of reports; the disabled-counter test covers `set_total`
  (27 unit + 11 CLI). README's "Progress" section already described this.

**Next:** `update --quiet` (print nothing on success, for scripts), then
roadmap item 8: release build notes / install docs (`cargo install --path .`,
static musl build?), and decide whether `--json` (item 7) is worth it.

## 2026-10-20

- `update --quiet`, matching `verify --quiet`: no progress line, no summary,
  and only the status lines that affect the exit code (`CORRUPTED` unless
  `--accept-corrupted`, and `UNREADABLE`). The stderr hint about corrupted
  files still shows, since it only appears on failure. So a silent
  `update --quiet` means the manifest now matches the folder.
- Test: 1 CLI test (added + removed files: no output, exit 0, then
  `verify` is clean) (27 unit + 12 CLI). README and `help` text updated.

**Next:** roadmap item 8: install docs (`cargo install --path .`, maybe a
static musl build note), and decide whether `--json` output (item 7) is
worth it or should be dropped from the roadmap.

## 2026-10-21

- Roadmap item 8: `[profile.release]` with `lto`, `codegen-units = 1`,
  `strip`. Release binary is about 390 KB on Linux.
- README "Install": what `cargo install --path .` does, building offline,
  copying the single binary next to an archive, `cargo uninstall`. I left
  out a musl/static build note because I couldn't test it here.
- Decided against `--json`. The report lines are already one file per line
  with a fixed layout (status padded to 10, two spaces, escaped path at
  column 13), which `grep`/`cut` handle fine. A JSON format would be a
  second format to keep stable. README now has "Using the output in
  scripts", which promises this layout.
- Test: `report_paths_start_at_column_13` checks the `cut -c13-` recipe
  from the README (27 unit + 13 CLI).
- Roadmap items 7 and 8 are done, so the original roadmap is finished.

**Next:** pick the next round of work. Ideas: `rotcheck verify --since`
or a way to skip hashing files whose size/mtime changed (faster "quick"
check)? Excluding paths (`.rotcheck-ignore`)? Releasing 1.0 with a
CHANGELOG and a written-down manifest format spec (FORMAT.md) so the format
is clearly stable. FORMAT.md is probably the most valuable for the
two-year goal.

## 2026-10-22

- Wrote FORMAT.md: a spec for the v1 manifest (encoding, header, metadata
  lines, `sha256sum` checksum lines and escaping, path rules, what a
  manifest covers, and a stability promise: v1 only ever gains optional
  `key=value` metadata fields; anything bigger gets a new header version).
  Linked from the README.
- Code change to match the spec: a `# rotcheck manifest vN` header with
  N other than 1 is now refused instead of read as v1, so this rotcheck
  can't misread a future format.
- Tests: `refuses_other_format_versions`, and `format_md_example_parses`,
  which pulls the example out of FORMAT.md with `include_str!` so the spec
  and the parser can't drift apart (29 unit + 13 CLI).

**Next:** a CHANGELOG.md and bump to 1.0.0 now that the format is written
down. After that: an ignore mechanism (`.rotcheckignore`?) or a quick
size/mtime-only check mode.

## 2026-10-23

- Released 1.0.0: version bumped in Cargo.toml, new CHANGELOG.md. The 1.0.0
  entry says what the stable interface is (manifest format v1 per FORMAT.md,
  commands and flags, exit codes, report line layout and status words) and
  what isn't (summary wording, error messages, progress counter). It also
  lists the features in 1.0.
- README: a short "Stability" section that links to CHANGELOG.md.
- Test: `changelog_top_entry_matches_version` checks the top CHANGELOG
  heading against `CARGO_PKG_VERSION` and `rotcheck --version`, so a
  version bump without a changelog entry fails (29 unit + 14 CLI).
- New changes go under a `## Unreleased` heading at the top of the
  CHANGELOG; the test skips that heading.

**Next:** an ignore mechanism. Probably a `.rotcheckignore` file with one
glob-ish pattern per line (keep it simple: exact paths and `*` within a
path segment, no regex), used by `list`, `init`, `verify` and `update`.
Has to be written into FORMAT.md/README too. Alternative: a quick
size/mtime-only `verify --fast`.

## 2026-10-24

- `.rotcheckignore`: new `src/ignore.rs`. One pattern per line, `#`
  comments, `*` matches within a path segment (no other wildcards). No `/`
  in the pattern = matches a name at any depth; with `/` = anchored to the
  root; trailing `/` = folders only. Ignored folders aren't descended into.
  Applied inside `walk::walk`, so `list`, `init`, `verify` and `update` all
  follow it without other changes. The ignore file itself is recorded.
- Decided: entries already in a manifest that become ignored show up as
  `MISSING` in verify and are dropped by `update`. Simple and visible,
  documented in the README.
- Docs: README "Leaving files out", a paragraph in FORMAT.md "What a
  manifest covers", `help` text, CHANGELOG "Unreleased" entry.
- Tests: glob/pattern unit tests, a walk test, and a CLI test where edits
  to ignored files leave `verify` clean (34 unit + 15 CLI).

**Next:** maybe have `update` say how many entries it dropped because
they're now ignored (instead of plain MISSING). Then either a quick
`verify --fast` (size/mtime only) or release 1.1.0 with the ignore file.

## 2026-10-25

- `verify` and `update` now explain entries that are missing only because
  `.rotcheckignore` covers them. `verify` adds a line after the summary
  ("N of the missing files match .rotcheckignore; run `rotcheck update DIR`
  to drop them"). `update` adds "(N of the removed are now ignored)" to its
  summary. Report lines and status words stay the same (they're part of the
  stable interface); only summary wording changed, and that isn't promised
  to stay fixed.
- New `Ignore::covers(rel)`: true if the file or any folder above it is
  ignored, matching what the walk skips.
- Tests: `covers_files_inside_ignored_folders` and the CLI test
  `newly_ignored_entries_are_explained` (35 unit + 16 CLI). README and
  CHANGELOG "Unreleased" updated.

**Next:** release 1.1.0 (move "Unreleased" to a dated 1.1.0 entry, bump
Cargo.toml). Then maybe a quick `verify --fast` that only compares size
and mtime, for a cheap daily check between full runs.

## 2026-10-26

- Released 1.1.0: Cargo.toml bumped, the "Unreleased" changelog notes moved
  into a dated 1.1.0 entry (under "Added"), and an empty "Unreleased"
  heading left on top for the next changes.
- Made the `.rotcheckignore` pattern syntax part of the stable interface
  (stated in the 1.1.0 entry and the README "Stability" section): later 1.x
  may add syntax, but existing ignore files keep their meaning.
- The changelog/version test already covers the bump (35 unit + 16 CLI).

**Next:** `verify --fast`: compare only size and mtime against the
manifest (no hashing), for a cheap check between full runs. It can't tell
corruption from edits, so it should report differing files as `MODIFIED`
and say clearly in the summary that contents weren't checked. Needs README,
help text and a CHANGELOG "Unreleased" entry.

## 2026-10-27

- `verify --fast`: compares only size and mtime against the manifest, no
  hashing. Differing files are `MODIFIED` (never `CORRUPTED`), and the
  summary gets an extra line saying contents weren't checked. Entries with
  no metadata (size 0 + mtime 0, i.e. plain `sha256sum` lines) are hashed
  anyway, since there's nothing else to compare.
- `verify.rs`: the per-file check is now a parameter of a private
  `scan_with`; `scan` uses the hashing `full_check`, the new `verify_fast`
  uses `quick_check`. `update` is unchanged (always hashes).
- Note: picking `verify::verify` vs `verify_fast` as a fn value in main
  failed on closure lifetime inference; passing `&mut report` to either
  call works.
- Docs: README "A quick check" section, help text, CHANGELOG "Unreleased".
- Tests: `fast_verify_compares_only_size_and_mtime` (unit) and
  `fast_verify_skips_contents` (CLI) — 36 unit + 17 CLI.

**Next:** maybe `--fast` should not flag a changed mtime alone when size
matches? (No: without contents we can't know; keep it.) Ideas: a
`rotcheck status`-style "last full verify" timestamp in the manifest
(would need a FORMAT.md comment field), or release 1.2.0 with `--fast`.
