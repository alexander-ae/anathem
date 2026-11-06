# Changelog

All notable changes to anathem are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`mem update` tells you whether a newer version exists; this file tells you
what you would be getting. Memories are never touched by an upgrade: they
live in the database, not in the binary.

## [Unreleased]

### Added

- The **minisign public key** releases are signed with. It lives in
  `scripts/install.sh`, in `anathem.pub` at the root and in the project's
  description on GitLab. Only the last of those is independent — the copies
  in here change together with a single push — so the description is the one
  a repository tampered with on its own cannot reach. v0.3.0 was signed after
  it was published, and is the first release for which
  `sh install.sh --verify-signature` has anything to check.

### Added

- **`budget` is a setting**, next to `candidates` and `excerpt` in
  `config.json`. It was a compile-time constant, and it silently governed the
  two beside it: the budget trims the candidate list before `candidates` gets
  to matter, so a configuration that raised `candidates` to 50 and left the
  budget at 1,200 changed nothing. One of the pair was configurable and the
  other was not, which made the configurable one a setting that does nothing
  on its own. The default is the same 1,200, a caller may still override it
  per call, and zero is refused the way `candidates` and `excerpt` already
  refuse it.

- **`search_memory` says how much rephrasing is worth.** It already told the
  model to rephrase when a search came back empty; what it did not say is
  that one more try is not the point. Measured over 16 questions that
  describe a situation instead of naming it, one phrasing lands the memory
  inside the twenty candidates 5 times and three genuinely different
  phrasings land it 15 — and several arrive at rank 1. Rephrasing also
  reorders the ones that already worked: rank 16 to 1, 36 to 2, 43 to 3.

  The guidance lives in the tool description, which is anathem's own voice
  and not retrieved content. The matching signal in the *response* — telling
  the model when nothing matched strongly — is deliberately not here: with
  no stopword list, a coverage threshold would count "que" and "los" as
  terms covered, which is the failure this project names when describing
  Engram, and shipping an uncalibrated threshold is worse than shipping
  none.

- **`mem stats`**, which reads the search log back. "Is this still earning
  its place after a month" had no answer with numbers in it, and the data to
  answer it was already being written: how often a search led to opening a
  memory, what the searches cost in tokens, which ones came back empty, and
  how all of that moves week by week.

  It says what it cannot measure before it says anything else. It is not
  recall — you never learn whether the right memory existed and was missed,
  and a search with no follow-up can mean three different things. And it
  refuses to publish a median rank until there are enough rows to support
  one: in the installation this was built against, 669 searches recorded 19
  follow-ups and **zero** of them recorded which memory, so any MRR would
  have been a number with nothing behind it.

  Nothing in it runs during a search.

- **`mem tune`**, which looks for a better `bm25_weight` for the corpus in
  front of it. There is no right default: measured on LongMemEval the best
  weight is 1.0, and on a corpus of real technical notes it is 8.0 — prose
  gives the vector ranking something to catch, while notes dense in
  identifiers have bm25 already confident at rank 1, where fusing dilutes
  it. The distance is 0.03 to 0.06 of MRR in both directions.

  Finding it normally costs an afternoon of writing questions and labelling
  answers. It does not here, because the topic keys are that labelling
  already done: `--topic` is somebody declaring that a memory is the current
  answer about a subject, which is a query and its expected document. The
  output says, above the table, that a key names its subject in keywords and
  so flatters bm25 — it is for choosing between configurations, where the
  bias falls on every row equally, not for quoting as a quality figure.
  Checked against a hand-labelled set of 50 queries over the same corpus:
  the two rank the weights the same way, Spearman 0.86, and both put the
  default third of seven.

  It recommends and changes nothing, refuses under 20 keys saying how many
  there are, refuses with no vocabulary pack because then every weight gives
  the same answer, and says so when the best value sits on the edge of the
  grid.

- **An eval case can name a memory by id.** `expect_docs` matched
  `memories.ext_id` and nothing else, and only `migrate` and `import` assign
  one — so every memory `save_memory` wrote was unmatchable, and `mem eval`
  could measure an imported corpus but not the corpus a person actually
  accumulated. `id:<number>` names a `memories.id`; a bare entry still means
  an ext_id, so no existing eval set changes meaning. `per_case[].returned`
  now lists those hits too, under the same spelling, because the block
  `benchmarks.md` tells you to compute the strict metrics from was silently
  dropping them and its positions no longer lined up with the ranks beside
  it.

### Changed

- **The binaries are built on GitHub Actions**, in a public mirror that
  exists for that and nothing else, and `scripts/publish-binaries.sh`
  uploads them here. Actions is free and unlimited for public repositories
  on all three operating systems; GitLab's 400 monthly minutes were going
  28 at a time on Windows alone, and its macOS runners are Premium and
  Ultimate only. GitLab keeps the Linux suite and the dependency audit on
  every push, and no longer builds.

- **The README quotes the numbers that are worse, next to the ones that are
  better.** It led with 90.5% recall@5 on LongMemEval-S and said nothing
  about the 78.0% on -M, or the 52% measured over the author's own notes,
  both of which were already in `docs/`. The gap is not a different method —
  a benchmark's questions are written against the sessions that answer them,
  and your notes were not — so it is reported rather than buried.

