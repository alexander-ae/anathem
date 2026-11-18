# Measuring retrieval against a public dataset

anathem's numbers in the README were measured on the author's private corpus.
That is the honest way to tune a tool for its own use, and the wrong way to
let anyone else judge it: nobody can reproduce a measurement over a corpus
they cannot see.

This is the reproducible one. It uses **LongMemEval**, which asks the same
question anathem exists to answer — can the assistant find what it was told
earlier — and ships the ground truth for it. A second corpus, **LoCoMo**,
follows below: a different shape of history, a different licence, and a
result that does not flatter the same choices.

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
LoCoMo is the one that survived, and its terms are stated in its own section.

## Running it

Two measurements come out of the same eval set, and they answer different
questions. The default one ranks: fifty candidates, an unlimited token budget
and no excerpt, because trimming the payload would falsify the figures.

`--as-mcp` asks the other question — whether the memory the answer needed is
among the candidates `search_memory` would actually have shown, at the
`candidates`, `excerpt` and `budget` in `config.json`:

```bash
mem --db bench.db eval --evalset bench/evalset.json --label "30x80" --as-mcp
```

It prints `found` (the expected memory is among the returned candidates),
`first` (it came first) and `tokens(p50)`, counted the same way `search_log`
counts it. It is the harness for deciding whether to trade fewer long
excerpts for more short ones; the ranking numbers and these do not have to
agree, and when they disagree the payload is the one a person sees.


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

The p50 row predates the scoped-search fix: the same command on the same
corpus now reports **110 ms** for bm25, with the recall columns unchanged to
the case. Hybrid was not re-run after the fix, so its 369 ms is still the old
figure.

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

## A second corpus: LoCoMo

Everything above is one dataset, and someone could reasonably ask how much of
it is a fact about anathem and how much is a fact about LongMemEval's shape:
fifty sessions per question, each a block of prose. So the same questions were
asked of **LoCoMo**, the benchmark from *Evaluating Very Long-Term
Conversational Memory of LLM Agents* (Maharana et al., ACL 2024).

It is a different animal. Ten conversations between two people, each running
across 19 to 32 sessions and 369 to 689 turns, with 1,540 questions written
against them, and the evidence marked **per turn** (`D12:3`) rather than per
session. Its questions are split into multi-hop (282), temporal (321),
open-domain (92) and single-hop (841) — the same 1,540-question, four-category
subset the vendors quote, since its fifth category is adversarial and has no
correct answer.

**Its licence is stricter than LongMemEval's: CC BY-NC 4.0.** The figures here
can be published with attribution; the corpus cannot travel with them. Nothing
of it is in this repository, and `tools/locomo_to_memories.py` reads a file you
download yourself. Measured on `data/locomo10.json`, sha256
`79fa87e90f04081343b8c8debecb80a9a6842b76a7aa537dc9fdf651ea698ff4`,
on 2026-11-15.

Four of the 1,540 questions carry no evidence and are not cases. Six more
need care with their evidence field: four pack several ids into one string
(`["D8:6; D9:17"]`, `["D9:1 D4:4 D4:6"]`) and two carry a token that is not an
id (`D`, `D:11:26`). The converter splits on `;` and spaces and reports the two
malformed tokens, because reading the field as it comes would have dropped the
second half of those questions' evidence and blamed the ranking for it.

### Two units, and they are not interchangeable

`--unit session` puts one memory per session: 272 memories, about 27
candidates per question, which is what a memory is in anathem and what the
LongMemEval conversion does. `--unit turn` puts one per dialog turn: 5,882
memories and around 600 candidates per question, which is the setting the
paper's own retrieval evaluation uses, since its context ids are dia_ids.

Both are honest. Quoting one against a number measured the other way is how a
benchmark gets misread, so both are here, in order. And the same warning as on
LongMemEval-S applies to `recall@50` at session level: twenty-seven sessions
per question and fifty candidates asked for, so 100% is arithmetic and not
retrieval.

