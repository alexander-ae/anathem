# anathem

Local memory for coding agents (Claude Code, opencode): one binary, one
SQLite file, no daemon and no HTTP server. An agent saves what it learned
and finds it again in the next session — on your machine, with no model call
and no embedding on write, and asking nobody for an API key. Reading it back
is what costs context: about 1,300 tokens per search, measured, and the
excerpt protocol is what keeps that from being the whole memory.

Search is BM25 by default. An optional 22.5 MB vocabulary pack adds a vector
layer that helps across languages; the binary works just as well without it.

The source, the issues and the releases live on
[GitLab](https://gitlab.com/__alexander__/anathem). The
[GitHub repository](https://github.com/alexander-ae/anathem) is a mirror that
builds the binaries, nothing else — its issues are turned off, and a merge
request belongs on GitLab.

## Why it exists

The problem it was built for: a memory that decides on its own what is worth
keeping. Deciding takes a model, so a system built that way wants an LLM API
key of its own before it will store anything — a second provider to pay, to
rotate, and to hand your notes to, in exchange for having them summarised by
someone else's machine.

The deciding is not free in the other sense either. Measured over a
390-document corpus, **56% of what one such system saved that way was
noise**: facts nobody asked for, extracted from conversations and stored with
the same standing as the ones that mattered. Holding them cost **2 GB
resident**, on a laptop that also has to run an editor. And its embedder
normalised Spanish into English before storing, so a question in Spanish
shared neither a lexical nor a semantic anchor with the fact it was looking
for. That system was a self-hosted supermemory v0.0.8 — one installation,
measured by the author in September 2026 and not since, so it says why this
exists and not what that project is today.

anathem takes the opposite bets. Nothing is written unless someone asks for
it. Writing costs no model and no tokens. Corrections are explicit —
a topic key or an id — and the old version stays in the history rather than
competing with the new one in searches. It is also far smaller and faster,
and *What it costs*, below, says by how much without rounding in its own
favour.

Whether those are the right bets for you is a different question, and three
documents answer it with numbers instead of adjectives:
[supermemory](docs/comparison-with-supermemory.md),
[Engram](docs/comparison-with-engram.md) — the nearest neighbour, which took
the same decisions separately — and
[OpenViking](docs/comparison-with-openviking.md), which takes the opposite
ones at a far larger scale.

## When a text file is enough

If your notes fit in a `CLAUDE.md` or an `AGENTS.md` of a couple hundred
lines, use that: no install, no database, and the model reads all of it every
time. anathem begins to earn its place when the notes stop fitting — 449 of
them in the measurement below — and something has to choose which twenty go
in front of the model, at about 1,300 tokens, instead of all of them or none.

Over those 449 memories and 50 questions, recall@5 is 52% when the question
describes the situation instead of naming it. Over a separate 174-memory
corpus, the two-stage excerpt put the right memory among the twenty
candidates in 82% of queries, against 72% before it. A file you paste has
none of that problem and none of that cost.

What a file also cannot do: hand work from one agent to the next, keep a
superseded version without it competing in searches, or answer "what did we
decide about the payment provider" without you grepping for it.

And what anathem does not do either: it never decides what to remember. If
the agent does not save it, nothing is there.

## Install

```bash
curl -fsSL https://gitlab.com/__alexander__/anathem/-/raw/main/scripts/install.sh -o install.sh
sh install.sh --setup-path --connect
```

It downloads the prebuilt binary for your platform, checks its sha256,
installs it at `~/.local/bin/mem`, creates the database at
`~/.anathem/mem.db`, downloads the vocabulary pack (22.5 MB; `--no-pack`
skips it and leaves search lexical) and registers anathem as
an MCP server. It does not need Rust: there are binaries for Linux x86_64,
macOS (both architectures) and Windows x86_64, and on anything else it
compiles from source.

Check it with `mem doctor`, which has to end in `All good.` and exit 0.

Inside Claude Code, the repository is its own marketplace, so there is no
JSON to edit:

```
/plugin marketplace add https://gitlab.com/__alexander__/anathem.git
/plugin install anathem@anathem
```

The step-by-step version, what the script cannot do on its own and how to
connect each agent by hand: [`docs/installation.md`](docs/installation.md).

### Checking that a release is really ours

Releases are signed with [minisign](https://jedisct1.github.io/minisign/).
The public key is below, in [`anathem.pub`](anathem.pub) and in
[`scripts/install.sh`](scripts/install.sh) — and, outside the repository, in
the project's description on GitLab. Only that last copy is independent: the
three in here change together with a single push, so the description is what a
repository tampered with on its own cannot reach:

```
RWSWmH6T7DiU70G5GQAS2vwI5/3YwCf0LlwSgJFQ/mlm3oh2CyVD0XdT
```

```bash
sh install.sh --verify-signature                       # the installer checks it
minisign -Vm mem-linux-x86_64.tar.gz -p anathem.pub    # or by hand
```

The `.sha256` beside each file catches a corrupt download; it cannot catch a
tampered release, because it travels with the file it describes. That is
what the signature is for.

## In thirty seconds

```bash
mem save "The payment provider is Adyen since October" --topic payment-provider
mem search "payment provider"
mem show --topic payment-provider
```

Saving with `--topic` is how a fact that will keep changing gets corrected:
the next save under the same key replaces the previous one, which stays in
the history rather than competing with it in searches.

From an agent, none of this is typed. The MCP server exposes five tools —
`search_memory`, `save_memory`, `get_memory`, `list_tags` and
`recent_memories` — and they carry their own explanation, so there is
nothing to learn by heart.

## Subcommands

| Command | What it does | Example |
|---|---|---|
| `init` | Creates the database if it does not exist | `mem --db ~/.anathem/mem.db init` |
| `migrate --from <jsonl>` | Imports a frozen supermemory export | `mem migrate --from my-export.jsonl` |
| `search <query>` | Searches the memory (BM25, filters by `status=active`; if a vocabulary pack is installed, it merges the ranking with vector similarity) | `mem search "cache breakdown" --project my-project` |
| `search <query> --scope <key:value>` | Narrows the search to a `project`, a `tag` or a `topic`. Repeatable, and the scopes combine with AND. A key it does not know is an error, never a filter quietly ignored. It removes distractors from the pool; it does not change the ranking | `mem search "branching" --scope topic:branching-policy --scope tag:decision` |
| `save <text>` | Saves a new memory; without `--project` it derives it from the repository you are standing in | `mem save "The /api/x endpoint now pages by 50"` |
| `save <text> --topic <key>` | With a topic key, the new memory replaces the active one holding the same key in the same project (the old one is left `superseded`, not edited and not deleted). It says which one it replaced. Without `--topic`, nothing changes | `mem save "The payment provider is Adyen" --topic payment-provider` |
| `topics` | Lists the topic keys: project, how many memories each one has had and the id of the current one | `mem topics` |
| `save <text> --external --from <label> --project <p>` | Saves a memory **sent by another agent or another machine**. It arrives as a draft, invisible to search until someone accepts it, and keeps who sent it. `<text>` can be `-` to read the body from stdin, so a note pipes over SSH | `ssh host mem save - --external --from claude@laptop --project api < note.md` |
| `accept <id>` | Lets a received memory into search. `--all --from <label>` does a whole sender's batch | `mem accept 42` |
| `show <id>` | Prints one memory whole, whatever its status: the body, where it came from and whether it is still waiting | `mem show 42` |
| `recent --external` | Only what arrived from elsewhere, drafts included — which is how you see what is waiting | `mem recent --external` |
| `show --topic <key>` | What that topic says today — the body of the version that answers for the key — and below it every version it has had, with ids, dates and states. `topics` is the index; this is the content | `mem show --topic payment-provider` |
| `edit <id>` | Edits the body of a memory in `$EDITOR` | `mem edit 42` |
| `log` | What the MCP server searched for, newest first: query, how many memories came back, what it cost and whether the agent then opened one. Searches run from this CLI are not logged. `--prune <window>` removes old entries, and nothing else ever does | `mem log --since 24h` |
| `why <id>` | Which logged searches returned that memory, in what position, and whether it was the one opened | `mem why 42` |
| `export --out <dir>` | Exports everything to markdown with frontmatter, one file per memory, losing nothing ([format](docs/export-format.md)) | `mem export --out export/` |
| `import --from <dir>` | Reads what `export` wrote: restoring a backup, moving the memory to another machine, or syncing two computers by hand with git. It demands an empty database unless you pass `--merge`, which adds only what is missing | `mem import --from export/` |
| `reindex` | Rebuilds the chunks and the FTS5 index from `memories.body` | `mem reindex` |
| `supersede <new> <old>` | Marks `old` as replaced by `new`; it stops showing up in searches | `mem supersede 42 43` |
| `pin <id>` | Pins a memory so it survives the search's trimming of candidates | `mem pin 17` |
| `unpin <id>` | Removes the pin | `mem unpin 17` |
| `forget <id>` | Takes a memory out of search without deleting the row | `mem forget 88` |
| `history <id>` | Shows a memory's supersession chain, newest to oldest | `mem history 42` |
| `recent` | What was saved lately, newest to oldest, and which agent saved it. It is the opposite of `search`, which orders by relevance. `--since` accepts `6h`, `90m`, `2d` or `todo` (no bound); 24h by default | `mem recent --since 6h` |
| `stats` | What the search log says about whether this is earning its place: how often a search led to opening a memory, what it cost in tokens, and the queries that found nothing. Read-only, and it says above the table what it cannot measure | `mem stats --since 30d` |
| `tune` | Looks for a better `bm25_weight` for **this** corpus, using the topic keys as a free eval set. It prints a table and a recommendation and changes nothing; it needs a vocabulary pack and at least 20 topic keys | `mem tune` |
| `eval --evalset <json>` | Runs the evaluation harness | `mem eval --evalset eval/evalset.json --out eval/results/anathem-bm25.json` |
| `serve` | Starts the MCP server over stdio (5 tools: `search_memory`, `save_memory`, `get_memory`, `list_tags`, `recent_memories`) | `mem serve` |
| `model install <file>` | Installs a vocabulary pack (`.anathem-pack`) and reindexes every chunk | `mem model install mini-es.anathem-pack` |
| `model status` | Shows the active pack, its dimensions and the vector coverage | `mem model status` |
| `model remove` | Removes the vectors **and deletes the pack file**; search goes back to bm25 only | `mem model remove` |
| `update` | Checks whether a newer version has been published. It goes to the network **only when you ask it to**: anathem does not talk to any server on its own. It downloads nothing and replaces nothing, it tells you what to run | `mem update` |
| `doctor` | Diagnoses the installation without modifying anything: version and commit of the binary, database (counts, FTS5 index, orphans, supersession chains, memories that look like the same fact saved twice), vector layer (pack, file, coverage) and `config.json`/`tags.json`. It exits with 0 if there are no problems, 1 if there are and 2 if it could not run, so a script can tell "it found problems" from "it broke"; having no pack installed is information, not a problem, and a database that does not exist is a notice (code 0) | `mem doctor` |

Every subcommand accepts `--db <path>` (`~/.anathem/mem.db` by default).
`mem --version` prints the version and the commit the binary was built from.
That commit is the one at `HEAD` when it was built: it does not reflect uncommitted
changes, and `mem doctor` shows it but does not compare it against anything. Knowing
whether the installed binary is up to date is still up to whoever runs it: compare it
with `git rev-parse --short HEAD` in the repository.
See [`docs/vocabulary-packs.md`](docs/vocabulary-packs.md) for
the pack format and how to generate one.

## How it works

- **Search** is BM25 over chunks, merged with a vector ranking when a pack
  is installed. `search_memory` returns twenty candidates with a ~120
  character excerpt each, so the model picks among more options at the same
  token cost. Twenty and not a handful because a memory that hands back only
  what it is confident about declines to answer more often than it looks:
  measured over a public corpus, the system this replaced returned at most
  four documents per query and **nothing at all for 19 of 46 questions**
  ([the numbers](docs/comparison-with-supermemory.md)). Filtering is cheap for
  a model already in the conversation; a missing document cannot be recovered
  by anyone. `--scope project:/tag:/topic:` narrows the pool, which is a
  measured improvement and not only a convenience: +8.9 points of recall@5 on
  `multi-session` questions, +6.0 with the pack. Every candidate says how
  much of the query it holds — `[no word match]`, `[1/3 words]`, or nothing
  when all of it landed — counting only the words rare enough to tell one
  memory from another, so one `de` in a Spanish query no longer makes an answer
  look matched. That ranking sorts the whole corpus by distance and always
  answers, so the label is what separates a find from the nearest neighbour
  of a corpus that knows nothing about the question.
  → [`docs/search.md`](docs/search.md)
- **Every memory belongs to a project**, derived from the repository you are
  standing in unless you say otherwise. `search_memory` searches that project
  and widens by itself when it finds nothing there; `mem search` on the
  command line searches everything until you pass `--project`.
  → [`docs/search.md`](docs/search.md)
- **Agents hand work over** through `mem recent` and a short summary anathem
  sends in the MCP greeting, so the next session starts knowing what the
  last one left. A memory can also arrive from another machine, marked with
  who sent it and held as a draft until you accept it. That summary goes into
  the system prompt, so it is also a prompt-injection route: a note an agent
  saved summarising a web page ends up there. It travels inside a delimited
  block, sanitised so its content cannot close it, and labelled as data with
  the name of whoever saved each note; what that covers, and what it does
  not, is in [`docs/security.md`](docs/security.md).
  → [`docs/handoff.md`](docs/handoff.md)
- **The vocabulary pack** is the optional vector layer: 22.5 MB, no model at
  runtime, and it earns its size on cross-lingual search.
  → [`docs/vocabulary-packs.md`](docs/vocabulary-packs.md)

## Configuration

A `config.json` next to the database, every key optional:

| key | default | what it does |
|---|---|---|
| `bm25_weight` | `1.0` | how much the lexical ranking weighs against the vector one. The right value depends on your corpus — `mem tune` measures it |
| `candidates` | `20` | how many candidates `search_memory` returns |
| `excerpt` | `120` | characters of each candidate's snippet |
| `budget` | `1200` | tokens `search_memory` spends on its answer. It trims the list before `candidates` applies, so raising one without the other changes nothing |
| `outline` | `false` | replaces the excerpt of a long memory that has headings with an outline of it |
| `handoff_items` | `5` | memories named in the MCP greeting; `0` turns it off |
| `handoff_window` | `"24h"` | how far back that greeting looks |
| `release_pack_after` | `300` | seconds idle before the pack is released from memory; `0` never |

A malformed file fails loudly, naming the file and the key. It never falls
back silently to a value you did not ask for. What each default was measured
against is in [`docs/search.md`](docs/search.md) and
[`docs/vocabulary-packs.md`](docs/vocabulary-packs.md).

## What it costs

A session that has not searched holds about **9 MB**; one with the vocabulary
pack loaded holds about **64 MB**, and the pack is released after five minutes
idle, which takes it back to 12. Each agent session runs its own `mem serve`,
so four of them searching at once is a quarter of a gigabyte and not nine
megabytes — still well under one 2 GB process, and not the 220× the first
figure on its own would suggest. The measurements, and the setting that tunes
the release, are in
[`docs/vocabulary-packs.md`](docs/vocabulary-packs.md).

Search answers in **8 ms** over a 449-memory corpus and **375 ms** over the
23,854 of the public benchmark below. Which of those you see is a fact about
how much you have saved.

## How it measures up

On [LongMemEval-S](docs/benchmarks.md) — 470 answerable questions over
23,854 memories, a public dataset anyone can download — recall@5 is **82.3%
with BM25 alone and 87.9% with the vocabulary pack**, counting a question as
answered only when *every* evidence session it needs is in the top five.
Counted the looser way, as the fraction of the evidence that lands there:
90.5% and 92.9%. The strict one is the one to carry, because it is the one
that answers "did I get everything I needed".

Those are retrieval numbers: whether the evidence was handed to the model,
not whether the model then answered correctly. The method, the three
definitions of recall and what none of it proves are in
[`docs/benchmarks.md`](docs/benchmarks.md).

**Depth costs more than the headline admits.** On LongMemEval-M, where each
question searches some 476 sessions instead of some 50, the same two runs are
**78.0% and 81.6%**. That is the same code against a deeper haystack, and it
is the number to carry if your memory is going to get large.

**And a real corpus is harder than either.** Measured over the author's own
notes, asked in prose rather than in keywords, recall@5 is **52%** — 449
memories and 50 queries, in
[`comparison-with-engram.md`](docs/comparison-with-engram.md). An earlier
measurement split those queries by how they were phrased, and the split is
the finding: **88%** when the question named the thing, **19%** when it
described the situation instead, in
[`comparison-with-supermemory.md`](docs/comparison-with-supermemory.md).

Fifty queries is a small sample and one person's writing is not everyone's,
so treat those as the author's own calibration rather than as a measurement
you could repeat. They are here because they are worse than the headline, not
because they are strong enough to argue with.

Both are true, and the distance between them is the honest subject.
LongMemEval's questions are written against the sessions that answer them, so
they share vocabulary with the evidence; a note you wrote in June and a
question you ask in November often share nothing but the subject. So the
public figure is the reproducible one and the private figure is the one you
will live with, and which of the two you get depends on how you write and how
you ask. The second cannot be reproduced by anybody else — which is why it is
labelled rather than headlined, and why it is here rather than left out.

## Known limitations

- `migrate` and `save_memory` assign no tag to a body that matches no rule
  of the active taxonomy (the default one or `tags.json`). That is
  expected: the default taxonomy is small on purpose and does not claim to
  cover all the legitimate knowledge of a project (product decisions,
  operational procedures, etc.) — extending it is a decision for whoever
  operates each installation, not a bug.
- A curated document that the source export lists with status `failed` is
  not migrated: `migrate` reports it but does not insert it. Its content
  stays intact in the source JSONL; it has to be reimported by hand with
  `mem save`.
- `mem supersede` does not automatically link a memory with its later
  correction: which one replaces which is a judgement call for whoever
  migrates or edits, not something the binary can infer safely. What the
  binary does do is replace when whoever saves declares it with a topic key
  (`mem save --topic`, or `topic` in `save_memory`). The key is exact and it
  is set by whoever saves; a new key does not reach an old memory that was
  saved without one — that one is marked with `mem supersede` or, from MCP,
  with `supersedes: <id>` in `save_memory`.
- **There is no reranker.** A question that describes the situation instead
  of naming it is the weak case: 19% recall@5 on the author's own notes,
  against 88% when the question names the thing (see *How it measures up*).
  The two-stage excerpt — handing the model twenty candidates and letting it
  order them — is what stands in for one.

## Documentation

- Installing, and connecting each agent: [`docs/installation.md`](docs/installation.md)
- How search works and how to tune it: [`docs/search.md`](docs/search.md)
- Handing work between agents, and memories received from elsewhere: [`docs/handoff.md`](docs/handoff.md)
- Vocabulary packs, and what several sessions cost in RAM: [`docs/vocabulary-packs.md`](docs/vocabulary-packs.md)
- Security: where text you did not write gets in, what is mitigated and what is not: [`docs/security.md`](docs/security.md)
- Measuring retrieval against a public dataset: [`docs/benchmarks.md`](docs/benchmarks.md)
- Why `bm25_weight` and the pack size are what they are: [`docs/choosing-the-defaults.md`](docs/choosing-the-defaults.md)
- The export format, which is complete enough to rebuild the database elsewhere: [`docs/export-format.md`](docs/export-format.md)
- Windows, and what is still unverified there: [`docs/windows.md`](docs/windows.md)
- Compared with [supermemory](docs/comparison-with-supermemory.md), [Engram](docs/comparison-with-engram.md) and [OpenViking](docs/comparison-with-openviking.md)
- What changed in each version: [`CHANGELOG.md`](CHANGELOG.md)
- Where the tests run: on the GitHub mirror, through
  [`.github/workflows/test.yml`](.github/workflows/test.yml). GitLab has no
  pipeline, so a merge request opened there runs **no checks at all** —
  run `cargo test` before opening one, and expect whoever merges it to be
  the only gate.
- The phase 1 spec, a historical document written against the author's
  private corpus: [`docs/historia/2026-09-21-memoria-local-design.md`](docs/historia/2026-09-21-memoria-local-design.md)

## License

MIT. See [`LICENSE`](LICENSE).

You can use it, change it and ship it, including in closed-source products.
The one condition is that the copyright notice and the licence text travel
with any substantial portion of the code you redistribute.
