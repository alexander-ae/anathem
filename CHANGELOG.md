# Changelog

All notable changes to anathem are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`mem update` tells you whether a newer version exists; this file tells you
what you would be getting. Memories are never touched by an upgrade: they
live in the database, not in the binary.

## [Unreleased]

### Added

- **`mem reindex --stem on` makes `registro` find `registros`.** The index
  is `unicode61` with no stemming, so a singular against a plural was not a
  result ranked low — it was no result at all, in a corpus that writes the
  same noun both ways. Snowball for English and Spanish, applied to the index
  and to the query by one function, with identifiers left exactly as written:
  anything carrying a digit or an interior capital, so `n+1`, `OP-204`,
  `RateLimitable` and a SHA are untouched.

  **Off by default, and the setting lives in the database rather than in
  `config.json`.** It describes how the rows were written, not how anyone
  likes things, and a query stemmed against an index that is not matches
  nothing — silently, with no exception to notice it by. Changing it *is*
  rewriting every chunk, so the command that rewrites them is the only way to
  change it. One second over 785 memories.

  **Measured, and smaller than it looked.** Three queries of fifty gained,
  one lost; lexical recall@5 goes 24/34 to 27/34 and the paraphrastic subset
  goes *down* by one. The experiment that motivated it had predicted eight,
  because it simulated stemming by adding surface forms to the query — and
  adding `deployment`, `deploying` and `deployed` as three `OR` branches
  scores differently from collapsing them to one token. A simulation that is
  not the operation it stands for is worth less than its number suggests, and
  that is in `docs/search.md` next to the table.

### Changed

- **Query expansion was measured and is not being shipped.** `docs/search.md`
  had named two levers for the retrieval gap — "a vector layer or query
  expansion" — and only one of them had ever been tried. Expanding a query
  is adding terms to it and `to_match` already joins them with `OR`, so the
  whole experiment ran on an expanded evalset with no production code at all.

  It is not noise: expanding with the same number of words drawn at random
  from the same vocabulary is consistently *worse* than not expanding (24, 22
  and 23 of 50 against a baseline of 26), while the pack's neighbours reach
  30. The semantics are doing the work.

  It still closes, by the rule fixed before the run: `recall@50` never moves
  (41, 42, 41, 40), so nothing new is retrieved, and the paraphrastic subset
  goes from 2 to 3 of sixteen, which is one case. The net is unstable in *k*
  — +1, +4, +1 — with eleven cases changing in both directions at k=5 for a
  net of one.

  What it turned up instead is a different gap: three of the five queries it
  wins are *lexical*, and they are won by morphological variants and
  translations. The index has no stemmer, so `registro` does not match
  `registros` at all — not ranked low, absent. That is written next to the
  numbers.

- **The documentation guard tells a sample from a paragraph.** `no_spanish_
  left_in_the_docs` flagged any line with an accent, including inside fenced
  blocks. A corpus half in Spanish produces Spanish output, so a guard that
  cannot tell a measurement from prose would force the documents to misquote
  their own data to stay green. Fences are skipped; prose is never in one, so
  nothing the test exists for escapes, and that was checked by putting a
  Spanish sentence outside a fence and watching it fail.

## [0.8.0] - 2026-11-08

### Added

- **`mem stale` and `mem origin`, the two mechanical halves of a review
  pass.** Going back over what was saved means judging whether a note matches
  what happened, which takes a model — and a binary that called one would
  need an API key of its own. So the pass runs in the agent session already
  open, and the binary provides only what it can do honestly: which memories
  nobody has opened, and where the evidence for each one is.

  `mem origin` works because saving a memory is a tool call and a tool call
  is recorded with its arguments, so the note is in the transcript verbatim
  and finds itself — no matching by project name or date, both unreliable.
  Measured across 472 transcripts and 1.44 GB: twenty memories tried, twenty
  found, about a second each. It holds for memories saved through MCP and for
  ones saved with `mem save` in a shell an agent was driving; one typed by a
  person in their own terminal is in no log, which is reported plainly rather
  than as an error.

  `mem stale` orders by `last_accessed`, and prints what that signal is not:
  it counts fetching a whole body, so a memory read from the excerpt in a
  search result does not register — and most are, since that is what the
  two-stage protocol is for. The list is where to start looking, not an
  inventory of neglect, and the command says so on every run.