| | session bm25 | session hybrid | turn bm25 | turn hybrid |
|---|--:|--:|--:|--:|
| recall@5, any evidence | 88.8% | 86.7% | 52.2% | 49.2% |
| recall@5, fraction of evidence | 82.6% | 80.4% | 47.2% | 43.7% |
| recall@5, every evidence | 77.2% | 74.6% | 43.3% | 39.6% |
| recall@50, any evidence | 100% | 100% | 76.8% | 79.2% |
| MRR | 0.761 | 0.713 | 0.409 | 0.389 |
| p50 per query | 10 ms | 11 ms | 15 ms | 18 ms |

recall@5 by question type, as the mean fraction of the evidence, bm25:

| question type | n | session | turn |
|---|--:|--:|--:|
| single-hop | 841 | 95.2% | 57.0% |
| temporal | 321 | 86.0% | 56.4% |
| multi-hop | 282 | **49.6%** | **16.3%** |
| open-domain | 92 | 56.7% | 20.6% |

### What those four columns say

**The granularity decides the number, not the ranker.** Same corpus, same
1,536 questions, same binary: 88.8% or 52.2% depending on whether a memory is
a session or a turn. A note someone wrote is already an aggregation, and that
aggregation is half of what answering a multi-hop question needs. Turn-level
retrieval is the harder and more honest measure of a *retriever*; session-level
is the one that describes a *memory*, which is what anathem is.

**Multi-hop is the floor at both granularities, and the second piece of
evidence is what goes missing.** At session level, 89% of questions have
*some* evidence in the top 5 and only 25% of the multi-hop ones have *all* of
it; at turn level, 4.6% do. This is the same shape as LongMemEval's
`multi-session` row, reproduced on a dataset built by somebody else, and it
says the binding constraint is assembling several places into one answer
rather than finding any one of them.

**The pack costs here, and that is worth publishing.** On LongMemEval the
vector layer bought +2.4 points of fraction recall on -S and +3.6 on -M. On
LoCoMo it costs 2.2 points at session level and 3.5 at turn level, and takes
MRR down with it. The regression is the one already reported for
`single-session-assistant`: a conversation is full of turns that are *about*
the same thing without saying it, the vector half pulls those in, and RRF
dilutes a BM25 rank-1 that was already right. Three datasets now show the same
shape, which makes it a property of the fusion and not of one corpus. It does
not make the pack a bad default — it makes it a per-corpus decision, and
`mem tune` is the instrument for that decision.

**And the 20-candidate protocol covers this corpus almost completely.**
`mem eval` measures the ranking; what a model actually receives is
`search_memory`, and `mem eval --as-mcp` measures that: same twenty
candidates, 120-character excerpts, 1,200-token budget.

| | session | turn |
|---|--:|--:|
| the expected memory is among the candidates | 95.9% | 67.0% |
| it came first | 65.6% | 30.6% |
| tokens spent, median | 1,152 | 1,161 |

The first row is nearly tautological at session level and should be read that
way: twelve candidates out of the twenty-seven a conversation has, at the
shipped 300-character excerpt. What it says is that the payload is not where
this dataset is lost. Counted strictly
— every expected session among the candidates, which is what a multi-hop
answer needs — it is **94.5%** for the same 1,536 questions; the binary
reports the loose count, and the strict one was computed from the same cases.

That remains a ceiling and not an accuracy: what the model sees is 300
characters per candidate, and reading the fact itself costs a `get_memory`
call.

### Do the answers get better: five arms, one model

Everything above measures whether the evidence reached the model. It cannot
measure whether the model then answered, and that is the axis on which every
memory product publishes a number. `tools/answer_eval.py` measures it here:
200 questions sampled evenly inside each type, **one model answering every
arm** so the difference is the memory and not the model, judged by a second
model, with token F1 against the reference answer as a second opinion.

