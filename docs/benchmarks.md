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

For `longmemeval_m_cleaned.json` — 2.6 GB, around 500 haystack sessions per
question — add `--questions N` to the converter. It keeps the first N
instances with their full histories, which is the part that makes -M hard;
all 500 of them do not fit on a laptop. A prefix and not a random sample, so
there is no seed to record and the same N gives everyone the same corpus.

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

**The paper's retriever table is not confirmed to be this split.** LongMemEval's
Appendix E.1 gives session-level Recall@5 of 0.634 for BM25, 0.723 for
Contriever and 0.720 for Stella V5 1.5B, against 0.905 measured here for BM25
alone. That gap is too large to take at face value: the appendix does not name
its split, the adjacent table uses LongMemEval-**M**, and -M gives every
question roughly 500 sessions of haystack where -S gives about 50. Retrieval
over ten times the distractors is a different problem.

There is a second candidate explanation that would be real rather than an
artefact: anathem indexes chunks and lifts the best chunk to its memory, while
a plain session-level BM25 scores each whole session as one document, and term
frequency dilutes in a long one. Chunk-level retrieval beating document-level
BM25 on long documents is a known effect.

Both explanations fit, and this measurement cannot separate them. Running
LongMemEval-M (`longmemeval_m_cleaned.json`, 2.7 GB) would: it is the harder
haystack, and if the gap survives there the architecture is doing the work.
Until someone does that, the honest claim is the narrow one — the pack buys
2.4 points of recall over BM25 on -S, and the two sit in the same range as the
retrievers the paper tried.

## What this does not measure

Retrieval, and only retrieval: recall@5, recall@50 and MRR over the session
the evidence is in. LongMemEval also scores whether an assistant answers
correctly from what it retrieved. anathem does not answer anything — it hands
candidates to a model — so that half of the benchmark is out of scope.

Session-level recall is also coarser than LongMemEval's turn-level recall. A
session here becomes one memory, with both speakers' turns in it; the dataset
marks the individual turns that carry the evidence, and that finer measure is
not used.
