# anathem versus Engram

[Engram](https://github.com/Gentleman-Programming/engram) is the closest thing
to anathem that exists: a Go binary, SQLite with FTS5, no daemon, no Node, no
Python, no Docker, an MCP server for any agent. The same underlying decisions,
taken separately.

And as a product it is further along: v3.4.0, releases for Linux, macOS and
Windows on both architectures, installation via Homebrew, a TUI, an HTTP API,
a CLI, and 23 MCP tools against anathem's 5.

So the honest question is whether anathem has any reason to exist. This
measures it.

## The benchmark

The same 449 memories and the same 50 queries on both systems. The memories
were imported into Engram with its own `engram save`, one by one, and queried
with `engram search`. The queries are conversational on purpose: they describe
the situation instead of naming it, which is how a person asks.

| | returned | ms | tokens | r@1 | r@5 | r@20 | paraphrastic @20 |
|---|---|---|---|---|---|---|---|
| **anathem** | 20 | **8** | **1,303** | 28 % | **52 %** | **74 %** | **31 %** |
| Engram `--match all` (the default) | 0 | 427 | 28 | 4 % | 4 % | 4 % | 0 % |
| Engram `--match any` | 20 | 434 | 2,475 | 0 % | 0 % | 0 % | 0 % |
| Engram `all`, without stopwords | 0 | 433 | 25 | 6 % | 6 % | 6 % | 0 % |
| Engram `any`, without stopwords | 20 | 443 | 2,458 | **30 %** | 40 % | 54 % | 25 % |

## What it says

**Engram is not meant for conversational queries.** With its default mode —
`--match all`, which requires every word — a ten-word question returns
nothing: 4 % recall, because only the two shortest queries land. With
`--match any` it returns twenty results, but it latches onto the stopwords
(`de`, `el`, `que`) and orders by something that in practice looks like
recency: 0 % recall, the twenty are the newest memories.

**Its best configuration requires preprocessing the query.** Stripping the
stopwords by hand — something Engram does not do — takes it up to 30 % at r@1,
which is **better than anathem** (28 %). That is a real result and it has to
be said.

**But it falls off with depth.** At r@5 anathem is 12 points ahead, at r@20 it
is 20 ahead. And on the paraphrastic queries, 31 % against 25 %.

**And it costs twice the tokens.** 2,458 against 1,303 per query, because it
returns the full body of each result instead of an excerpt.

**Latency is the biggest difference, and it is real.** I measured the startup
of the Engram binary separately: 2 ms. So those 435 ms are search, not
startup — the comparison is not biased by the method. anathem answers in 8 ms
with a warm server and 108 ms cold (including loading the 22.5 MB pack).
Engram is some 54 times slower per search.

**And it does not search across languages.** That is what anathem adds with
the optional pack, and what none of the thirteen memory servers compared
documents.

## When to pick which

**Engram**, if you want something that is already finished and you do not mind
phrasing things with keywords instead of questions. It installs with Homebrew,
it has a TUI, an HTTP API, and many more MCP tools. For a single-language
corpus queried with concrete nouns, its r@1 is competitive.

**anathem**, if you ask in prose, if your corpus mixes languages, or if you
care about token cost and latency. All three are measured above.

## The caveats

- anathem's figures here (74 % at r@20) are lower than those in
  `medicion-corpus-real.md` (82 %), because the corpus grew from 174 to 449
  memories. More distractors, same question.
- The stopword removal I did myself, with a list of my own. Engram might
  perform differently with other preprocessing.
- I imported the memories with `engram save` title by title; if its normal
  ingestion — over MCP, from an agent — indexes differently, the numbers could
  change.
- n=50, with 16 paraphrastic. Differences of a few points are noise; of twenty
  points, they are not.
