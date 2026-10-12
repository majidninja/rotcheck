# rotcheck

rotcheck keeps an eye on folders you care about, like photo libraries, music,
scanned documents or backups. It tells you when a file has gone missing, been
changed, or been quietly corrupted on disk.

Disks and flash drives sometimes flip bits without telling you. You usually
find out years later, when an old photo won't open. rotcheck records a SHA-256
checksum for every file once. Later you run it again and it shows you exactly
what's different.

- **Offline.** No accounts, no network, no services that might shut down.
- **No dependencies.** Even SHA-256 is implemented in this repo, so the code
  will still build years from now.
- **Plain-text manifests.** The checksum list uses the same format as
  `sha256sum`, so you can still check your files with standard tools if
  rotcheck ever disappears.

> **Status:** early development, but the core loop works: `init` records a
> folder and `verify` checks it. Accepting intentional changes (`update`) is
> next; see the roadmap in [DEVLOG.md](DEVLOG.md).

## Install

You need a Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
git clone https://github.com/majidninja/rotcheck
cd rotcheck
cargo install --path .
```

## Usage

Print checksums for some files:

```sh
$ rotcheck hash Cargo.toml src/main.rs
28e5cc653ee8554c8a510a14b549875029fc6e81275fff2cb178ddf58864bd79  Cargo.toml
0f9280f25be1cbd1f72a520ee3b23992fcd8f22e294ee3be86547441656324f1  src/main.rs
```

The output is the same as `sha256sum`'s, so you can save it and check it later
with `sha256sum -c`:

```sh
rotcheck hash *.jpg > photos.sha256
sha256sum -c photos.sha256
```

See which files rotcheck would track in a folder:

```sh
$ rotcheck list ~/Pictures
2024/beach.jpg
2024/notes.txt
scan.png
```

Paths are relative to the folder, always use `/`, and are sorted. Symbolic
links are skipped (not followed), so a link can't pull in files from outside
the folder or send rotcheck in circles.

Record a checksum for every file in a folder:

```sh
$ rotcheck init ~/Pictures
rotcheck: recorded 3 files in /home/you/Pictures/.rotcheck.sha256
```

This writes `.rotcheck.sha256` at the top of the folder. It's a plain text
file that looks like this:

```text
# rotcheck manifest v1
# size=48213 mtime=1700000000
9f2c...e41a  2024/beach.jpg
```

The `#` lines hold each file's size and modification time. `sha256sum` skips
them, so you can check the folder without rotcheck too:

```sh
cd ~/Pictures && sha256sum -c .rotcheck.sha256
```

`init` won't overwrite an existing manifest. File names that contain a
newline or a backslash aren't supported yet, and `init` stops with an error
if it finds one.

Check the folder against its manifest, whenever you like:

```sh
$ rotcheck verify ~/Pictures
MODIFIED    2024/notes.txt
MISSING     old.txt
CORRUPTED   scan.png
NEW         new.txt
rotcheck: 1 ok, 1 corrupted, 1 modified, 1 missing, 1 new
```

Every recorded file is read and hashed again, so this takes about as long as
`init` did. Files that are fine aren't listed. The words mean:

| Word         | What happened                                                          |
|--------------|------------------------------------------------------------------------|
| `CORRUPTED`  | The content changed, but the size and timestamp didn't. Likely bit rot. |
| `MODIFIED`   | The content changed and so did the size or timestamp. Probably an edit. |
| `MISSING`    | The file is in the manifest but no longer in the folder.               |
| `NEW`        | The file is in the folder but not in the manifest.                     |
| `UNREADABLE` | The file is there, but rotcheck couldn't read it.                      |

`CORRUPTED` is the one to worry about: restore that file from a backup. A file
whose timestamp changed while its content stayed the same counts as fine.

`verify` never changes anything, neither your files nor the manifest.

### Exit codes

Handy for cron jobs and scripts:

- `0`: everything recorded is intact. New files alone don't count as a problem.
- `1`: something is corrupted, modified, missing or unreadable.
- `2`: rotcheck couldn't do the check (bad arguments, no manifest, unreadable
  folder).

```sh
rotcheck verify /mnt/backup/photos || echo "check the photo backup!"
```

`verify` also accepts a plain `sha256sum` list saved as `.rotcheck.sha256`.
Such a list has no sizes or timestamps, so any changed file shows up as
`MODIFIED`.

## Planned

```sh
rotcheck update ~/Pictures    # accept intentional changes into the manifest
```

## License

MIT
