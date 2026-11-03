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

## What this does not measure

Retrieval, and only retrieval: recall@5, recall@50 and MRR over the session
the evidence is in. LongMemEval also scores whether an assistant answers
correctly from what it retrieved. anathem does not answer anything — it hands
candidates to a model — so that half of the benchmark is out of scope.

Session-level recall is also coarser than LongMemEval's turn-level recall. A
session here becomes one memory, with both speakers' turns in it; the dataset
marks the individual turns that carry the evidence, and that finer measure is
not used.