| arm | answered | F1 | multi-hop | temporal | single-hop | open-domain |
|---|--:|--:|--:|--:|--:|--:|
| the whole conversation in the prompt (17,837 tokens) | **78.8%** | 63.7 | 61.1% | 68.3% | 93.6% | 33.3% |
| `search_memory`, and the agent decides whether to open bodies | **68.0%** | 53.1 | 36.1% | 70.7% | 80.6% | 41.7% |
| `search_memory`, then three bodies opened for it | 62.6% | 50.4 | 37.8% | 58.5% | 70.1% | 41.7% |
| `search_memory`, and nothing else | 45.5% | 38.5 | 27.8% | 54.8% | 48.6% | 36.4% |
| no memory at all | 0.0% | 0.6 | — | — | — | — |

`gpt-6-luna` answering, `gpt-6.1-sol` judging, n=200, ±6-7 points, on the
shipped payload of 20 candidates at 300 characters. The last three are the
protocol's own stages and the distance between them is the finding: **an
agent that reads the excerpts and answers from them gets 46% of the answers
right, opening three bodies gets 63%, and letting the model decide when to
open — up to six — gets 68%.**

**The profile does not depend on the model; the level does.** The same five
arms, the same 200 questions, with a model from a different family and a
different price sheet (`deepseek-flash`, judged by the same `gpt-6.1-sol`):

| arm | `gpt-6-luna` | `deepseek-flash` |
|---|--:|--:|
| the whole conversation | 78.8% | 79.4% |
| `search_memory`, agent decides | **68.0%** | **68.5%** |
| three bodies opened for it | 62.6% | 65.0% |
| excerpts only | 45.5% | 44.4% |
| no memory | 0.0% | 0.0% |

Two families, two price sheets, and the numbers land within 2.4 points of each
other in every arm. The distance between full context and the protocol is
10.8 points with one and 10.9 with the other, and 9.0 with `gpt-6.1-terra` at
a higher tier. That is what makes any of these figures quotable: what they
measure is the protocol, not the model that happened to answer.

That order is worth pausing on. The arm where the *model* decides between the
two stages beats the arm where the harness decides for it, by 5.4 points. The
two-stage protocol is not something to impose on an agent; it is something to
offer it, with the decision left where the information is.

The memory that was needed was among the candidates 95.9% of the time in all
of these. What fails is the step between, and the next table is why.

**The payload change had a price, and it is in the same table.** With 300
characters per candidate the budget fits twelve of them instead of twenty, so
the payload that answers more questions reaches fewer memories: counting
strictly, every piece of evidence a multi-hop question needs is among the
candidates for **47.9%** of multi-hop questions where 120 characters reached
82.3%. Across all 1,536 questions the strict figure is 85.8% against 94.5%. So the
change cost 8.7 points of strict coverage — 34 of them in multi-hop — and
bought 18 points of answers. That trade is worth taking and worth naming: it
is also why multi-hop stays the floor at 27.8% in the one-shot arm.

| shape of the payload | tokens | the needed memory is among them | answered |
|---|--:|--:|--:|
| 20 × 120 (the old default, forced three reads) | 1,195 | 98.6% | 44.7% |
| 15 × 200 | 1,197 | 97.5% | 58.0% |
| 10 × 300 | 1,048 | 95.4% | 61.9% |
| 20 × 300, budget 1,200 (**the default now**, forced three reads) | 1,152 | 95.9% | **62.6%** |
| 20 × 300, budget 1,600 | 1,571 | 97.5% | 64.5% |

At a fixed budget, the length of the excerpt does the work and the number of
candidates barely matters: three points of "the needed memory is among them"
bought eighteen points of answers. That is why `excerpt` moved to 300 and the
budget did not move at all — the last two rows are the same shape at two
budgets, and 400 more tokens bought 1.9 points, inside the noise. That change
is the one this measurement produced.