- **"About 9 MB at rest and single-digit milliseconds" was the best case
  presented as the only one.** 9 MB is a session that has not searched yet;
  one holding the vocabulary pack is about 64 MB, and each agent session runs
  its own `mem serve`. 8 ms is a 449-memory corpus; the public benchmark's
  23,854 take 375 ms. Both figures were already measured in
  `docs/vocabulary-packs.md` and `docs/benchmarks.md`, and the README now
  carries them whole.

- **The head-to-head table against supermemory is gone.** Its LLM provider
  key was expired on that machine, so its memory agent never ran and the
  figures compared a finished anathem against supermemory's chunk layer. The
  caveat was already written underneath; a caveat underneath is not enough,
  because the table is what gets quoted. What stays is the part that does not
  depend on its agent and is a property of the API: it returns at most four
  documents per query, and nothing at all for 19 of 46 questions.

### Fixed

- **A memory in Chinese or Japanese was silently unsearchable.** `unicode61`
  splits on whitespace and punctuation, and those languages have neither
  inside a sentence, so a whole clause was indexed as one token: `設定ファイル`
  sat inside a stored memory and searching for it returned nothing. The
  memory was saved, counted and exported, and could not be retrieved; `mem
  doctor` reported the installation healthy, because everything it checks
  was true. Runs of Han, hiragana and katakana are now indexed as
  overlapping bigrams, and the query is transformed the same way. Korean is
  untouched, since it is written with spaces and already worked. Latin text
  is untouched too, verified rather than assumed: the same 50 queries over
  the same corpus give the same recall@5, recall@50 and MRR as before. An
  existing database migrates itself when it is next opened.

- **`docs/installation.md` said the vocabulary pack "is not published
  anywhere yet".** It has been published since v0.3.0 — the release carries
  `en-es.anathem-pack` with its `.sha256` and its signature, and `install.sh`
  downloads it unless you pass `--no-pack`. The document sent a new reader to
  ask someone for the file or to build it from a 512 MB model instead, and it
  is the document the README sends people to.

- **`docs/security.md` read as though no release had been signed yet.**
  v0.3.0 was, after it was published: five `.minisig` files, one per asset.

- **The minisign key was described as living in "three places", of which two
  were inside the repository.** Three copies that a single push changes
  together are one copy for this purpose. The claim now rests where it
  always did: on the one outside, in the project's description.

- **A sentence in `docs/benchmarks.md` claimed the README had quoted its old
  figures "for years".** The first design document is dated 2026-09-21. It
  quoted them from v0.1.0 to v0.2.2, which is six weeks.

- **Smaller ones**: `comparison-with-supermemory.md` said anathem hands the
  model fifty candidates in one paragraph and twenty in another (it is
  twenty); the README rounded the pack to 22 MB where every other document
  says 22.5; and `installation.md` pointed at `paquetes-de-vocabulario.md`,
  renamed to `vocabulary-packs.md` during the translation.

- **The macOS binaries published as v0.2.1 and v0.2.2 were built from
  v0.2.0's code.** Both carry `commit b0586db`. The workflow checked out
  with no `ref`, so it built whatever sat on the mirror's default branch
  while the tag it was handed went unused, and nothing ever synced that
  mirror. It could not be noticed from the machine that published it,
  because a macOS binary does not run on Linux. The checkout now takes the
  tag, every job refuses to upload a binary whose checkout is not that tag,
  and the publish script pushes the tag to the mirror before dispatching
  anything.

## [0.3.0] - 2026-11-05

### Added

- **Memories received from another agent or another machine**, with
  provenance. `mem save --external --from <label> --project <p>` records who
  sent it, reads the body from stdin when the text is `-` so a note pipes
  over SSH, and saves it as a **draft**: invisible to search until someone
  runs `mem accept <id>`. `--all --from <label>` accepts a sender's batch.

  The draft default is the point. A received memory is text another model
  wrote, perhaps while summarising a page it read, and letting it into an
  agent's context before a person has seen it would make the provenance
  label decorative — it would record who sent it after the agent had already
  read it. The lifecycle had the room: `status` has allowed `draft` since the
  first schema and nothing ever wrote it.

  Once accepted it carries `[from: sender]` in `mem search`, in what MCP
  injects and in `mem recent`, so an agent can tell what this installation
  was told from what it observed. `mem recent --external` lists what arrived,
  drafts included; `mem doctor` says how many are waiting; `export` and
  `import` carry the provenance across.

  `--topic` together with `--external` is refused, and the message says why.
  The transport is SSH and stays that way: no port, no daemon, no API key.
- **`mem show <id>`**, which the CLI never had — it could print a topic and
  not a memory. It shows any status on purpose, since the point is reading a
  draft before deciding whether to accept it.
