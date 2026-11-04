# Changelog

All notable changes to anathem are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`mem update` tells you whether a newer version exists; this file tells you
what you would be getting. Memories are never touched by an upgrade: they
live in the database, not in the binary.

## [Unreleased]

## [0.2.2] - 2026-11-04

### Added

- A **licence**: MIT. Without one the default is that nobody may copy, modify
  or redistribute the code, which is the opposite of what a public repository
  is for.
- This changelog, and the `license`, `description`, `repository` and `readme`
  fields the manifest was missing.
- **A benchmark anyone can reproduce**, over LongMemEval, with its numbers and
  its caveats in [`docs/benchmarks.md`](docs/benchmarks.md). The figures in
  the README were measured on a corpus nobody else can see; these are not.
  `tools/longmemeval_to_memories.py` converts the dataset, which is not
  redistributed here.
- An eval case can name its `project`, and the search is confined to it —
  needed by any benchmark that gives each question its own history.
- `mem doctor` reports whether the database is readable by other users on the
  machine, and says so without failing the diagnosis.

### Changed

- The seven default tag rules now fire in English as well as Spanish. They had
  been written against a Spanish corpus, so `decision` stayed mute on a
  sentence like "We decided to use SQLite". A `tags.json` next to the database
  still replaces them entirely.
- Repository file names are in English, finishing the migration that 0.2.1
  started: `scripts/install.sh`, `scripts/publish.sh`, `src/permissions.rs`.

### Fixed

- `scripts/publish.sh` claimed there was no previous release when there was
  one, and published without carrying the vocabulary pack forward. It now asks
  the API instead of parsing a listing, and checks afterwards that the pack
  arrived.
- `scripts/publish-mac.sh` tried to upload the artifact directories alongside
  the binaries and died on the last step, after reporting the build and the
  checksums as correct.
- The read-only pack test no longer asserts that a write fails when the suite
  runs as root, where permission bits are ignored.

## [0.2.1] - 2026-11-02

The release that made anathem presentable to someone who is not its author:
everything a user reads is in English, and six security findings are closed.

### Added

- `mem import`, so an export can be used to come back, not only to leave.
- `scripts/publish.sh`, which publishes a release from a workstation without
  the CI, checking its preconditions before it starts.
- Two guard tests that fail when the repository drifts from what it documents:
  one against Spanish re-entering `src/` and `scripts/`, one against documents
  teaching flags or outputs the binary no longer has.

### Changed

- The whole public surface is in English: `config.json` keys, CLI flags, MCP
  tool names, their schemas and their results, the `mem doctor` report, the
  help text and the installer. Old config keys and old flags still work as
  hidden aliases, so existing setups keep running.
- The database schema uses English names, with automatic migration on open.

### Security

- A vocabulary pack name can no longer be a path: `..`, an absolute path or a
  separator is rejected before the file is parsed.
- `mem export` refuses to write a file outside the output directory.
- Everything anathem creates — the database, its `-wal` and `-shm`, and the
  directory holding them — is restricted to its owner on every open.
- An installed vocabulary pack is left read-only.
- An MCP call has caps on what it can ask for, so one request cannot exhaust
  memory.
- The installer stops instead of continuing when it cannot verify a download's
  sha256.
- `cargo audit` runs in CI, and the bundled SQLite went from 3.46 to 3.54.

### Fixed

- Release download links need `filepath`, or the installer gets a 404.
- CI stopped spending 79% of the monthly quota running the Windows suite on
  every push; it now runs on tags and on request.

## [0.2.0] - 2026-10-29

### Added

- `mem recent`, which lists the latest memories of a project and says which
  agent saved each one, so one agent can pick up where another left off.
- That handover now arrives on its own: anathem sends it in the MCP greeting
  through the protocol's `instructions` field, verified against both Claude
  Code and opencode.
- The installer registers anathem as an MCP server, and configures opencode by
  editing the `"mcp"` key in place instead of rewriting the file. `AGENTS.md`
  documents the assisted install.
- `mem update`, which says whether a newer version exists. It touches the
  network only when a person asks it to.

### Changed

- The server releases the vocabulary pack after a stretch with no searches,
  instead of holding 22 MB resident for the life of the session.

### Fixed

- A freshly installed opencode was left without anathem, and silently.

## [0.1.0] - 2026-10-28

First release with prebuilt binaries.

### Added

- Binaries for Linux x86_64, macOS arm64, macOS x86_64 and Windows x86_64,
  each with its sha256. Installing no longer requires Rust; on any other
  platform the installer still compiles from source.
- An installer that detects the platform, verifies the checksums, sets up the
  `PATH`, creates the database and ends with a diagnosis.

### Changed

- The project lives on GitLab. GitHub remains only as the macOS builder, since
  GitLab's macOS runners are Premium and Ultimate only.

[Unreleased]: https://gitlab.com/__alexander__/anathem/-/compare/v0.2.2...main
[0.2.2]: https://gitlab.com/__alexander__/anathem/-/compare/v0.2.1...v0.2.2
[0.2.1]: https://gitlab.com/__alexander__/anathem/-/compare/v0.2.0...v0.2.1
[0.2.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.1.0...v0.2.0
[0.1.0]: https://gitlab.com/__alexander__/anathem/-/releases/v0.1.0