- **`mem tune --apply` writes the weight it recommends.** It measured a
  better `bm25_weight`, named a file, and stopped — on a machine with no
  `config.json` that left the reader to work out that it is JSON and what a
  minimal one looks like. Without `--apply` it now prints the object to
  paste; with it, it writes the key and **keeps every other key the file
  had**, including ones this version does not know about. It edits the parsed
  JSON rather than serialising a `Config`, which would have turned seven
  defaults into settings somebody appears to have chosen. A `config.json`
  that does not parse is reported and left alone, not overwritten.

  It says what it did — created, key added, or the old value replaced — how
  to undo it, and that the measurement is against this corpus's topic keys,
  which issue #25 showed does not carry across corpora.

- **The grid extends itself past the end it landed on.** It used to stop at
  `0.25 … 16` and, when the best value was one of the ends, say that the real
  optimum might lie further out — leaving the user to re-run something it
  could do itself. It now doubles or halves until the best value is interior
  or it runs out of room, which costs one sweep per extra point. On the
  author's corpus that answered the question in one more pass: 32 scores
  worse than 16, so 16 was the optimum rather than the edge of the grid.

- **A fact somebody checked is told apart from one somebody typed.** They
  used to be indistinguishable once saved: same shape, same position, same
  weight when they reached the model, so an agent reading them back had
  nothing to go on. `mem verify <id>` marks the first kind and
  `search_memory` labels it `[verified]`; `save_memory` takes
  `verified: true` for something confirmed in that same session.

  Only the positive case is labelled. Marking the rest would put the word on
  nearly every line of nearly every answer, which is how a signal stops being
  read, and it would read as an accusation against notes written before there
  was any way to say otherwise — so its absence means nobody said, never known
  to be wrong.

  **It moves no ranking**, and a test pins that: promoting what is verified
  would quietly bury a correct hunch, and nothing here moves a ranking on
  something that was not measured. It is additive in the same way the handoff
  columns were — every memory that existed before reads as unverified, with no
  backfill — and confirming is not correcting, so it rewrites neither the body
  nor the date.

### Changed

- **The greeting's token cost is published, after being measured.** It is
  the only text anathem puts in a model's system prompt, so it is paid in
  every session of a project whether or not anything is ever searched, and
  that number was nowhere. On a real corpus with the default
  `handoff_items: 5` it is **186 tokens**, with a ceiling of **470** that
  follows from limits already in the code — 160 characters of title, 40 of
  agent, 40 of topic key, five notes — so it cannot grow with the corpus.
  A search costs about 1,300 for comparison.

  The measurement also answers what to do about it: **nothing**. Of those
  186, a hundred are the fixed framing that tells the model the notes are
  data and not instructions, and only 54 are the notes. Making the list
  terser would cut the smaller half and leave the half that cannot go.

- **Two known limitations were decisions described as shortcomings.** The
  README listed "there is no reranker" among the things anathem lacks, when
  what it has instead is measured and is the design the research it was
  compared against recommends: twenty candidates with excerpts, ordered by
  the model already in the conversation rather than by a second model nobody
  loaded — 72 % of queries to 82 %, and 25 % to 50 % on paraphrastic ones.
  What is actually missing is a dedicated reranking model, which the entry
  now says. The deliberately small default taxonomy was the other: a fixed
  "record only failures" rule was measured **costing** the strongest models
  most of memory's benefit, so leaving the judgement to the agent has
  evidence behind it rather than being an omission.

- **`docs/handoff.md` says what happens when two agents hold one topic
  key.** The code has always guaranteed it — an `immediate` transaction, a
  unique index over the active rows, and twelve concurrent processes in a
  test — but the guarantee was visible only to someone reading `ingest.rs`,
  and the handoff document is exactly where sharing a key comes up. It now
  also says what two writers really cost, which is meaning and not data: the
  loser of the race is left `superseded` rather than overwritten.

### Fixed

- **The README attached 52% to the wrong question.** It read *"recall@5 is
  52% when the question describes the situation instead of naming it"*. 52%
  is the average over all 50 questions — `mem eval` prints it — and the whole
  point of that measurement is that the average hides the split: 88% when the
  question names the thing, 19% when it describes the situation. The README
  stated the correct 19% four hundred lines further down, under *Known
  limitations*, so the document contradicted itself and the generous reading
  was the one on the first screen. The sentence now gives the average and the
  split together, which is the only form in which that number means anything.