**Two things were built, measured and left out.** The first was a graduated
payload: 300 characters for the top five candidates and 120 for the other
fifteen, so that twenty of them fit in the same budget. It did what it was
supposed to do and it did not help — strict coverage rose from 85.8% to 91.1%
(all of it, 47.9% to 69.1% on multi-hop) and the answers *fell*, from 68.0% to
65.3%. An agent decides what to open from what it reads, and fifteen
candidates that read like the old payload pull it in the wrong direction: the
quality of each candidate matters more than how many there are. The code is
not in the binary; the measurement is here.

**Multi-hop was attacked four times and did not move.** The arm where the
model decides was also given the ability to search again with different words,
up to twice, and then told to use it when a question needs facts from several
conversations. Neither helped:

| arm | answered | multi-hop |
|---|--:|--:|
| agent, decides whether to open bodies | 68.0% | 36.1% |
| the same, and it may search again | 67.8% | 41.7% |
| the same, and it is told to for multi-fact questions | 65.5% | 32.4% |

Multi-hop is 37 questions in this sample, so a ±26-point interval covers all
three rows: what the table says is that nothing here is signal, not that the
hint does damage. Together with the query expansion and the greedy coverage
selection that were measured and rejected in the same week, it settles the
question for this design: multi-hop does not improve by asking the same
retriever twice, by reformatting what it returns, or by instructing the model
harder. The published methods that do improve it put a *model* at query time —
a decision model in SAG2, extracted evidence requirements in EviCover — and
that component is the one this project exists without.

**And so was the other thing:** a sentence at the end of the
result asking the agent to open the body when the answer depends on it. It
changed nothing — 68.0% with it, 69.0% without it — and it cost thirty tokens
on every search, so it is not in the payload. The tool description already
says it, and evidently one telling is enough.

**And full context still wins, at every model tried.** At `gpt-6-luna` the
whole conversation is 78.8% against 68.0%; at `gpt-6.1-terra` — a model at
GPT-5's price point, 50 questions, forced three reads — it is 88.0% against
69.4%. The gap is
about the same at both, so it is not a fact about the model: it is what one
kilobyte of excerpts costs against eighteen kilobytes of everything. A memory
layer that compiles what it read into a page, and hands over the page, is
spending tokens in the other place — and this measurement is what that design
is buying.

**One of those points came from a defect worth naming.** The first run of
this experiment scored 29.3% on temporal questions in its best arm. The cause
was not retrieval: the conversion had put each session's date in the
frontmatter, where `created` lives, and `get_memory` returns the body. The
model never saw a single date, on any arm, for questions that ask *when*.
Putting the date in the body took the full-context arm from 29.3% to 68.3%
without changing anything else. anathem has the same shape of hole today: its
candidates and its bodies reach the model without their date.

### What this section does not show

It measures retrieval, and (above) answer accuracy for one cheap model and one
judge. Neither is a verdict on whether a memory is *useful*: the answers are
judged against a reference string, which rewards the fact and not the
judgement around it.

Session-level hits are also coarser than the dataset's own definition: the
evidence is marked per turn, and a hit at session level says the right 600-turn
block was found, not the right sentence. And 27 candidates per question is a
small pool, which is why the session row should not be read as "anathem is at
88.8% on LoCoMo" without the unit attached.

The answer arms have their own limits, and they are listed here rather than
under the tables: the answering model is a cheap one, so the absolute numbers
are low and a GPT-5-class model would lift all four arms; `cards+` reads at
most three bodies, which is a choice of the harness and not of the binary; and
the judge is a model, with a model's opinion of what "the same thing" means.
Token F1 is printed next to it for that reason, and the two agree in ordering
in every table above.

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

## The supermemory head-to-head, and why its numbers are not here

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

## A second head-to-head: Engram

This one produced numbers that can be quoted, and the difference from the
supermemory attempt is method rather than luck. [Engram][engram] is the other
local-first memory for agents (Go, SQLite, FTS5, MCP), so both systems could be
fed the *same* public corpora and asked the *same* questions under the same
metric: session-level recall@5 and MRR.

