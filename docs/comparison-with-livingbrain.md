# anathem versus Living Brain

[Living Brain](https://livingbrain.com/) (ChatAndBuild) is a hosted memory
layer for AI tools and agents. Its headline is the same sentence anathem's
README opens with — the memory is decoupled from the model and from the
harness — which makes it the closest thing to a competitor this project has
in *argument*, and the farthest from it in *implementation*.

Everything attributed to it below comes from its own site and documentation,
read on 2026-11-15. Nothing is inferred, and where this document does not know
something it says so. The anathem figures were measured the same day and are
in [`benchmarks.md`](benchmarks.md).

## What it is

A **hosted service**. Each "brain" belongs to a subject, and everything goes
through its API: captures in (notes, links, files, transcripts, chat turns),
compiled knowledge out. There are TypeScript and Python SDKs, a `livingbrain`
CLI, and a remote MCP server exposing about 60 tools, with paste-ready setup
for Claude Code, Codex, Cursor, VS Code, Hermes, OpenClaw, ChatGPT and Claude.
There are importers for Obsidian, Claude's memory and ChatGPT's memory.

Captures are **compiled by a model** into *pages*, sorted into buckets and
linked into a graph. Four scoring policies decide each one: Memory Worthiness
(keep it?), Placement Fit (which page?), Connection Resonance (link it to
what?) and Re-entry Priority (bring it back when?). Each has named signals,
each runs its eligibility checks first, and the weights are not published.
Pages can be edited, moved, pinned, archived and versioned, and a pair of
pages that fit a fact almost equally well goes to **review** rather than being
forced onto one.

Reading is a merged semantic and keyword search over pages, `topK` 5 by
default, with a similarity floor of 0.35 that drops weak semantic matches and
keeps keyword hits below it. Results carry `matchedOn` (`both`, `semantic`,
`keyword`) and excerpts of 500 characters. A *prompt index* — a short digest
of the brain's most important pages, pinned ones first — is what an agent
loads at the start of a session, and search is for when a question needs
something specific.

Every brain reports three health scores from 0 to 100: **Freshness**,
**Readiness** and **Strain**, each with the `factors` that produced it, a
90-day history and suggestions. Prices are 8 $ per million input tokens and
0.50 $ per million output.

anathem is one 3 MB Rust binary, one SQLite file, five MCP tools, no daemon,
no model, no account and no API key, under MIT.

## Where Living Brain wins, without haggling

- **The write path does the work for you.** A capture is enough: a model
  decides what it means, which page it belongs on and what it connects to.
  anathem stores exactly what someone asked it to store and nothing else, so
  an agent that forgets to save leaves no memory behind. That is the single
  biggest practical difference and it is not in anathem's favour.
- **Every tool at once, with no local install.** ChatGPT, Claude, Cursor,
  Codex and the rest share one brain through a hosted MCP endpoint. anathem
  shares one database across agents on one machine; taking it to a second
  machine means `mem export` and `mem import` by hand.
- **A knowledge graph and a UI to see it in.** Connections between pages are
  first-class, and `living_brain_show_graph` draws them inside the chat.
  anathem's `references` are a table you read with `sqlite3`.
- **Health as a trend, with the reasons attached.** Freshness, Readiness and
  Strain over 90 days, each with its factors. anathem's `mem doctor` is a
  checklist of the current state, with no history and no trend.
- **Team features**: subjects, multi-tenancy, webhooks, change feed, a daily
  brief, Slack and Telegram. anathem is single-user by construction.
- **Post-training on your own memory**, which they describe as a second path:
  curated captures become training examples. anathem has an export format and
  nothing else.

## Where the numbers cannot be put side by side

They publish **answer accuracy**; anathem publishes **retrieval**. Their
LoCoMo figure is 92.2% — 1,420 of 1,540 questions judged correct, with GPT-5
answering, batch `f8d531ac`, dated 1 November 2026, in a **single run**, scored
by a judge that is not named. anathem answers nothing: it hands candidates to
a model and the model answers. A retrieval recall is not a worse version of
that number; it is a different axis.

What *can* be measured on the same corpus is what the model is given, and that
was measured here on 2026-11-15:

| LoCoMo, 1,536 answerable questions | Living Brain | anathem |
|---|--:|--:|
| memory context handed to the model per question | ~9.4K tokens | **1,277 tokens** (median) |
| the memory the question needed is in it | not published | **95.9%** |
| all of the evidence a multi-hop question needs | not published | **47.9%** |
| retrieval latency, median | 592 ms (their doc; 464 ms on the landing page) | **10 ms** |
| accuracy, judged | 92.2% | not measured |
| full-context baseline accuracy | not reported | not measured |

The distance to their number is now measurable from this side. A model at
GPT-5's price point reading the whole history of each conversation answers
**85.0%** of 100 of these questions; the same model with anathem's 1,200 tokens
answers 76.0%. Their 92.2% is seven points above what raw history gives at a
comparable tier, and that margin — if it survives a second run — is the most
their architecture can be adding.

Two things follow, and neither is a victory.

**Their architecture is defensible on that table.** Compiling pages is how you
answer a question whose evidence is spread over several sessions without
handing the model the sessions. anathem's 95.9% is a ceiling — the evidence is
*somewhere in the twenty candidates*, at 300 characters each — and a model
reading only those excerpts answers far less often than that. What
the two-stage protocol buys is that the model can ask for the three bodies it
needs; what it costs is a round trip the compiled page does not need.

**And their 92.2% is missing the number that would make it mean something.**
They report no accuracy for the full-context baseline, calling its 26K tokens
"a size reference only". Without it there is no way to tell whether the
architecture buys anything over simply giving GPT-5 the conversation. That is
the same gap [`benchmarks.md`](benchmarks.md) refuses to paper over on this
side, and it is not filled by ranking first "in our internal comparisons",
which names no systems compared.

Two smaller things are worth saying plainly. The landing page and their own
benchmark page disagree on latency (464 ms / 3.7 s against 592 ms / 11.2 s for
the same LoCoMo run), and the LongMemEval-S figures the landing page leads
with — 22.6× fewer tokens, 95% lower cost, **0% hallucinations** — have no
methodology page at all: no split, no model, no judge, no date. The 22.6× is
also a multiple of a 116K-token baseline, while the same comparison on LoCoMo
is 2.8×. Multiples of a baseline are not measurements of a system.

## Why anathem still does not compile pages

The temptation is obvious: their pages exist because a model writes them, and
that is the one thing this project was built to avoid. The measurement behind
that choice has not changed — over a 390-document corpus, 56% of what a system
compiling memories on its own decided to keep was noise — and the literature
has since argued the same way: a designed memory is a patch for a weaker
writer, and its ceiling is set by the model that writes it
([`research.md`](research.md)).

There is now a paper for that position, and it is not ours: arXiv 2610.41172
finds that under a competent harness, LLM-compiled structural artifacts gave
**no detectable accuracy gain** on multi-hop FRAMES, while what a compiled
navigation tree does buy is tokens — 91% fewer when the evidence suffices. A
compiled page is a supply layer, not a better retriever.

What their design gets right, and this project does not, is that **it keeps
what it should not have to be asked for**. Their answer to that is a model
deciding worthiness. anathem's answer is smaller and older: `mem pin`, and the
topic keys that make a correction replace its predecessor. Neither decides
what is worth keeping; both let a person decide once and stop repeating it.

## What was taken from it

- **Pinned memories in the greeting.** `mem pin` already existed, and it only
  protected a memory from the candidate trimming in search — it still had to
  be matched by a query. Their prompt index, pinned pages first, is the better
  half of that idea: a rule saved in March should not need to be found in
  November. The greeting now carries pinned memories first, outside the time
  window, inside the same slot count as before.
- **The cost axis.** "Correct answers per dollar, retrieval and generation
  included" is the right way to compare memory systems and anathem had no
  number on it. `tools/answer_eval.py` and the token figures above are the
  beginning of one.
- **Health as factors and history rather than one number.** Their own page
  warns that a 95 on Readiness says nothing about whether the missing page is
  the one a question needed, and that is the argument against a composite
  score. The useful part is the factor list and the trend, and `mem doctor`
  has neither yet.
- **The honesty of a "what this does not cover" section**, which their
  benchmark page has and this repository has had since `benchmarks.md`
  existed. It is worth noting when a competitor does it too.

## What was not taken

- **Compiling pages with a model.** Cost per capture, non-determinism, and a
  write path that can invent what nobody said. `mem export` would stop
  reproducing what went in.
- **A composite health score.** A number that moves is easy to watch and easy
  to trust for the wrong thing.
- **Proactive resurfacing** (their Re-entry Priority, daily brief,
  notifications). Delivering memory nobody asked for is the intervention that
  the literature finds hardest to justify: eight thousand episodes from three
  memory systems moved resolve rates by 0.4 points against no memory at all.
- **A graph maintained by a model.** The edges are cheap to store and
  expensive to be wrong about, and wrong edges are invisible.

## When to pick which

Pick **Living Brain** for a person or a team who wants memory across ChatGPT,
Claude, Cursor and Codex without installing anything or curating anything,
who is paying for tokens anyway, and who wants a graph and a dashboard to look
at. It is a better product for that job and there is no measurement here that
says otherwise.

Pick **anathem** for one developer on one machine who wants the memory in a
file they own, no account, no per-capture model call, deterministic output,
and a two-stage protocol that costs about 1,200 tokens per search. It asks
more of its user: the agent has to decide to save, and the user has to pin
what must always travel.

They answer different questions, and the honest answer to "which is better"
is that one is a service and the other is a file.