## [0.7.1] - 2026-11-08

### Added

- **`cargo install anathem` works.** The crate carries the metadata the
  registry needs — keywords, categories, a homepage — and leaves out of the
  package what nobody depending on it would want: `graft/`, a context index
  for agents working on this repository, and `.claude/`. `docs/` stays in,
  because the README links into it and a published crate whose own links 404
  is worse than 150 KB of markdown. It installs the binary and nothing else:
  no vocabulary pack, no database, no MCP registration. The README says so
  where it offers the path.

### Changed

- **The README leads with the problem instead of with whose product had
  it.** The first screen opened on a competitor by name, which is what the
  head-to-head table was removed for in 0.4.0 — *"a caveat underneath is not
  enough, because the table is what gets quoted"* — and a paragraph one level
  up has the same problem. What leads now is the part that cannot expire: a
  memory that decides on its own what to keep needs a model to decide with,
  so it wants an API key of its own before it will store anything. Nothing
  measured was dropped; the figures keep their caveat in the paragraph where
  they appear, and the one claim that will still be true next year — at most
  four documents per query, nothing at all for 19 of 46 questions — moved to
  *How it works*, where it argues for a design decision instead of
  complaining about a product.

- **The README says where the project lives, and the GitHub mirror stopped
  collecting issues nobody reads.** The mirror is public and exists to build
  the binaries, but nothing on it said so to a reader who arrived from a
  search result: an Issues tab, no statement that it is a mirror, and no link
  to GitLab. Anyone who opened one would have concluded the project was
  abandoned rather than that they were on the wrong site. Issues there are now
  off, its homepage points at GitLab, and one paragraph near the top of the
  README says it — the mirror republishes the README, so that sentence appears
  on both sites and cannot drift between them.

### Fixed

- **An install with no repository stopped looking miscompiled.** `mem
  --version` was a `concat!` that always printed a parenthesis, so a binary
  built where `build.rs` found no git said `mem 0.7.0 (commit unknown)`, and
  `mem doctor` raised a warning: *"there is no way to tell whether this binary
  is up to date with the code"*. Both are true of a random tarball; neither is
  true of an install from a published crate, where the version **is** the
  identity and `mem update` answers the question that matters. It is also what
  every `cargo install` would have looked like. The whole string is now built
  in `build.rs`, the only place that knows whether there was a commit, and the
  diagnosis reports its absence as information rather than as a fault. Inside
  a repository nothing changes.

- **The changelog said GitLab still ran the test suite, and it never did in
  that release.** The 0.4.0 entry claimed *"GitLab keeps the Linux suite and
  the dependency audit on every push"* while the same release deleted
  `.gitlab-ci.yml` — v0.3.0 has the file, v0.4.0 does not. The sentence was
  not stale, it was wrong when it was written, which is why correcting it
  rewrites nothing. What is true: the suite runs on the GitHub mirror, and a
  merge request on GitLab runs no checks at all, so whoever merges one is the
  gate.

## [0.7.0] - 2026-11-07

### Fixed

- **One stopword in the query no longer silences the "nothing matched"
  warning.** The `[no word match]` label asked whether *anything* had
  matched rather than whether anything useful had, so a single `de` gave
  every memory in a Spanish corpus a score and the warning went quiet.
  Measured on a real corpus: `quuxly frobnicate`, with neither word anywhere
  in it, came back with 7 of 8 hits labelled; `de quuxly frobnicate` came
  back with **none**, and `la de el en` answered a question that says
  nothing with eight confident, unlabelled results. The warning therefore
  fired for keyword searches and stayed silent for the questions phrased the
  way a person asks them.

  A term is now counted only when it appears in fewer than half the chunks,
  which is where BM25's own IDF crosses zero — the ranker already treats a
  word past that point as saying nothing. It is frequency and not a stopword
  list on purpose: the index is `unicode61` with no stopwords in any
  language, and dropping short words instead would throw away `api`, `ci`
  and `n+1`. Below twenty chunks the test is suspended, because in a corpus
  that small a frequent word is not a stopword, it is a small corpus.

- **Signing a release no longer fails on a signature that is already
  there.** GitLab answers a duplicate asset link with a 400 that aborts the
  whole run, and `sign-release.sh` had no guard against it — it did not
  need one until the vocabulary pack started being carried over from the
  previous release **with its signature**. Signing v0.6.0 then ended in a
  red error after having correctly signed and uploaded all four binaries.
  Harmless only because the pack happened to be last: first, it would have
  taken the binaries with it and left a release that looked signed in the
  log and was unsigned on the server.


