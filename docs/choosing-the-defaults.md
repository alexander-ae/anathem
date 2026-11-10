# How the defaults were chosen

Two numbers in `config.json` are not arbitrary and not guesses:
`bm25_weight`, and the size of the vocabulary pack. This is the measurement
behind each, including the part where the first answer was wrong.

It is kept because the code points here. `src/config.rs` and `src/fusion.rs`
cite it for why the lexical and vector rankings are fused the way they are,
and a reader who asks "why 1.0?" deserves somewhere to land that is not a
commit message.

These are the **early** measurements, from September 2026, over a corpus of
171 memories. [`benchmarks.md`](benchmarks.md) and the `bm25_weight` section
of [`search.md`](search.md) carry the later ones, over a public dataset and
over a corpus four times larger, and where they disagree the later ones win.
Both are here on purpose: the disagreement is informative, and it is the
subject of the last section.

## What a vector layer buys, and what it costs

Measured on mMARCO: 10,000 English passages, 182 queries available in both
English and Spanish under the same ids. The `es→en` column is the case that
matters — a memory written in one language, searched in another.

| retriever | disk | en→en r@5 | es→en r@5 | es→en r@50 | indexing 10k |
|---|---|---|---|---|---|
| BM25 alone | 0 MB | 79 % | 21 % | 33 % | 0.1 s |
| potion-multilingual-128M, int8 | 128 MB | 69 % | 48 % | 76 % | 0.3 s |
| MiniLM-L12-v2 multilingual | 471 MB | 92 % | 69 % | 87 % | 12 s |

BM25 does not understand. Its cross-language hits are proper nouns and
loanwords — `hematuria`, `alexa`, `tortilla` — never comprehension. And a
reranker cannot rescue it: at 33 % recall@50 the right document is not in
the candidate set to be reranked in two queries out of three.

MiniLM is better at everything and costs 471 MB and a transformer at
runtime, which is the thing this project does not want. The static model is
the compromise, and the rest of this section is about making it small.

## How small a static model goes

It is a lookup table, not a network, so it compresses in ways a transformer
does not allow.

| variant | disk | en→en r@5 | es→en r@5 | es→en r@50 |
|---|---|---|---|---|
| fp32 | 512 MB | 69 % | 47 % | 76 % |
| fp16 | 256 MB | 69 % | 47 % | 76 % |
| int8 | 128 MB | 69 % | 48 % | 76 % |
| int8, Latin-script vocabulary only | **71 MB** | 68 % | **47 %** | **76 %** |
| int8 + 128 dimensions (PCA) | 64 MB | 61 % | 41 % | 69 % |
| int8 + 128 dim + Latin | 35 MB | 62 % | 41 % | 69 % |
| int8 + 64 dim + Latin | 18 MB | 52 % | 32 % | 56 % |

Quantizing to int8 is free. Dropping dimensions is not: it costs immediately
and proportionally. **71 MB with no measurable loss** is the knee, and
trimming the vocabulary further — to the 15 % of entries English and Spanish
actually exercise — is what takes the published pack to 22.5 MB. That part,
and why re-segmentation makes it lossless, is in
[`vocabulary-packs.md`](vocabulary-packs.md).

## Fusing the two rankings

| fusion | en→en r@5 | en→en r@50 | es→en r@5 | es→en r@50 |
|---|---|---|---|---|
| BM25 alone | 77 % | 87 % | 19 % | 31 % |
| static alone | 69 % | 86 % | 48 % | 76 % |
| RRF 1.0 / 1.0 | 77 % | 93 % | 43 % | 74 % |
| RRF 0.25 / 1.0 | 74 % | 88 % | **52 %** | 78 % |

(BM25 reimplemented in Python here, so that it could be fused: it scores
77/19 against anathem's 79/21, so the bench is faithful but weaker. That
detail matters later.)

**Equal weights make the cross-language case worse**, 48 % to 43 %. When the
query is not in the index's language BM25 contributes almost only noise, and
at equal weight that noise displaces good vector hits. At 0.25 the fusion
beats both components in the cross-language direction, for three points in
the monolingual one. No weight wins in both directions.

### A hypothesis that was tested and discarded

Deciding the weight per query, from the strength of BM25's best score, on
the theory that a query in another language would score low.

**It does not discriminate.** The median is 21.4 monolingual against 14.1
cross-language, and the distributions overlap. The adaptive scheme landed
exactly where fixed weight 1.0 did.

This is worth keeping rather than quietly dropping, because the same wall
was hit again from a different direction a month and a half later: no cheap
lexical signal separates a weak search from a strong one, measured over 98
labelled queries, with the best rule firing on 39 % of searches and being
wrong more than half the time. Two independent attempts at "let the system
notice when it is doing badly" both failed on the same thing — the
information needed to notice is the information retrieval was missing in the
first place. Anything per-query needs a signal from outside the ranking,
explicit language detection being the obvious one.

## Where the mMARCO numbers stop applying

mMARCO is web passages: lexical in shape, with queries and documents sharing
vocabulary. Personal memories are shorter, more paraphrastic and full of
identifiers. Measured over a real corpus of 171 memories, the direction held
and the magnitudes did not — and the correction went against the earlier
recommendation.

| | same language r@5 | en → corpus r@5 | pt → corpus r@5 |
|---|---|---|---|
| bm25 only | **66 %** | 34 % | 56 % |
| hybrid, weight 0.25 | 56 % | **62 %** | 48 % |
| hybrid, weight 1.0 | **66 %** | 58 % | 50 % |

At 0.25 same-language search **loses 10 points** to gain 4 across languages.
At 1.0 it ties pure BM25 in the same language, gains 24 points in English,
and lifts recall@50 from 88 % to 92 %.

**So the default is 1.0, and it used to be 0.25.** The earlier figure came
from fusing against the Python BM25, which is weaker than anathem's. Against
the real one, lowering its weight costs more than that bench predicted.

The lesson is more general than this setting, and it is the reason this
document exists at all: **a component replaced by a weaker approximation
does not only lower its own score, it moves the optimum of everything
calibrated against it.** A benchmark can be faithful on the axis it reports
and still point the wrong way on the decision it is used to make.

## And the later measurements moved it again

Over a corpus of 682 memories rather than 171, with hand-written questions,
the best weight is **8.0**, not 1.0 — and on LongMemEval it is 1.0, not 8.0.
The two disagree because the corpora disagree: prose gives a vector ranking
meaning to catch, while notes dense in tickets and flags have BM25 already
confident at rank 1, where fusing dilutes it.

That is not a third correction to the default. It is the finding that no
single default serves both shapes, which is why `mem tune` exists and why
1.0 stays the shipped value — it is where the published figures are
measured, and it suits prose. The tables are in the `bm25_weight` section of
[`search.md`](search.md).

## Isolation

Both measurements ran on copies, outside the repository, against
`~/anathem-bench/`, and the personal database was fingerprinted before and
after and left untouched.
