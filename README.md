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

> **Status:** stable since 1.0. `init` records a folder, `verify` checks it
> and `update` accepts your intentional changes. See [Stability](#stability)
> for what stays fixed, and [DEVLOG.md](DEVLOG.md) for what might come next.

## Install

Each [release](https://github.com/majidninja/rotcheck/releases/latest) has
packages for x86_64 and aarch64 (ARM64) Linux. The binary inside is
statically linked, so they work on any distribution and depend on nothing.

On Debian, Ubuntu and friends, download the `.deb` and install it:

```sh
sudo apt install ./rotcheck_*_amd64.deb      # or _arm64.deb
```

On Fedora, RHEL, openSUSE and friends, use the `.rpm`:

```sh
sudo dnf install ./rotcheck-*.x86_64.rpm     # or .aarch64.rpm
```

Both install the `rotcheck` command and a man page (`man rotcheck`).
On any other Linux, the `.tar.gz` holds the same binary plus the docs:
unpack it and copy `rotcheck` somewhere on your `PATH`. Each release also has
a `SHA256SUMS` file, so you can check the downloads with `sha256sum -c` first.

### From source

You need a Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
git clone https://github.com/majidninja/rotcheck
cd rotcheck
cargo install --path .
```

That builds an optimized binary and puts it in `~/.cargo/bin`, which rustup
adds to your `PATH`. Check it worked with `rotcheck --version`.

rotcheck has no dependencies outside the Rust standard library, so the build
needs no network access once you have the source. If you'd rather not install
anything, build it and copy the single file wherever you like:

```sh
cargo build --release
cp target/release/rotcheck /usr/local/bin/   # or ~/bin, or a USB stick
```

The binary is small (around 400 KB on Linux) and needs nothing else at run
time. Keeping a copy next to an archive, together with this source, means
you can still check it in ten years.

To remove it again: `cargo uninstall rotcheck` (or `sudo apt remove
rotcheck` / `sudo dnf remove rotcheck` for the packages). Manifests are plain
text files, so nothing else is left behind.

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

`init` won't overwrite an existing manifest. File names with a backslash,
newline or carriage return are written the same way `sha256sum` writes them:
the line starts with `\` and the name has `\\`, `\n` or `\r` in it. So those
lines still work with `sha256sum -c`, and checksum lists from `sha256sum`
with such names work as rotcheck manifests. rotcheck's own output (`list`,
`hash`, and the `verify`/`update` reports) escapes these names the same way,
so every file is always on exactly one line.

The full manifest format is written down in [FORMAT.md](FORMAT.md), in case
you ever want to read or write manifests with something other than rotcheck.

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

After you've added, edited or deleted files on purpose, bring the manifest up
to date:

```sh
$ rotcheck update ~/Pictures
MODIFIED    2024/notes.txt
MISSING     old.txt
CORRUPTED   scan.png
NEW         new.txt
rotcheck: manifest updated: 1 added, 1 updated, 1 removed, 0 unchanged, 1 corrupted (kept)
```

`update` reads every file again, like `verify`, and then rewrites the
manifest: new files are added, missing ones are dropped and edited ones get
their new checksum.

Files that look corrupted are left alone. They keep their old checksum, so
`verify` goes on flagging them until you've restored them from a backup. If
you're sure the current content is what you want, say so explicitly:

```sh
rotcheck update --accept-corrupted ~/Pictures
```

It's worth running `verify` and reading its output before `update`, because
`update` accepts every `MODIFIED` and `MISSING` file without asking.

### Leaving files out

Some files change all the time and aren't worth checking: caches, thumbnails,
editor temp files. List them in a file called `.rotcheckignore` at the top of
the folder, one pattern per line:

```
# junk the OS and editors leave behind
.DS_Store
*.tmp
# whole folders
thumbnails/
cache/*
```

- `*` matches anything except `/`. There are no other wildcards.
- A pattern without `/` matches a file or folder name anywhere in the tree.
- A pattern with `/` matches the full path from the top of the folder.
- A trailing `/` means "folders only". Everything inside an ignored folder is
  left out.
- Lines starting with `#` and blank lines are skipped.

`init`, `update`, `verify` and `list` all follow it. If you add a pattern for
files that are already in the manifest, `verify` reports them as `MISSING`
until you run `update`, which drops them. Both commands tell you how many of
the missing files are covered by `.rotcheckignore`, so you can tell them
apart from files that really disappeared. The `.rotcheckignore` file itself
is checked like any other file.

### A quick check: `verify --fast`

A full `verify` reads every byte, which takes a while on a big photo
archive or a slow USB disk. `verify --fast` only compares each file's size
and modification time with the manifest, so it finishes in seconds:

```text
$ rotcheck verify --fast ~/Pictures
MODIFIED    2024/notes.txt
rotcheck: 4211 ok, 0 corrupted, 1 modified, 0 missing, 0 new
rotcheck: fast check: only sizes and times were compared, not contents; ...
```

It finds missing, new and edited files, but it **can't find bit rot**:
a corrupted file keeps its size and timestamp, so it looks fine. Any file
whose size or time changed is reported as `MODIFIED`, even if only its
timestamp moved. Use it for a cheap daily look, and keep a full `verify`
every week or month. (Entries from a plain `sha256sum` list have no sizes
or times, so `--fast` reads those files in full.)

### Exit codes

Handy for cron jobs and scripts:

- `0`: everything recorded is intact. New files alone don't count as a problem,
  unless you pass `--strict` (useful for archive folders that should never
  grow).
- `1`: something is corrupted, modified, missing or unreadable.
- `2`: rotcheck couldn't do the check (bad arguments, no manifest, unreadable
  folder).

For cron, add `--quiet`: `verify` then prints only the files that make it
fail (no summary line, and `NEW` files only with `--strict`). No output means
all is well, so cron only mails you when there's something to look at:

```sh
# every Sunday at 3am
0 3 * * 0  rotcheck verify --quiet ~/Pictures
```

`update` uses the same codes: `0` when the manifest now matches the folder,
`1` when it left a corrupted or unreadable file as it was. `update --quiet`
works like `verify --quiet`: it prints only the corrupted or unreadable files
(plus the hint on stderr), and nothing at all when the update went fine.

```sh
rotcheck verify /mnt/backup/photos || echo "check the photo backup!"
```

`verify` also accepts a plain `sha256sum` list saved as `.rotcheck.sha256`.
Such a list has no sizes or timestamps, so any changed file shows up as
`MODIFIED`. A leading `./` on paths (as `find . -type f -exec sha256sum {} +`
prints them) is fine. Absolute paths and paths with `..` are refused, so a
manifest can never make rotcheck look outside the folder.

### Using the output in scripts

The report lines from `verify` and `update` are meant to be stable: one file
per line, the status word, spaces up to column 13, then the path (escaped as
described above). So plain shell tools are enough, for example to list just
the corrupted files:

```sh
rotcheck verify --quiet ~/Pictures | grep '^CORRUPTED' | cut -c13-
```

The summary line and error messages start with `rotcheck:`, and errors go to
stderr.

### Progress

On big folders, `init`, `verify` and `update` show a running count of files
on the last line of the terminal (`rotcheck: hashing [1234/5678]`). It goes
to stderr, is wiped before each report line, and only appears when stderr is
a terminal, so it never shows up in logs, pipes or cron mail. `verify
--quiet` never shows it.

## Stability

rotcheck is at 1.2. Within 1.x, the manifest format, the commands and flags,
the exit codes, the layout of the report lines and the `.rotcheckignore`
patterns stay as they are, so a
manifest you write today keeps working and so do your cron jobs and scripts.
New commands, flags and status words may be added. [CHANGELOG.md](CHANGELOG.md)
lists exactly what's covered and what changed in each release.

## License

MIT, see [LICENSE](LICENSE).
