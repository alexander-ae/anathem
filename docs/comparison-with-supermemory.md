# anathem versus supermemory

supermemory is what anathem was written to replace, so this is the
comparison with the most history behind it and the least neutrality: the
author migrated away from it, and these numbers are the reason.

They are also the oldest. They were measured in September over a corpus
nobody else can see, with a self-hosted supermemory v0.0.8. For the
comparison anyone can reproduce — a public dataset, the same metric for both
and every caveat spelled out — see [`benchmarks.md`](benchmarks.md), which
is the one to quote.

## What was wrong with it


supermemory ran 2 GB of RSS at rest (PGlite + ONNX + Rivet) for 390
documents, 56% of which was noise — raw turns and conversations competing
for the same 5 injection slots alongside the curated memory. The embedder it
had configured is English-only and **atomizes and normalizes the text into
English before storing it**, so a query in Spanish shares neither a lexical
nor a semantic anchor with the stored fact. And corrections piled up: the
old version stayed active next to the new one.

Measured over the same 50 Spanish queries and the same corpus (ground truth
restricted to 171 curated documents — see the caveats below):

```
                   recall@5   MRR     p50
anathem (BM25)        66%     0.537   5.4 ms
supermemory           24%     0.120   47.2 ms
```

**The second row does not mean what it looks like.** supermemory's search
returns about one document per query and never more than four, whatever
`limit` you ask it for. It is not ranking the right answer low; it declines
to answer. Measured again in November over a public corpus, it returned
nothing at all for 19 of 46 questions — and for the 27 where it did answer,
the evidence was among its results 89% of the time. A `recall@50` for it
would be a fact about its API's ceiling and not about its ranking, which is
why that column is not in this table. The two tools make different bets:
anathem hands a model twenty candidates and lets the model filter,
supermemory hands back a few it is confident about.

These figures are also from September, over a corpus nobody else can see.
For the same comparison on a public dataset, with the same corpus and the
same metric for both and every caveat spelled out, see
[`docs/benchmarks.md`](benchmarks.md).

The average hides the real finding anyway — stratified by query type:

```
                lexical (n=34)                  paraphrastic (n=16)
anathem         recall@5  88%  recall@50 100%   recall@5  19%  recall@50 62%
supermemory     recall@5  35%                   recall@5   0%
```

BM25 on its own completely solves the lexical queries (identifiers, exact
jargon). On the paraphrastic ones it finds the right document 62% of the
time (recall@50) but only puts it in the top 5 19% of the time (recall@5) —
a **ranking** problem for the ones it does find (the lever is a reranker)
and a **retrieval** problem for the ones that never show up in the top 50
(the lever is a vector layer or query expansion). supermemory surfaced
none of the 16 paraphrastic queries, though with at most four results per
query it had little room to.


## Where this leaves it

The two tools make different bets, and neither is obviously wrong. anathem
hands a model twenty candidates with a short excerpt each and lets the model
filter; supermemory hands back a few it is confident about, with a VLM and
an embedding stack behind the decision.

What made it the wrong bet *here* was not the ranking. It was 2 GB resident
for 390 documents on a laptop that also runs the editor, an embedder that
normalises Spanish into English before storing it, and a write path that
decided on its own what was worth keeping — and kept noise 56% of the time.

The other two comparisons in this repository are with tools that are closer
in shape: [Engram](comparison-with-engram.md), which took the same
decisions separately, and [OpenViking](comparison-with-openviking.md), which
takes the opposite ones at a much larger scale.
