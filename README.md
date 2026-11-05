# anathem

Local memory for coding agents (Claude Code, opencode): one binary, one
SQLite file, no daemon and no HTTP server. An agent saves what it learned
and finds it again in the next session — on your machine, costing no tokens
to write and asking nobody for an API key.

Search is BM25 by default. An optional 22 MB vocabulary pack adds a vector
layer that helps across languages; the binary works just as well without it.

## Why it exists

The tool it replaces, a self-hosted supermemory, ran **2 GB resident for 390
documents** on a laptop that also has to run an editor. Its embedder
normalised Spanish into English before storing it, so a question in Spanish
shared neither a lexical nor a semantic anchor with the fact it was looking
for. And it decided on its own what was worth keeping: measured over that
corpus, **56% of what it saved that way was noise**.

anathem takes the opposite bets. Nothing is written unless someone asks for
it. Writing costs no model and no tokens. Corrections are explicit —
a topic key or an id — and the old version stays in the history rather than
competing with the new one in searches. It is about 9 MB at rest and answers
in single-digit milliseconds.

Whether those are the right bets for you is a different question, and three
documents answer it with numbers instead of adjectives:
[supermemory](docs/comparison-with-supermemory.md),
[Engram](docs/comparison-with-engram.md) — the nearest neighbour, which took
the same decisions separately — and
[OpenViking](docs/comparison-with-openviking.md), which takes the opposite
ones at a far larger scale.

## Install

```bash
curl -fsSL https://gitlab.com/__alexander__/anathem/-/raw/main/scripts/install.sh -o install.sh
sh install.sh --setup-path --connect
```

It downloads the prebuilt binary for your platform, checks its sha256,
installs it at `~/.local/bin/mem`, creates the database at
`~/.anathem/mem.db`, downloads the vocabulary pack and registers anathem as
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
The public key is below, and the same key is in
[`anathem.pub`](anathem.pub) and in the project's description — three
places, so a repository tampered with on its own cannot swap it without the
others disagreeing:

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
| `eval --evalset <json>` | Runs the evaluation harness | `mem eval --evalset eval/evalset.json --out eval/results/anathem-bm25.json` |
| `serve` | Starts the MCP server over stdio (5 tools: `search_memory`, `save_memory`, `get_memory`, `list_tags`, `recent_memories`) | `mem serve` |
| `model install <file>` | Installs a vocabulary pack (`.anathem-pack`) and reindexes every chunk | `mem model install mini-es.anathem-pack` |
| `model status` | Shows the active pack, its dimensions and the vector coverage | `mem model status` |
| `model remove` | Removes the vectors **and deletes the pack file**; search goes back to bm25 only | `mem model remove` |
| `update` | Checks whether a newer version has been published. It goes to the network **only when you ask it to**: anathem does not talk to any server on its own. It downloads nothing and replaces nothing, it tells you what to run | `mem update` |
| `doctor` | Diagnoses the installation without modifying anything: version and commit of the binary, database (counts, FTS5 index, orphans, supersession chains), vector layer (pack, file, coverage) and `config.json`/`tags.json`. It exits with 0 if there are no problems, 1 if there are and 2 if it could not run, so a script can tell "it found problems" from "it broke"; having no pack installed is information, not a problem, and a database that does not exist is a notice (code 0) | `mem doctor` |

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
  token cost. `--scope project:/tag:/topic:` narrows the pool, which is a
  measured improvement and not only a convenience.
  → [`docs/search.md`](docs/search.md)
- **Every memory belongs to a project**, derived from the repository you are
  standing in unless you say otherwise. Search does not filter by it on its
  own. → [`docs/search.md`](docs/search.md)
- **Agents hand work over** through `mem recent` and a short summary anathem
  sends in the MCP greeting, so the next session starts knowing what the
  last one left. A memory can also arrive from another machine, marked with
  who sent it and held as a draft until you accept it.
  → [`docs/handoff.md`](docs/handoff.md)
- **The vocabulary pack** is the optional vector layer: 22 MB, no model at
  runtime, and it earns its size on cross-lingual search.
  → [`docs/vocabulary-packs.md`](docs/vocabulary-packs.md)

## Configuration

A `config.json` next to the database, every key optional:

| key | default | what it does |
|---|---|---|
| `bm25_weight` | `1.0` | how much the lexical ranking weighs against the vector one |
| `candidates` | `20` | how many candidates `search_memory` returns |
| `excerpt` | `120` | characters of each candidate's snippet |
| `outline` | `false` | replaces the excerpt of a long memory that has headings with an outline of it |
| `handoff_items` | `5` | memories named in the MCP greeting; `0` turns it off |
| `handoff_window` | `"24h"` | how far back that greeting looks |
| `release_pack_after` | `300` | seconds idle before the pack is released from memory; `0` never |

A malformed file fails loudly, naming the file and the key. It never falls
back silently to a value you did not ask for. What each default was measured
against is in [`docs/search.md`](docs/search.md) and
[`docs/vocabulary-packs.md`](docs/vocabulary-packs.md).

## How it measures up

On [LongMemEval-S](docs/benchmarks.md) — 470 answerable questions over
23,854 memories, a public dataset anyone can download — recall@5 is **90.5%
with BM25 alone and 92.9% with the vocabulary pack**, counting a question by
the fraction of its evidence that lands in the top five. Counting only
questions where *every* evidence session makes the top five, 82.3% and
87.9%.

Those are retrieval numbers: whether the evidence was handed to the model,
not whether the model then answered correctly. The method, the three
definitions of recall and what none of it proves are in
[`docs/benchmarks.md`](docs/benchmarks.md).

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

## Documentation

- Installing, and connecting each agent: [`docs/installation.md`](docs/installation.md)
- How search works and how to tune it: [`docs/search.md`](docs/search.md)
- Handing work between agents, and memories received from elsewhere: [`docs/handoff.md`](docs/handoff.md)
- Vocabulary packs, and what several sessions cost in RAM: [`docs/vocabulary-packs.md`](docs/vocabulary-packs.md)
- Security: where text you did not write gets in, what is mitigated and what is not: [`docs/security.md`](docs/security.md)
- Measuring retrieval against a public dataset: [`docs/benchmarks.md`](docs/benchmarks.md)
- The export format, which is complete enough to rebuild the database elsewhere: [`docs/export-format.md`](docs/export-format.md)
- Windows, and what is still unverified there: [`docs/windows.md`](docs/windows.md)
- Compared with [supermemory](docs/comparison-with-supermemory.md), [Engram](docs/comparison-with-engram.md) and [OpenViking](docs/comparison-with-openviking.md)
- What changed in each version: [`CHANGELOG.md`](CHANGELOG.md)
- The phase 1 spec, a historical document written against the author's
  private corpus: [`docs/historia/2026-09-21-memoria-local-design.md`](docs/historia/2026-09-21-memoria-local-design.md)

## License

MIT. See [`LICENSE`](LICENSE).

You can use it, change it and ship it, including in closed-source products.
The one condition is that the copyright notice and the licence text travel
with any substantial portion of the code you redistribute.