- **Release signing, the half that does not need the key.**
  `scripts/sign-release.sh` signs a published release with minisign after
  checking every file against its own sha256, uploads the signatures and then
  downloads each one back to verify it. `sh install.sh --verify-signature` is
  the other end: opt-in, and with no public key compiled in it says so and
  refuses rather than reporting a check it did not make. The key is the
  maintainer's and not any CI system's — a pipeline key signs whatever the
  pipeline produced, and this project builds on two of them.
- **A deterministic outline for long memories**, off by default behind
  `outline` in `config.json`. With it on, a candidate whose memory is long
  *and has markdown headings* carries its first paragraph and its headings
  instead of the excerpt — read off the body at search time, with no model,
  nothing generated and nothing stored, so the same body gives the same
  outline forever.

  It is off because of what measuring it showed. Over LongMemEval-S, 98.3% of
  its 23,854 memories are long enough to carry an outline and 1.5% of those
  have a single heading: they are transcripts, not documents. A memory with
  no headings now gets no outline at all and keeps the chunk that matched the
  query, which is worth more than its opening lines at the same price.
- **`--scope key:value` in `mem search`, and a `scope` argument in
  `search_memory`**: narrows the pool to a `project`, a `tag` or a `topic`,
  repeatable and combined with AND. The SQL was already there — `filters_for`
  has built the project and tag `WHERE` since the vector side appeared, and
  `topic` is one more column on the same table — so what this adds is a way
  to say it and a way to compose it. A key it does not recognise is an error
  rather than a filter quietly dropped: a dropped filter returns more than
  was asked for and looks like a search that worked. A scope that holds
  nothing says so, which is a different answer from "no results for this
  query" and the only one of the two where rephrasing will not help.

  **And it is a retrieval improvement, measured.** Narrowing a question's
  pool to a quarter of its distractors, with every evidence session kept,
  moves `recall@5` on `multi-session` questions from 83.9% to 92.8% on bm25
  and from 92.0% to 98.0% on hybrid, and takes the median query from 375 ms
  to 106 ms. That is a ceiling and not a promise — a scope naming the wrong
  tag drops evidence rather than distractors — and the method, the rule
  fixed before the run and what the number does not prove are all in
  [`docs/benchmarks.md`](docs/benchmarks.md).
- **`mem log` and `mem why <id>`**: the retrieval trajectory the MCP server
  has been writing since it existed, which nothing read back. `mem log`
  prints one line per search — when, which project, the query, how many
  memories came back, what they cost in tokens and whether the agent went on
  to open one of them. `mem why <id>` answers it from the other side: which
  searches returned that memory, in what position, and whether it was the one
  opened. A memory that never came back says so, which is the interesting
  case an empty answer would hide. CLI only: no new MCP tool, and the five
  that exist are untouched.
- `mem doctor` reports how large the search log has got. Nothing trimmed that
  table and no command had ever mentioned it existed. `mem log --prune
  <window>` removes entries older than a window you give it, and there is no
  default and nothing that trims on write: retrieval history goes when a
  person says so.
- **`mem show --topic <key>`**: what a subject says right now, without
  reading every version of it. It prints the body of the memory that answers
  for the key, who saved it and when, and below it every version the key has
  had — ids, dates and states, no bodies. `mem topics` was the index and
  there was no way to read the content short of copying an id out of it and
  going to `mem history` or `mem export`. Deterministic output, no colour
  codes, made to be piped. An unknown key lists the keys that project does
  have, the same courtesy `save_memory` already extends.

### Changed

- `mem model install` **refuses a pack that arrives with no `.sha256` beside
  it**, instead of installing it without a word. Every pack
  `tools/build_pack.py` builds carries one and so does every pack a release
  publishes, so its absence means the file travelled without it and nothing
  can check it. `--no-verify` installs anyway, for whoever knows where the
  pack came from. The installer script already refused in that situation; the
  two halves of the same decision now agree.
- The pack that lands in `~/.anathem/paquetes/` now keeps a `.sha256` of its
  own, so reinstalling from there — which is how an index gets rebuilt — can
  still be verified. `mem model remove` takes it away with the pack.

### Fixed

- The installation guide an agent reads, [`AGENTS.md`](AGENTS.md), assumed the
  `claude` and `opencode` commands are on the `PATH`. With only a desktop
  application installed they are not, and the file's answer was the plugin,
  which depends on that same `PATH`, and a relative link to
  `docs/installation.md` that leads nowhere when the file is read from its raw
  URL — which is how an agent reads it when the repository is not cloned. The
  `mcpServers` entry with its absolute paths is now written out where it is
  needed, and so is the condition under which the script passes over opencode
  without saying anything.

- The CI uploaded the Linux and Windows binaries to package version `0.2.2`
  and registered the release links against `v0.2.2`. The job finished green,
  the release page looked complete, and every Linux and Windows download
  returned 404. A new `verificar:release` job now downloads each link it just
  published and fails the pipeline if any of them does not answer 200.

- A memory whose frontmatter is separated by CRLF is no longer dropped on
  import. `mem export` writes LF everywhere, but anything that rewrites a
  memory on Windows — an editor, a script using the platform's default
  newline — turns those separators into CRLF, and `mem import` skipped the
  file as if it had no frontmatter: no error, no count, the memory simply
  gone.

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
