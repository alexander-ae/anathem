# anathem versus OpenViking

[OpenViking](https://github.com/volcengine/OpenViking) is the project anathem
will be compared against from now on, so the answer should be written down
rather than improvised. It is an open-source context database from Volcano
Engine, ByteDance's cloud arm, with 41.6k stars on GitHub.

Everything attributed to it here comes from its own README and documentation,
read on 2026-11-05. Nothing is inferred, and where this document does not
know something it says so.

## What it is

OpenViking "organizes context as a virtual filesystem under `viking://`" and
unifies three things an agent needs — memory, knowledge (RAG) and skills —
under one store. Its context is layered in three tiers: **L0** (Abstract, "a
one-sentence summary for quick relevance checks"), **L1** (Overview) and
**L2** (Details), so an agent can judge whether a file is worth loading
before loading it.

Sessions are archived on commit: "Committing a session archives the
conversation and extracts memories as Markdown you can inspect, edit, and
merge", processed in the background. It ships Python, Go and TypeScript SDKs
plus an HTTP API, a self-hostable Web Studio and a hosted demo, account and
user isolation, and opt-in resource ACLs.

Running it yourself needs uv, Python 3.10+ and a model provider with **an
embedding model and a VLM**; its `init` wizard offers Volcengine, OpenAI,
Codex OAuth, Kimi, GLM and local Ollama. The main project is AGPLv3, with the
CLI crate and examples under Apache 2.0 and some adapters under MIT.

anathem is one 3 MB Rust binary, one SQLite file, five MCP tools, no daemon,
no model and no API key, under MIT.

## Where OpenViking wins, without haggling

- **RAG and memory in one index.** anathem stores memories. Documents,
  codebases and web pages are not its subject, and bolting them on would be a
  different program.
- **Context scoped to a directory**, with summaries at each level, so a large
  corpus can be navigated instead of searched blind. anathem narrows by
  project, tag and topic rather than by path (`--scope`), which is the
  smaller version of the same idea — and it is worth having: measured here,
  narrowing the pool is a recall improvement and not only a convenience
  ([`benchmarks.md`](benchmarks.md)). What it does not have is the
  navigable hierarchy, or summaries to navigate it by.
- **Skills as first-class context.** anathem has no notion of a skill.
- **Inspection through a Web Studio.** anathem has a CLI and a database you
  can open with `sqlite3`.
- **Multi-tenancy, ACLs and SDKs in three languages.** anathem is
  single-user by construction: the database is yours, `0600`, on your disk.
- **A hosted option.** anathem has none and will not have one.

If the shape of the problem is "a team, a large corpus, several agents, an
inspectable service", that is OpenViking's problem and not anathem's.

## Where anathem wins, with the numbers that exist

- **Zero tokens and zero models to write a memory.** `mem save` is a SQLite
  insert. OpenViking's extraction path runs a VLM; its own memory evaluation
  used Doubao 2.0 Pro with Doubao-embedding-vision.
- **It installs without asking anyone for an API key.** One binary, one
  database, a 22 MB vocabulary pack if you want the vector layer. No Python,
  no uv, no provider.
- **About 9 MB resident at rest**, 12 MB after idling, and 5–8 ms per query
  on a laptop. The figures and the method are in the README.
- **Corrections are auditable rather than decided by a model.** A topic key
  or an explicit `supersedes` replaces a memory, the old one stays in the
  history, and `mem history` and `mem show --topic` show the chain. Nothing
  merges two memories on your behalf.
- **A reproducible retrieval benchmark**: LongMemEval-S, 470 answerable
  questions over 23,854 memories, recall@5 of 92.9% as the mean fraction of
  evidence in the top 5, MRR 0.920 — see [`benchmarks.md`](benchmarks.md) for
  the three definitions of recall and why that is the row to quote.
- **The en-es pack earns its size on cross-lingual search**, which is the
  case it exists for.
- **MIT.** OpenViking's core is AGPLv3.

## Why the benchmarks cannot be put side by side

They do not measure the same thing, and putting the numbers in one table
would be the kind of comparison this document exists to avoid.

OpenViking publishes **answer accuracy**: on LoCoMo, 82.08% with OpenClaw
against 24.20% native, 82.86% with Hermes against 33.38%, 80.32% with Claude
Code against 57.21%; on tau2-bench, 77.81% on Retail and 66.25% on Airline.
Those are end-to-end figures for a whole system — retrieval, a VLM, and the
agent answering — measured with Doubao 2.0 Pro and Doubao-embedding-vision.

anathem publishes **retrieval**: recall@5 and MRR on LongMemEval-S, which ask
whether the evidence was handed to the model, and say nothing about whether
the model then answered correctly.

One measures the system; the other measures one layer of it. A higher number
on either side would not mean what putting them in one row would suggest.

**No head-to-head has been run.** Running one would mean standing up
OpenViking with an embedding model and a VLM, and picking an agent and a
dataset that both can be scored on. Until that exists, this document makes no
performance claim about the two together, and neither should anything that
cites it.

## Non-goals, and why

These are not missing features. They are decisions, and this is where they
live so that a later proposal has to argue against the reason rather than
rediscover it.

**Automatic extraction of memories at the end of a session.** Measured here,
against supermemory's extraction on the same corpus: 56% of what it saved was
noise. A memory worth keeping is one someone decided to keep. The cost of
getting this wrong is not a missing memory — it is a corpus that fills with
plausible rubbish and makes every search worse.

**L0/L1 summaries generated by a model.** Two reasons, one of principle and
one measured. The principle: a generated summary costs tokens per note and
stops memory being deterministic, so `mem export` would no longer reproduce
what went in. The measurement: anathem has a **deterministic** outline — the
first paragraph and the markdown headings, read off the body at search time,
no model — and over LongMemEval-S, 98.3% of its 23,854 memories are long
enough to carry one while only 1.5% of those have a single heading. Even the
free version of this tier has nothing to say about a corpus of conversations.
It ships off by default, behind `outline` in `config.json`.

**A skills subsystem.** A different product. anathem stores what was learned,
not what to do.

**Multi-tenancy and ACLs.** The access control is the filesystem: the
database is `0600` and it is yours. Adding users would mean adding a server,
and the absence of a server is the point.

## When to pick which

Pick **OpenViking** for a team, a large mixed corpus of documents and
conversations, several agents sharing context, inspection through a web UI,
or any case where a hosted service and an API key are not obstacles.

Pick **anathem** for one person, one laptop, memories rather than documents,
an agent that must not spend tokens to remember, a machine with no API key,
or a corpus you want to keep auditable and reproducible by hand.

They are not the same tool, and the honest answer to "which is better" is
that they answer different questions.
