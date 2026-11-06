# Measuring retrieval against a public dataset

anathem's numbers in the README were measured on the author's private corpus.
That is the honest way to tune a tool for its own use, and the wrong way to
let anyone else judge it: nobody can reproduce a measurement over a corpus
they cannot see.

This is the reproducible one. It uses **LongMemEval**, which asks the same
question anathem exists to answer — can the assistant find what it was told
earlier — and ships the ground truth for it.

## The dataset

[LongMemEval](https://github.com/xiaowu0162/LongMemEval), MIT licence,
© 2024 Di Wu. Each of its 500 instances is one question plus that user's chat
history, with the evidence sessions labelled. Its chat histories are built
from ShareGPT (Apache 2.0) and UltraChat (MIT).

**It is not redistributed here.** Not a line of it lives in this repository.
You download it yourself, and the licence travels with it. Download
`longmemeval_s_cleaned.json` from the Hugging Face dataset
`xiaowu0162/longmemeval-cleaned` — about 280 MB, 500 instances of roughly 40
history sessions each.

There is also `longmemeval_oracle.json`, about 15 MB, whose histories contain
only the evidence sessions. It is useful as a smoke test and useless as a
benchmark: with no distractors, recall is near 1.0 by construction.

Other benchmarks were considered and dropped. BEIR's NFCorpus is the obvious
small IR alternative, but its licence is unresolved — the BEIR authors list it
as "not reported", the source page permits academic use only, and the
underlying NutritionFacts.org material carries separate terms. A benchmark
whose terms you cannot state is not one you can ask other people to run.

## Running it

```bash
python3 tools/longmemeval_to_memories.py longmemeval_s_cleaned.json bench/
mem --db bench.db init
mem --db bench.db import --from bench/memories
mem --db bench.db eval --evalset bench/evalset.json --label "anathem bm25" \
  --out bench/bm25.json
```

With a vocabulary pack installed, `mem reindex` before the eval measures the
hybrid layer instead of BM25 alone, and the two result files are directly
comparable.

For `longmemeval_m_cleaned.json` — 2.6 GB, 460 to 490 haystack sessions per
question, 237,655 sessions in all — add `--questions N` to the converter. It
keeps N instances spread evenly through the file, with their histories
intact: the depth of one haystack is what makes -M hard, and all 500 of them
do not fit on a laptop.

**Evenly spread, not the first N.** The file is sorted by question type. Its
first 100 instances are 100% `single-session-user` and
`single-session-assistant` — the two types anathem already answers perfectly
on -S — while the file as a whole is 54% `multi-session` and
`temporal-reasoning`. A prefix measures the easy stratum and reports it as
the benchmark. With the spacing, `--questions 100` lands within three points
of the full set on every question type.

The selection is deterministic, so there is no seed to record and the same N
gives everyone the same corpus.

`--distractors FRACTION` is the other knob, and it exists for one
measurement: it keeps every evidence session and an evenly spaced share of
the rest. That is the shape of a corpus a perfect scope would leave behind,
which is what "Does narrowing the pool help?" below uses to bound what
scoping could buy. It is a bound and not a product — nothing in anathem
builds a corpus this way for a user.

## Two decisions the conversion makes

**Every question searches only its own history.** Each memory is namespaced
under its question id, in both `project` and `ext_id`, and each eval case
names that project. Pooling 500 haystacks into one database and searching all
of them would measure something harder than LongMemEval defines, and the
number would not be comparable with anything published.

The namespacing is not only about scope: LongMemEval session ids repeat across
instances, and `memories.ext_id` is unique for the whole database. Loading the
raw ids would drop every collision on import, and the dropped memory could be
the evidence.

**Abstention instances are not cases.** The 30 instances whose id ends in
`_abs` have no evidence session, because the right answer is to abstain. There
is nothing to retrieve, so counting them would drag recall down for questions
the benchmark itself excludes from retrieval scoring.

## Reading the numbers

Two things about the harness matter more than the headline.

**`recall@50` is meaningless on this dataset.** The harness asks for 50
candidates and a LongMemEval-S haystack holds around 50 sessions, so the
search returns the whole history and the evidence is always somewhere in it.
It comes out at 100% by construction. Only `recall@5` and the MRR say
anything.

**A hit means *one* evidence session, not all of them.** `mem eval` ranks by
the first expected document it finds, which is the right measure when a
question has one answer. Here 300 of the 470 questions have several evidence
sessions — up to six — and answering them needs all of them. The strict
figure has to be computed from `per_case[].returned`, and it is the one worth
quoting for `multi-session` and `temporal-reasoning`.

## The numbers, measured

LongMemEval-S, `longmemeval_s_cleaned.json`
(sha256 `d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442`):
470 answerable questions over 23,854 memories, on one laptop. The hybrid run
uses the `en-es` pack, 256 dimensions, 623,648 chunks, full coverage.

|                                   |  bm25 | hybrid |      Δ |
|-----------------------------------|------:|-------:|-------:|
| recall@5, any evidence session    | 96.0% |  96.0% |  +0.0% |
| recall@5, fraction of evidence    | 90.5% |  92.9% |  +2.4% |
| recall@5, every evidence session  | 82.3% |  87.9% |  +5.5% |
| MRR                               | 0.910 |  0.920 | +0.010 |
| p50 per query                     | 375ms |  369ms |   -6ms |

The middle row is the textbook Recall@k when a query has several relevant
documents — the mean fraction of them inside the top k — and it is the one to
quote next to anybody else's number. The other two bound it: the first is
generous, the last is what answering the question actually needs.

`mem eval` prints only the first. The other two are computed from the
`per_case` block that `mem eval --out` writes, by
`tools/eval_metrics.py`, so every table here can be recomputed rather than
taken on trust:

```bash
python3 tools/eval_metrics.py bench/evalset.json bench/bm25.json bench/hybrid.json
```

The headline says nothing: on the loose measure the two are identical. The
whole difference is in the questions that need several sessions, which is
where a second ranking has something to add.

| question type              |   n |  bm25 | hybrid |     Δ |
|----------------------------|----:|------:|-------:|------:|
| multi-session              | 121 |   66% |    83% |  +17% |
| temporal-reasoning         | 127 |   76% |    81% |   +5% |
| knowledge-update           |  72 |   96% |    99% |   +3% |
| single-session-user        |  64 |  100% |   100% |    +0% |
| single-session-assistant   |  56 |  100% |    96% |   -4% |
| single-session-preference  |  30 |   70% |    70% |    +0% |

Three things in that table are worth more than the average.

**The pack earns its 22 MB on `multi-session`**, and almost nowhere else:
+17 points. Those questions need every evidence session in the top 5, and the
vector ranking surfaces the ones whose wording does not match the question.

**It costs 4 points on `single-session-assistant`.** Both regressions are
questions that point at the conversation rather than at a topic — "I think we
discussed work from home jobs for seniors earlier", "looking back at our
previous conversation where you created two…". There is little for either
ranker to grip, and the vector half pulls in sessions that are merely
adjacent, pushing the right one past rank 5. It is a real trade, not noise.

**`single-session-preference` is the floor, and the pack does not lift it.**
It is the worst type for both at 70%, and the vector layer rescues two
questions while breaking two others: seven of the nine failures are the same
questions in both runs. Whatever is missing there, a second lexical-adjacent
ranking is not it.

## LongMemEval-M: the same question, ten times the haystack

-S gives each question about 50 sessions to search. -M gives it 460 to 490.
That is the variable that made the comparison with the paper unreadable, so
it is the one to pin down.

94 questions (`--questions 100`, six of them abstention and skipped), 47,298
memories, every question searching ~476 sessions. All 168 evidence sessions
verified present in the database before measuring.

|                                   | -S bm25 | -S hybrid | -M bm25 | -M hybrid |
|-----------------------------------|--------:|----------:|--------:|----------:|
| recall@5, any evidence session    |   96.0% |     96.0% |   88.3% |     89.4% |
| recall@5, fraction of evidence    |   90.5% |     92.9% |   78.0% |     81.6% |
| recall@5, every evidence session  |   82.3% |     87.9% |   68.1% |     73.4% |
| MRR                               |   0.910 |     0.920 |   0.819 |     0.789 |
| p50 per query                     |   375ms |     369ms |   763ms |     822ms |

**The pack earns more as the haystack deepens**: +2.4 points of fraction
recall on -S, +3.6 on -M. That is the opposite of a vanishing effect, and it
is the argument for carrying 22 MB around.

**And on -M it costs MRR while gaining recall** — 0.819 down to 0.789. More
evidence lands inside the top five and the first hit sits lower in it: RRF
fusing two rankings dilutes a confident BM25 rank-1. For a tool that hands a
model five candidates rather than one answer, that is the right side of the
trade, but it is a trade.

`recall@50` stops being a tautology here and starts meaning something: 97%,
because 50 candidates out of 476 sessions is a real cut.

| question type              |   n | -M bm25 | -M hybrid |     Δ |
|----------------------------|----:|--------:|----------:|------:|
| single-session-user        |  12 |    1.00 |      1.00 | +0.00 |
| knowledge-update           |  14 |    0.96 |      0.96 | +0.00 |
| single-session-assistant   |  12 |    1.00 |      0.92 | -0.08 |
| multi-session              |  24 |    0.67 |      0.77 | +0.10 |
| temporal-reasoning         |  26 |    0.69 |      0.72 | +0.04 |
| single-session-preference  |   6 |    0.33 |      0.50 | +0.17 |

The single-session types do not notice the extra 400 distractors at all:
still 1.00 with ten times the haystack. What degrades under depth is what was
already weak — `multi-session` and `temporal-reasoning` sit around 0.7 on
both splits.

Two rows are now reproduced on both splits, which is what makes them worth
believing. The pack buys `multi-session` (+0.17 on -S, +0.10 on -M) and it
costs `single-session-assistant` (-0.04 on -S, -0.08 on -M). The regressions
on -S were questions pointing at the conversation rather than at a topic, and
the same shape repeats here.

The preference row is the one not to quote. Six questions: a 95% interval of
roughly -0.05 to 0.71, and it moves +0.17 here while -S says +0.00 at n=30.
That is noise with a plausible-looking face on it.

## What happens as the corpus grows

The question a memory is finally judged on, and the one this document could
not answer until it was asked directly: does it still work in year three.

Measuring it needs care, because the obvious way is wrong. Raising
`--questions` does **not** grow the haystack — the converter gives every
question its own project and the eval case names it, so each question keeps
searching some fifty sessions however many questions exist. Growing that way
measures the same problem more times.

So the corpus is **pooled**: the `project` is stripped from every case and a
fixed hundred questions search the whole database. Sizes come from
`--distractors`, which keeps every evidence session and an evenly spaced
share of the rest. LongMemEval-S, bm25 only.

| memories | recall@5 | recall@50 | MRR | p50 | peak RSS |
|---:|--:|--:|--:|--:|--:|
| 1,024 | 72 % | 92 % | 0.560 | 38 ms | 17.4 MB |
| 2,520 | 68 % | 88 % | 0.528 | 78 ms | 17.4 MB |
| 6,481 | 57 % | 84 % | 0.427 | 179 ms | 17.8 MB |
| 12,267 | 48 % | 77 % | 0.392 | 333 ms | 18.2 MB |
| 23,854 | 42 % | 70 % | 0.330 | 646 ms | 18.9 MB |

Three things, and they do not all point the same way.

**Quality falls, and it is not subtle.** recall@5 goes from 72 % to 42 %
across a twenty-three-fold corpus. Whatever else is true, an undifferentiated
pile of twenty thousand memories is not something this searches well.

**Latency grows about linearly with the pool** — twenty-three times the
corpus, seventeen times the time. At a hundred thousand that extrapolates to
some 2.7 seconds, which is the point where it stops being a tool you reach
for without thinking.

**Memory does not grow.** 17.4 MB at a thousand memories and 18.9 MB at
twenty-four thousand, against a 774 MB database. SQLite stays on disk and
pages in what it needs. It is the one row of this table that can be quoted
without a caveat.

### What it does not cover

bm25 only, nothing past 23,854, and LongMemEval sessions are prose while a
working memory is technical notes — a difference this document has already
measured to matter elsewhere. And `recall@50` of 70 % on the pooled run says
the answer is usually *there* and ranked too low, which is a ranking problem
rather than a retrieval one, and a different piece of work from this.

The obvious mitigation is to search less of it. That is the next section,
which measures it twice — first as a ceiling, before there was a way to
measure a real scope, and then for real.

## Does narrowing the pool help?

`--scope` narrows a search to a project, a tag or a topic. It removes
distractors; it cannot move a document from position 30 to position 3. Those
are different halves of the same gap, and before spending a week on either it
was worth knowing which one the numbers care about.

So this was measured with its rule written down first, which is the only way
a measurement is allowed to say "no".

**The rule, fixed in advance:** if removing three distractors in four — while
keeping every evidence session — does not improve `recall@5 (fraction)` on
`multi-session` questions by at least **2 percentage points**, then narrowing
the pool is not a retrieval improvement. `--scope` stands as a convenience and
the effort goes to ranking instead.

**Why that models a scope.** The most a perfect scope could ever do is drop
distractors while keeping all the evidence. A corpus built that way bounds
what any scoping could buy, and bounds it generously: a real scope sometimes
drops evidence too.

```bash
# the same 500 instances, with every evidence session and a quarter of the rest
python3 tools/longmemeval_to_memories.py longmemeval_s_cleaned.json scoped/ --distractors 0.25
```

`--distractors` keeps the evidence and spreads its sample of the rest evenly,
with no seed to write down: two people running that command get the same
corpus. Here it took the 23,854 memories down to 6,481.

### The numbers

`recall@5` as the mean fraction of a question's evidence inside the top 5 —
the row to quote, as "Reading the numbers" explains.

| question type              |   n |  bm25 | bm25 scoped |      Δ | hybrid | hybrid scoped |      Δ |
|----------------------------|----:|------:|------------:|-------:|-------:|--------------:|-------:|
| multi-session              | 121 | 83.9% |       92.8% | **+8.9** |  92.0% |         98.0% | **+6.0** |
| temporal-reasoning         | 127 | 88.3% |       94.1% |   +5.8 |  90.4% |         96.5% |   +6.1 |
| single-session-preference  |  30 | 70.0% |       86.7% |  +16.7 |  70.0% |         93.3% |  +23.3 |
| knowledge-update           |  72 | 97.9% |       99.3% |   +1.4 |  99.3% |         99.3% |   +0.0 |
| single-session-assistant   |  56 |100.0% |      100.0% |   +0.0 |  96.4% |        100.0% |   +3.6 |
| single-session-user        |  64 |100.0% |      100.0% |   +0.0 | 100.0% |        100.0% |   +0.0 |
| **all**                    | 470 | 90.5% |       95.6% |   +5.1 |  92.9% |         98.0% |   +5.1 |

| | bm25 | bm25 scoped | hybrid | hybrid scoped |
|---|---:|---:|---:|---:|
| MRR         | 0.910 | 0.951 | 0.920 | 0.960 |
| p50 per query | 375ms |  106ms |  369ms |  109ms |

### The verdict

**The rule is met, and not narrowly.** `multi-session` gains 8.9 points on
bm25 and 6.0 on hybrid, against a threshold of 2. Narrowing the pool is a
retrieval improvement, not only a convenience, and `--scope` earns the work
that follows it. The deterministic outline waits behind it.

Three things that table is saying beyond the verdict.

**The effect is largest exactly where the gap was.** `multi-session` and
`temporal-reasoning` are the two types that need several evidence sessions
and that the README names as the weak ones; they are the two that move most.
Where the baseline already sat at 100% there is nothing to win, which is what
makes the first column worth reading.

**Latency falls to 106 ms from 375 — and this is the half of the simulation
that does not transfer.** It was once claimed here that it did, on the
reasoning that a smaller pool is faster to search. Measured since, over the
same 23,854 memories with each question confined to its own project, a real
scope is **slower**: 1,184 ms against 646 ms for the same questions pooled.

The reason is the one this section already gives for the recall half. The
simulation rebuilds the index over a smaller corpus; a real `--scope`
filters in the `WHERE` clause of the full index, so FTS5 walks further to
collect enough rows that match. Narrowing the pool buys recall, and it costs
time rather than saving it.

**`single-session-preference` is the odd one**, +16.7 and +23.3 from a
baseline of 70%. Thirty questions is a small stratum and this is the one
figure here not to lean on.

### And measured for real, it recovers all of it

The ceiling above is what a perfect scope *could* buy. This is a real one.
The same 23,854 memories as the pooled run two sections up, the same hundred
questions, each confined to its own project — which is what anathem does by itself, since every memory takes
the project of the repository it was saved from:

| | recall@5 | recall@50 | MRR | p50 |
|---|--:|--:|--:|--:|
| pooled | 42 % | 70 % | 0.330 | 646 ms |
| **scoped by project** | **95 %** | **100 %** | **0.886** | 1,184 ms |

So the degradation is a fact about the **searched pool** and not about the
database. Twenty-four thousand memories spread over twenty projects is the
top row of the first table, not the bottom one.

That reframes `--project` from bookkeeping into the scaling mechanism, and
it is the reason the default — derive the project from the repository and
never make anyone type it — matters more than it looks. It also sets the
price: scoping buys recall and spends time, 1,184 ms against 646.

### What the ceiling does not prove

These four are about the **simulation**, not about the real measurement
above it. The real one was scoped rather than modelled, so the first two do
not apply to it — it has its own limits, at the end.

**It is a bound, not a product.** Nothing was scoped: a corpus was built the
way a perfect scope would have left it. A real `--scope` is written by
someone who may name the wrong tag, and then it drops evidence instead of
distractors and recall falls rather than rises. This measures the ceiling.

**It changes two things at once.** The simulation rebuilds the index over a
smaller corpus, so both the candidate pool *and* the BM25 term statistics
change. A real `--scope` filters in the `WHERE` clause of one index, leaving
the statistics of the whole corpus in place. The ceiling is therefore not a
clean isolation of "fewer distractors": part of the gain comes from an IDF
computed over 6,481 memories instead of 23,854.

**Project scoping was already on.** Every question's haystack is its own
project here, in both runs, so what was measured is the gain from narrowing
*further inside* a project — which is what `--scope tag:` and `--scope
topic:` offer, and not what `--project` already did.

**It says nothing about ranking.** Removing distractors is not reordering.
The binding constraint — paraphrastic queries found at rank 30 — is
untouched by any of this, and remains the other half of the work.

And what the **real** measurement does not prove: it scopes by `project`
only, which is the one anathem sets by itself. `--scope tag:` and `--scope
topic:` are narrower still and were not measured directly, so the ceiling
above remains the only estimate for those. It is also one dataset of prose
sessions; a corpus of technical notes divides into projects differently, and
somebody whose whole memory is one repository gets the pooled row rather
than the scoped one.

## Against published numbers

Resist the comparison. Two gaps make most of them meaningless, and both are
about what is being measured rather than how well.

**The memory products report a different quantity.** Mem0 and Zep publish
LongMemEval scores around 93% and 71%: binary LLM-as-judge accuracy on the
*answer*. anathem never answers anything — it hands candidates to a model and
the model answers — so there is no number here that can be put beside theirs.
A retrieval recall of 92.9% is not "worse than Mem0's 93%"; it is not the same
axis. Those figures are also vendor self-reports; an independent run of Mem0's
open-source edition came out at 32.4% against the platform's 93.4%, which says
more about reproducibility in this area than about either system.

**The paper's retriever table is closer to comparable, with one unknown
left.** LongMemEval's Appendix E.1 gives session-level Recall@5 of 0.634 for
BM25, 0.723 for Contriever and 0.720 for Stella V5 1.5B. Against anathem's
BM25 on -S, 0.905, that gap was too large to take at face value, and the
appendix does not name its split while the adjacent table uses -M.

Measuring -M closes most of it. On the harder haystack anathem's BM25 is
0.780, against the appendix's 0.634 — a 15-point gap where -S showed 27. The
95% interval on 94 questions is [0.696, 0.864], so 0.634 sits outside it and
the remainder is not sampling noise. With the pack the figure is 0.816, above
the appendix's Contriever (0.723) and Stella V5 1.5B (0.720) rows as well.

What is left is one methodology difference, and it is a plausible one:
anathem scores chunks and lifts the best chunk to its memory, while a
session-level BM25 scores each whole session as one document, and term
frequency dilutes in a long one. Chunk-level retrieval beating document-level
BM25 on long documents is a known effect, and it is the kind of difference
that would survive any split.

So: anathem's retrieval is in the range of the paper's dense retrievers and
above its BM25 row, and the most likely reason is the indexing unit rather
than anything clever. That is a narrower claim than "anathem beats BM25 by 15
points", and it is the one the measurement supports.

## One head-to-head, and why its numbers are not here

Published scores from the memory products cannot be compared with these
(above), so one was run here instead: **supermemory** v0.0.8, self-hosted, 46
questions over 2,374 sessions of LongMemEval-S, scoped per question
(`containerTags` where anathem uses `project`), on the same machine with the
same metric.

**The retrieval figures it produced are not published, because the run could
not be made fair.** supermemory's LLM provider key was expired on this
machine, so its memory agent never ran and every document finalized with
`0 memories`. The half of the product that decides what a memory *is* never
executed, so a table would have put a finished anathem next to supermemory's
chunk layer and called the distance between them a result. On top of that, 46
questions is a small sample and it was v0.0.8 with v0.0.9 available. A
caveat underneath such a table does not fix it: the table is what gets
quoted, and the paragraph is what gets left behind.

What the attempt did establish does not depend on the agent, because it is a
property of the API rather than of the ranking: **supermemory returned about
1.3 documents per query, never more than four, whatever `limit` asked for,
and returned nothing at all for 19 of the 46 questions.** For the 27 where it
did answer, the evidence was among its results 89% of the time. It is not
ordering candidates badly; it abstains, and when it speaks it is usually
right.

That is a different design and not a worse one, and it is the part worth
keeping. anathem hands a model twenty candidates with a short excerpt each
and lets the model filter; supermemory hands back a few it is confident
about. Which of those is better depends on what is downstream, and this run
was in no position to say.

Anyone wanting the comparison this one failed to be should run supermemory
with its memory agent working. `mem eval` is the harness and the conversion
script is in `tools/`; neither is specific to anathem.

## What this does not measure

Retrieval, and only retrieval: recall@5, recall@50 and MRR over the session
the evidence is in. LongMemEval also scores whether an assistant answers
correctly from what it retrieved. anathem does not answer anything — it hands
candidates to a model — so that half of the benchmark is out of scope.

Session-level recall is also coarser than LongMemEval's turn-level recall. A
session here becomes one memory, with both speakers' turns in it; the dataset
marks the individual turns that carry the evidence, and that finer measure is
not used.

## Running it over your own corpus

`mem eval` is not only for this benchmark. Pointed at an eval set of your
own, it is a regression test for any future change to the ranking: the same
queries, the same corpus, before and after.

```bash
mem --db ~/.anathem/mem.db eval --evalset my-evalset.json --label "before" \
  --out before.json
```

An `expect_docs` entry names a `memories.ext_id`, or a `memories.id` with
the `id:` prefix — `"expect_docs": ["id:42"]`. The prefix is not decoration:
only memories that arrived through `migrate` or `import` carry an ext_id, so
`id:` is the only way to write a case about something anathem saved itself,
which on a corpus you grew rather than imported is all of it. `mem topics`
and `mem recent` both print the ids.

The format is the one `tools/longmemeval_to_memories.py` writes, and
`tools/validate_evalset.py` checks it before you spend an hour measuring
against it — including, with `--db`, that every `expect_docs` entry really
is in the corpus and really is `active`. A case whose evidence is not there
can never be answered, so it drags the score down for a reason that has
nothing to do with retrieval.

The figures the README quoted before this document existed — from v0.1.0 to
v0.2.2 — came from the author's private corpus, which nobody else can
reproduce. They are kept in
[`comparison-with-supermemory.md`](comparison-with-supermemory.md) and
[`comparison-with-engram.md`](comparison-with-engram.md) with that caveat
attached. This document does not replace them: it measures something
reproducible, and the gap between the two is reported in the README because
it is information rather than an embarrassment.
