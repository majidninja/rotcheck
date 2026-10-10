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

> **Status:** early development. Right now rotcheck can hash files and list a folder. The
> folder manifest and verify commands are next (see the roadmap in
> [DEVLOG.md](DEVLOG.md)).

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

## Planned

```sh
rotcheck init ~/Pictures      # record a checksum for every file
rotcheck verify ~/Pictures    # report missing, new, modified and corrupted files
rotcheck update ~/Pictures    # accept intentional changes into the manifest
```

rotcheck separates **modified** files (the timestamp changed, so you probably
edited them) from **corrupted** files (the content changed but the timestamp
didn't). The second kind is the one to worry about.

## License

MIT