Engram has no evaluation harness of its own. Its memories were imported from
the markdown corpus with `engram import` — one JSON backup, not one `save` per
memory — and its searches went through its HTTP API, one request per question,
scoped to the same project anathem scopes to. The returned observation ids were
mapped back to session ids by SHA-256 of the content, because Engram reassigns
ids on import and collapses word-for-word duplicates (5,390 of LongMemEval's
23,854 sessions). A hit on the surviving copy counts for every session whose
text it carries.

| corpus | memories | anathem r@5 | Engram r@5 | anathem MRR | Engram MRR |
|---|---:|---:|---:|---:|---:|
| LongMemEval-S | 23,854 | **96.2 %** | 80.2 % | **0.912** | 0.755 |
| LoCoMo, session unit | 272 | **88.8 %** | 81.1 % | **0.761** | 0.684 |
| LoCoMo, turn unit | 5,882 | **52.2 %** | 50.3 % | **0.409** | 0.392 |

**One number in that table is not like the others, and it is Engram's caveat
rather than a footnote: its 80.2 % is the best of four configurations, and it
requires the queries to be stripped of stopwords first.** Its default
`--match all` requires every word of the question, so a normally phrased
question returns nothing — 96 to 100 % of the questions, in every corpus — and
`--match any` alone recovers only 17.7 %. The same engine, asked in keywords,
is a real competitor. Which of those two numbers is "Engram" is a question
about how an agent phrases a search, and this repository's answer is that the
default matters because it is what an agent does.

**Latency is not compared, and the reason is on this side.** A repeat of the
LongMemEval run measured a p50 of 2,537 ms for anathem against the 375 ms
reported above, on the same corpus, with the search scoped the same way. That
discrepancy is unresolved, so no latency claim is drawn from this run in either
direction; Engram answered in 4 to 294 ms depending on configuration.

The full sweep of the four configurations, the per-question-type split and the
caveats — the stopword list is the author's, and Engram's normal ingestion is
an agent-written summary rather than a raw session — are in
[`comparison-with-engram.md`](comparison-with-engram.md).

[engram]: https://github.com/Gentleman-Programming/engram

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

## What the candidates × excerpt trade-off measured

LongMemEval-S, 470 questions, 23,854 memories, BM25 without a vocabulary pack,
`budget: 1200`. Four combinations, and the rule was fixed before the run:
keep the default unless something beats it by more than two points without
spending more tokens.

| candidates | excerpt | found | first | tokens (p50) |
|---:|---:|---:|---:|---:|
| 20 | 120 | 98.9% | 87.2% | 1197 |
| 30 | 80 | **99.6%** | 87.2% | 1196 |
| 40 | 60 | **99.6%** | 87.2% | 1166 |
| 30 | 120 | 98.9% | 87.2% | 1197 |

`found` is whether the memory the question needed is among the candidates
`search_memory` would have shown — *any* of its evidence sessions, which is
the loose reading. `first` is whether it came first.

Three things come out of it:

- **`candidates` on its own does nothing above what the budget allows.** The
  30 × 120 row is identical to 20 × 120 to the token, because the 1,200-token
  budget trims the list *before* the candidate count applies. `config.rs` says
  so; this is the measurement that shows it.
- **What buys more candidates is a shorter excerpt**, and the gain is real but
  small: +0.7 points, with `first` unmoved. The rule said two points, so **the
  default stays 20 × 120**.
- **The ceiling was already saturated.** With twenty candidates the needed
  memory is there 98.9% of the time, so at most 1.1 points were available. The
  payload trim is not what loses memories: what loses them is upstream, in the
  ranking and in covering *every* evidence session — the 82.3% strict figure,
  which is a different question and not what this table measures.

After the run the corpus was deleted; what stays is the harness that produced
it (`mem eval --as-mcp`) and this table.
