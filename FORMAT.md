# The rotcheck manifest format (v1)

This file describes `.rotcheck.sha256`, the manifest rotcheck keeps at the
top of a checked folder. It's written down so the format stays readable in
years to come, by rotcheck or by anything else: a shell script, another
language, or a person with a text editor.

The short version: it's the output of `sha256sum`, plus `#` comment lines
that hold each file's size and modification time. `sha256sum -c` ignores the
comments, so the file works with standard tools too.

## Example

```text
# rotcheck manifest v1
# size=48213 mtime=1700000000
9f2c5a0e7d3b1c4f8a6e2d9b0c7f1e3a5d8b2c6f9e0a4d7b1c3e5f8a2d6b9e41  2024/beach.jpg
# size=3 mtime=1700000123
\98ea6e4f216f2fb4b69fff9b3a44842c38686ca685f3f55dc48c5d3fb1107be4  odd\nname.txt
```

## Encoding and lines

- The file is UTF-8 text. Lines end with `\n` (rotcheck writes `\n`; a
  reader should also accept `\r\n`).
- Blank lines (empty or only whitespace) are ignored.
- There are three kinds of non-blank lines: the header, metadata lines and
  checksum lines. Any other line starting with `#` is a comment and is
  ignored.

## Header

```text
# rotcheck manifest v1
```

rotcheck writes this as the first line. It's optional when reading, so a
plain `sha256sum` list is a valid manifest. If a line starts with
`# rotcheck manifest v` and the version after it is not `1`, rotcheck refuses
the whole file instead of guessing what a newer format means.

## Metadata lines

```text
# size=<bytes> mtime=<seconds>
```

- A `#` line whose text (after the `#` and any spaces) starts with `size=`.
- Fields are separated by whitespace. `size` is the file length in bytes, a
  non-negative integer. `mtime` is the last-modification time in whole
  seconds since 1970-01-01 00:00:00 UTC, and may be negative.
- Both fields are required. Unknown `key=value` fields are ignored, so later
  versions of rotcheck can add fields without breaking older readers.
- A metadata line describes the checksum line right after it. A checksum
  line without one gets size 0 and mtime 0, which means "unknown": any
  content change then counts as `MODIFIED`, never `CORRUPTED`.

## Checksum lines

Exactly the format GNU `sha256sum` writes:

```text
<hash>  <path>
```

- `<hash>` is the SHA-256 of the file's contents as 64 hex digits. rotcheck
  writes lowercase and accepts either case.
- Then two spaces, then the path. A space followed by `*` (the "binary
  mode" marker from `sha256sum -b`) is accepted in place of the two spaces;
  it means the same thing.
- If the line starts with `\`, the path is escaped: `\\` stands for a
  backslash, `\n` for a newline and `\r` for a carriage return. Any other
  `\` sequence is an error. rotcheck escapes a path (and adds the leading
  `\`) only when it contains one of those three characters, just like
  `sha256sum`. Without the leading `\`, backslashes in the path are literal.

## Paths

- Relative to the folder that holds the manifest, with `/` as the separator
  on every platform.
- A leading `./` is dropped when reading (as `find . -exec sha256sum {} +`
  prints it).
- Absolute paths, and paths with an empty, `.` or `..` component, are
  refused. A manifest can never point outside its folder.
- If the same path is listed twice, the first entry counts.
- rotcheck writes entries sorted by the path's bytes, so two manifests of
  the same folder diff cleanly. Readers shouldn't rely on the order.

## What a manifest covers

`rotcheck init` and `update` record every regular file under the folder,
including hidden files. They skip symlinks (neither followed nor listed),
sockets, FIFOs and devices, and the manifest itself (`.rotcheck.sha256`)
plus its temporary file (`.rotcheck.sha256.tmp`) at the top level. File
names that aren't valid UTF-8 are an error.

Files and folders matched by a `.rotcheckignore` file at the top level are
left out too (see the README for its pattern rules). This only decides which
files get recorded; a manifest reads the same with or without one.

## Stability

Version 1 won't change in a way that makes existing manifests read
differently. New optional `key=value` fields on metadata lines are the only
planned kind of addition. Anything bigger gets a new header version, and
rotcheck will keep reading v1 manifests.