### Added

- **Candidates say how much of the query they hold**, as ` [1/3 words]`
  beside the existing ` [no word match]`. The boolean could not distinguish
  a hit that matched every word from one that matched a single common one,
  which is the ordinary case for a question asked in prose. `mem search
  --explain` prints the same count next to the raw scores.

### Changed

- **The documentation says what the last two releases changed.** `mem doctor`
  grew a section nothing described, `search_memory` prints a label nothing
  explained, and the README still listed the old set of checks. Added: what
  `[no word match]` means and when every candidate carrying it is the
  answer, the near-duplicate report and the 0.21 % that is why nothing
  automatic exists, and the pinned-version example no longer points at
  v0.1.0.


## [0.6.0] - 2026-11-06

### Added

- **A hit no word of the query matched says so**, with ` [no word match]`
  next to its project. The vector ranking orders the whole corpus by
  distance and returns the nearest, so it answers every query — including
  one whose words appear nowhere — and those answers used to be shaped
  exactly like the good ones. The model now has something to tell them
  apart with, and when *every* candidate carries the label the answer reads
  as "nothing here matched" instead of as findings. It is a label and not a
  filter: reach is what the vector layer measurably contributes, and a
  memory sharing no word with the query is sometimes the right one. (#28)

- **`mem doctor` lists memories that look like the same fact saved twice.**
  Information, never a problem — duplication is untidy, not broken. It
  reports and never merges: which of two overlapping notes is the keeper
  needs judgement, and merging destroys the loser. On the corpus it was
  written against it found one real pair on the first run, a note saved
  again after the shell ate part of it and never superseded. (#30)

### Changed

- **`docs/search.md` no longer quotes 97% recall without saying what the
  same measure gives on real notes** (52%). The README already carried both
  figures side by side; this one page did not. (#31)


## [0.5.1] - 2026-11-06

### Fixed

- **The widening that makes project scoping safe now works with a pack
  installed.** It watched for an empty result, and with a vector layer the
  search is never empty: the ranking returns its nearest neighbours whatever
  the query. So in the configuration anathem ships — a pack, installed by
  default — a memory in another project stayed hidden behind whatever the
  local project's vectors happened to answer. The net that justified
  narrowing by default was switched off by the feature that ships enabled.

  What is watched now is whether the project held any **lexical** evidence.
  None, and its vector hits are the nearest average of a corpus that knows
  nothing about the question, which is exactly the case worth looking past
  the project for. Found by running the installer end to end and searching
  for something that only existed elsewhere; v0.5.0 has the defect.


## [0.5.0] - 2026-11-06

### Fixed

- **A release published with `publish-binaries.sh` carries the vocabulary
  pack over.** It did not, and `publish.sh` did: the two publishing paths
  disagreed and the one in use was the one that forgot. v0.4.0 went out with
  four binaries and no pack, which leaves every new install on lexical search
  — quietly, because the installer warns and carries on, which is right for
  the installer and no use as a safeguard. The pack was added to v0.4.0 after
  the fact; now the script carries it, checks it against its own sha256
  before republishing it, and refuses to finish a release without it.

- **The docs that still said search ignores the project.** `README.md`,
  `docs/installation.md` and `AGENTS.md` described the old behaviour.

### Changed

- **`search_memory` now searches the project the session is in.** It was the
  only tool that did not: `save_memory` and `recent_memories` have always
  worked the project out from the working directory, so memories were filed
  by project and then searched as if they were not — the whole corpus on
  every query. That is the regime where the ranking is measurably worst:
  over a pooled corpus of 23,788 memories the right one is in the top five
  47 % of the time, against 90 % for the same corpus and the same questions
  searched within a project.

  Three ways out, all of which win over the implicit project: `all_projects:
  true` searches everything, `project` names another, and a `project:` scope
  is an explicit choice. And the narrowing is not a wall — when the implicit
  project comes back empty the search widens by itself and says so, with
  every hit printing the project it came from. The command line is
  unchanged: `mem search` still searches everything until `--project`.

## [0.4.0] - 2026-11-06

### Added

- The **minisign public key** releases are signed with. It lives in
  `scripts/install.sh`, in `anathem.pub` at the root and in the project's
  description on GitLab. Only the last of those is independent — the copies
  in here change together with a single push — so the description is the one
  a repository tampered with on its own cannot reach. v0.3.0 was signed after
  it was published, and is the first release for which
  `sh install.sh --verify-signature` has anything to check.


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
  Ultimate only. The suite and the dependency audit moved there with them:
  this release deleted `.gitlab-ci.yml`, so nothing runs on GitLab on a
  push or on a merge request, and the mirror is the only gate there is.

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


- **`docs/superpowers/` is gone, and what was load-bearing in it is not.**
  It held 3,200 lines of plans and specs for work that shipped months ago,
  and — mixed in with them — the two measurements the code cites for why
  `bm25_weight` is 1.0 and why the fusion is weighted the way it is. One of
  those was linked from an English document, so a reader following
  `search.md` landed in Spanish.

  Those two are now one English document,
  [`docs/choosing-the-defaults.md`](docs/choosing-the-defaults.md), which
  `config.rs`, `fusion.rs` and `search.md` point at. It keeps the part worth
  keeping: that the default used to be 0.25, that it was corrected against a
  stronger BM25, and the lesson underneath — a component replaced by a
  weaker approximation moves the optimum of everything calibrated against
  it. It also keeps a hypothesis that was tested and discarded in September,
  which turns out to be the same wall a different attempt hit in November.

  The live engineering debt that was buried in `capa-vectorial-pendientes.md`
  became an issue instead of disappearing: two allocations per row per
  query, up to 200 single-row queries per search, and 1 KB per chunk — which
  is exactly what the latency curve measured since then is made of.

  `docs/historia/` stays. It is one file, the README links it as the
  historical document it is, and it costs nothing.

- **The last two Spanish names are gone from disk**: `~/.anathem/paquetes/`
  is `packs/`, and the project a memory gets when nothing else applies is
  `no-project` rather than `sin-proyecto`. Every other surface a person sees
  had been English for months; these two were left because they are data
  rather than source, and moving data carelessly loses memories.

  So neither is a rename in place. The packs directory is **not moved**: a
  new installation gets `packs/`, one that already has `paquetes/` keeps it
  and keeps working, and `mem doctor` says how to move it and the one
  condition that makes it safe — no `mem serve` running, since the file in
  there is memory-mapped. Whoever renames it by hand gets the new name
  honoured from then on.

  The project name is migrated, because a corpus with rows under both
  spellings would filter wrong in both directions: one `UPDATE` when the
  database is opened. Anything that still produces the old string — an
  export taken yesterday, a script, another machine's database about to be
  imported — is folded into the new name on insert, so it cannot come back.

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

### Fixed

- **`mem` no longer panics when whatever was reading its output goes away.**
  `mem search x | head -1`, a `less` quit before the end, any pipeline whose
  reader leaves first: Rust ignores `SIGPIPE`, so the write came back
  `EPIPE` and `println!` panicked on it, printing a backtrace note at
  someone who had done nothing unusual. The default handler is restored at
  the top of `main`, and the process now ends the way `grep` and `cat` do.
  This also changes how `serve` ends when its MCP client disappears: it is
  killed by the signal instead of reporting `Error: Broken pipe` to a client
  that is already gone.

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

[Unreleased]: https://gitlab.com/__alexander__/anathem/-/compare/v0.8.0...main
[0.8.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.7.1...v0.8.0
[0.7.1]: https://gitlab.com/__alexander__/anathem/-/compare/v0.7.0...v0.7.1
[0.7.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.6.0...v0.7.0
[0.6.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.5.1...v0.6.0
[0.5.1]: https://gitlab.com/__alexander__/anathem/-/compare/v0.5.0...v0.5.1
[0.5.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.4.0...v0.5.0
[0.4.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.3.0...v0.4.0
[0.3.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.2.2...v0.3.0
[0.2.2]: https://gitlab.com/__alexander__/anathem/-/compare/v0.2.1...v0.2.2
[0.2.1]: https://gitlab.com/__alexander__/anathem/-/compare/v0.2.0...v0.2.1
[0.2.0]: https://gitlab.com/__alexander__/anathem/-/compare/v0.1.0...v0.2.0
[0.1.0]: https://gitlab.com/__alexander__/anathem/-/releases/v0.1.0
