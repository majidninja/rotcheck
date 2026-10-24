# Changelog

All notable changes to rotcheck. Versions follow
[Semantic Versioning](https://semver.org/): from 1.0.0 on, anything listed
under "Stable interface" below only changes in a new major version.

## Unreleased

- `.rotcheckignore`: a file at the top of the checked folder listing files
  and folders to leave out, one pattern per line (`*` within a name, a
  trailing `/` for folders only). Used by `init`, `update`, `verify` and
  `list`.

## 1.0.0 - 2026-10-23

First stable release. Nothing changed in behaviour since the last 0.1.0
build; this release is the promise that it stays that way.

### Stable interface

- The manifest format, `.rotcheck.sha256` version 1, as written down in
  [FORMAT.md](FORMAT.md). Manifests written by any 1.x rotcheck can be read
  by every later 1.x, and they stay readable by `sha256sum -c`.
- The commands and flags: `hash`, `list`, `init`, `verify` (`--strict`,
  `--quiet`), `update` (`--accept-corrupted`, `--quiet`), `help`,
  `--version`. New commands and flags may be added in 1.x.
- Exit codes: `0` all good, `1` something is corrupted, modified, missing or
  unreadable (or new, with `--strict`), `2` the check couldn't run.
- The report lines of `verify` and `update`: status word, spaces up to
  column 13, then the escaped path. The status words are `CORRUPTED`,
  `MODIFIED`, `MISSING`, `NEW` and `UNREADABLE`. A 1.x release may add new
  status words, so scripts should match the ones they care about rather
  than assume this list is complete.

Not covered: the wording of the summary line, error messages and the
progress counter on stderr.

### What's in it

- `rotcheck hash`: SHA-256 checksums in `sha256sum` format, with a built-in
  SHA-256 (no dependencies at all).
- `rotcheck list`: show which files a manifest would cover.
- `rotcheck init`: record checksums plus size and modification time for a
  folder in `.rotcheck.sha256`.
- `rotcheck verify`: tell corrupted files (content changed, timestamp and
  size didn't) apart from ordinary edits, missing and new files. Also reads
  plain `sha256sum` lists. `--strict` fails on new files, `--quiet` prints
  only what makes it fail.
- `rotcheck update`: accept edits, additions and removals into the manifest,
  but keep corrupted files flagged unless `--accept-corrupted` is given.
  `--quiet` stays silent on success.
- `sha256sum`-style escaping for file names with backslashes, newlines or
  carriage returns, in manifests and in all output.
- Manifest paths that are absolute or contain `..` are refused.
- Manifests with an unknown format version are refused instead of misread.
- A `[n/total]` progress counter on stderr, only when it's a terminal.
- Small release binary (LTO, stripped).
