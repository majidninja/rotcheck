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
2. [ ] Recursive directory walk (sorted, skip the manifest itself, don't follow symlinks)
3. [ ] Manifest format + `rotcheck init <dir>` writes `<dir>/.rotcheck.sha256`
4. [ ] `rotcheck verify <dir>`: missing / new / modified / corrupted report
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
