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
5. [ ] `rotcheck update <dir>`: accept changes
6. [ ] Paths with newlines/backslashes (sha256sum's `\` escaping convention)
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
