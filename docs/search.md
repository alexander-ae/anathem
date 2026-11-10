# How search works, and how to tune it

Search is BM25 over chunks, filtered to `status = 'active'`. If a vocabulary
pack is installed, a vector ranking is merged with it. Everything below is
about the two decisions that sit on top of that — how many candidates to
return and how much of each one — and about the filters that narrow the pool
before any of it runs.

Nothing here needs configuring. The defaults were measured, and this
document says what they were measured against.

## Two stages: many candidates, short excerpts

The MCP tool `search_memory` returns **20 candidates with an excerpt of ~120
characters** each, not a handful with the whole chunk. The agent consuming it
recognizes which one is useful and asks for its full body with `get_memory`.

The reordering is done by the model that is already in the conversation, so it
costs neither disk nor RAM — only the difference in tokens. Measured over a real
corpus of 174 memories and 50 queries:

| | tokens | candidates | the right memory is among those shown | same, paraphrastic queries |
|---|---|---|---|---|
| before | 1,173 | 7.6 | 72 % | 25 % |
| now | 1,301 | 20 | **82 %** | **50 %** |

Eleven percent more tokens for double the chances on the hard queries: the
ones that describe the situation instead of naming it, which are exactly
where lexical search falls short.

The trimming happens **before** counting the token budget, not at print
time. The other way round, the budget would cut the list down to eight
candidates and the change would be worth nothing.

Both values are tuned in `config.json`, next to the database:

```json
{ "candidates": 20, "excerpt": 120 }
```

On a corpus of short documents it pays to lower `candidates`: measured over
mMARCO, with passages of ~250 characters, the ceiling saturates at 10 and
asking for more is waste. With long memories, which is what anathem exists
for, the jump from 10 to 20 is the one that doubles the ceiling.

**The CLI did not change**: `mem search` still shows the full chunks.

A third key, `outline`, is **off** by default:

```json
{ "candidates": 20, "excerpt": 120, "outline": true }
```

With it on, a candidate whose memory is long **and has markdown headings**
carries a deterministic outline instead of the excerpt: its first paragraph
and its headings, read off the body at search time. No model, nothing
generated, nothing stored — the same body gives the same outline forever.

It is off because what it buys is not something recall can see, and because
it only has something to say about memories that are shaped like documents.
Measured over LongMemEval-S: 98.3% of its 23,854 memories are long enough to
carry an outline and 1.5% of those have a single heading. They are
transcripts of conversations, so there is no structure to report, and a
candidate with no headings keeps the chunk that matched — which is more
useful than its opening lines at the same price.

## The vector layer, and what it actually bought

**A vector layer? It is here already, and it is optional.** Those numbers
were the input to the question; these are the answer, measured over the same
corpus with a vocabulary pack installed:

```
                lexical (n=34)                  paraphrastic (n=16)
BM25 only       recall@5  88%  recall@50 100%   recall@5  19%  recall@50 62%
with the layer  recall@5  88%  recall@50 100%   recall@5  12%  recall@50 75%
```

It did what was expected of it and only that. The lexical ones did not move
—there was nothing to gain: they were already at 100% recall@50— and on the
paraphrastic ones it **closed part of the retrieval problem**: recall@50
goes from 62% to 75%, meaning documents that never used to show up now do.

What it did **not** do was improve the ranking. The difference between 19%
and 12% in recall@5 is a single case out of sixteen: noise, not a
regression. But the fact that it does not go up confirms the original
diagnosis — finding and ordering are different problems, and this change
attacked only one of them. **The reranker is still pending, and it is now
the binding constraint.**

### Query expansion, measured and not shipped

The sentence above names two levers for retrieval, "a vector layer or query
expansion". The layer was built; expansion was the one named in the docs with
no experiment behind it. It has one now.

The experiment needed no production code: expanding a query is adding terms
to it, and `to_match` already joins them with `OR`, so an evalset with
expanded queries measures exactly the effect. Terms came from the installed
pack — the nearest vocabulary entries to each query word, at most *k* per
word, skipping terms too rare (bm25 already answers those) or too common
(they discriminate nothing), which is the rule `search::word_evidence`
already uses.

Over the author's 50 queries:

| | recall@5 | recall@50 | paraphrastic recall@5 |
|---|---|---|---|
| no expansion | 26/50 | 41/50 | 2/16 |
| k=1 | 27/50 | 42/50 | 3/16 |
| **k=3** | **30/50** | 41/50 | 3/16 |
| k=5 | 27/50 | 40/50 | 4/16 |

**It is not noise.** The control settles that: expanding with the same number
of words drawn at random from the same vocabulary gives 24, 22 and 23 of 50
across three seeds — consistently *worse* than not expanding. The pack's
neighbours beat random by about seven cases. What moves the number is the
semantics and not the extra terms.

**But it does not do what it was proposed for.** `recall@50` is flat (41, 42,
41, 40), so nothing new is being retrieved — the documents were already
findable and expansion only reordered them. The paraphrastic subset moves
from 2 to 3 of sixteen, which is one case. And the net is unstable in *k*:
+1, +4, +1, with eleven cases changing in both directions at k=5 for a net of
one.

**What it actually fixes is something else.** Of the five queries that gain
at k=3, three are *lexical*, and the terms that win them are morphological
variants and translations:

```
q12   registros, registrar, registra        plural and conjugation
q35   deployment, deploying, deployed        morphology
q21   número -> number                       translation
q37   alineación, líneas, lignes             both
```

The index is `unicode61` with no stemmer, by a decision recorded above, so
`registro` does not match `registros` at all — not ranked low, absent:

```
save   "los registros de auditoría quedaron incompletos"
search "registros"   ->  1 result
search "registro"    ->  No results
```

So expansion from a static pack works here as a stemmer-and-translator
bolted onto BM25, which is a real gap and not the one the lever was aimed
at. Measured against its own decision rule — paraphrastic `recall@5`
unmoved, `recall@50` unmoved — expansion closes. What it leaves behind is
the question of whether the missing stemmer costs more than it saves, which
is a different experiment on a different lever.

### And then the stemmer itself, which did much less

The expansion above suggested that morphology was the lever: restricted to
neighbours sharing a root, it reached 33/50 from a baseline of 26, gaining
eight cases and losing one. `mem reindex --stem on` builds that properly —
Snowball for English and Spanish, applied to the index and to the query by
the same function — and over the same 50 queries it gives:

| with the pack | recall@5 | recall@50 | lexical | paraphrastic |
|---|---|---|---|---|
| no stemming | 26/50 | 41/50 | 24/34 | 2/16 |
| stemming | 28/50 | 40/50 | **27/34** | 1/16 |

Three gained, one lost. **Not eight.**

**The simulation was not a proxy for the thing it simulated**, and that is
the finding worth keeping. Expansion put `deployment`, `deploying` and
`deployed` into the query as three `OR` branches, each carrying its own IDF
into the score. A stemmer collapses all three to one token that matches
once. Adding surface forms and removing them are different operations, and
only one of them was ever going to be built.

What is real: lexical queries go from 24/34 to 27/34, and the defect the
whole thing started from is gone — `registro` reaches `registros` in a
database with no pack at all. What is not: the paraphrastic subset goes
*down* by one, which is the half this was supposed to help.

So it ships **off**, and turning it on is `mem reindex --stem on` — one
second over 785 memories. The setting lives in the database and not in
`config.json`, because it describes how the rows were written rather than
how anyone likes things: a query stemmed against an index that is not
matches nothing, silently, and there is no exception to notice that by.
Changing it *is* rewriting every chunk, so the only way to change it is the
command that does.

Identifiers are never stemmed — anything carrying a digit or an interior
capital, so `n+1`, `OP-204`, `RateLimitable`, `v0.8.0` and a SHA reach the
index exactly as written. That is what the lexical half of the eval set is
made of, and at 88% recall@5 it has the most to lose.

One limitation worth stating: this was measured on the author's corpus only.
LongMemEval was not re-run, so the figures above describe a 697-memory
bilingual corpus and nothing larger.

Where the layer does change things in a big way is across languages, which
is what it was built for: querying in English a corpus written in Spanish,
recall@5 goes from 34% to 58% (n=50). See
[`choosing-the-defaults.md`](choosing-the-defaults.md).

### Caveats of this measurement

- **The benchmark corpus is private and is not distributed with this
  repository.** They are the original author's real working notes; sharing
  them would violate the privacy of third parties mentioned in them. The
  numbers above are reproducible in **method** (run `mem eval` against your
  own corpus and your own eval set, with the same harness), but not in
  **data**: there is no reference corpus or eval set included to reproduce
  exactly these numbers.
- The eval set's ground truth was generated and reviewed by an agent; a
  human has not confirmed it yet.
- "390 documents" is what supermemory's document listing exposed in that
  measurement; its real search index was larger — sampling found at least
  488, and every document missing from the export that was sampled turned
  out to be a raw transcript (never a curated one). supermemory does not
  lose by being compared against a smaller document space than the one it
  was really searching.


## `bm25_weight`: the corpus decides it, not the default

`bm25_weight` balances the lexical ranking against the vector one when the
two are fused. The default is 1.0, and there is no value that is right for
everybody — this is measured, on two corpora, with the same code:

| `bm25_weight` | LongMemEval-S (MRR) | a corpus of real technical notes (MRR) |
|---|--:|--:|
| 0,25 | — | 0,390 |
| 0,5 | — | 0,388 |
| **1,0 (default)** | **0,938** | 0,409 |
| 2,0 | 0,932 | 0,422 |
| 4,0 | 0,920 | 0,454 |
| **8,0** | 0,904 | **0,466** |
| 16,0 | 0,902 | 0,455 |

(LongMemEval-S: 142 answerable questions of a 150-question sample, 7,168
memories, `en-es` pack installed. The notes: 48 hand-written questions over
682 memories. `recall@5` is saturated at 97% on the benchmark, so MRR is the
only column there that discriminates — and that saturation is a property of
the benchmark, not of the tool. Over the real notes the same measure is
**52%**, and the gap is the subject of the "How it measures up" section of
the README. Nothing below should be read as a figure anyone will see on
their own corpus.)

**The two want opposite settings, and both are right.** LongMemEval's
sessions are conversation in prose: there is meaning for a vector ranking to
catch, and it deserves its weight. A corpus of working notes is dense in
identifiers — ticket numbers, flag names, commit hashes — where BM25 is
already confident at rank 1, and fusing a second ranking dilutes that
confidence. The gap between the two optima is larger than what the pack
itself buys on either corpus.

The default stays at 1.0 because that is where the published figures are
measured and it suits prose. If your memory is mostly technical notes, try
4 to 8.

Or measure it instead of guessing: **`mem tune`** runs that grid over your
own corpus, using your topic keys as its eval set, and prints the table with
your current value marked. It recommends and changes nothing. What it cannot
do is give you a quality figure — a topic key names its subject in keywords,
so those questions flatter the lexical side — and the command says so above
its own table.

## Chinese and Japanese

FTS5's `unicode61` tokenizer splits on whitespace and punctuation. A Japanese
or Chinese sentence has neither inside it, so before this was handled an
entire clause arrived in the index as **one token**: `設定ファイル` sat inside
`決済プロバイダーはAdyenです。設定ファイルに書いてあります。` and searching for
it returned nothing, the way `figur` does not find `configuration`. The
memory was stored, counted, exported — and unreachable. `mem doctor` called
the installation healthy, because everything it checks was true.

Runs of Han, hiragana and katakana are now indexed as overlapping character
bigrams, which is what Lucene's `CJKBigramFilter` does for the same reason:
`設定ファイル` is indexed as `設定 定フ ファ ァイ イル`, and a query for `設定`
has a token to hit. The query goes through the same transformation, or the
two sides would be tokenized differently and nothing would ever match.

**Korean is deliberately left alone.** It is written with spaces between
words, so `unicode61` already splits it correctly and bigramming it would
break what works.

**Latin text is untouched**, and that is load-bearing: every figure this
document and `benchmarks.md` publish was measured on it. Checked rather than
assumed — the same 50 queries over the same 682-memory corpus give
`recall@5` 50%, `recall@50` 84% and MRR 0.409 before and after.

The indexed form lives in its own column, `chunks.fts_body`, rather than in
a trigger. `chunks_fts` is an external-content table, so
`insert into chunks_fts(chunks_fts) values ('rebuild')` re-reads the source
table: a transformation applied in the trigger would be reverted by the next
rebuild without saying anything. An existing database migrates itself the
next time it is opened.

## When nothing matched

A vector ranking has no notion of *too far*. It orders the whole corpus by
distance and returns the nearest entries, so with a pack installed a search
**always** answers — including one whose words appear nowhere in the corpus.
Those answers used to be shaped exactly like the good ones: same excerpt,
same position, same confidence.

`search_memory` marks them:

```
query "deployment"
  [id:1] the deployment pipeline runs nightly
  [id:2] [no word match] notes about invoices and billing

query "kubernetes autoscaling"
  [id:2] [no word match] notes about invoices and billing
  [id:1] [no word match] the deployment pipeline runs nightly

query "the deployment schedule"
  [id:1] [2/3 words] the deployment pipeline runs nightly
```

One labelled candidate among several is ordinary, and is the vector layer
doing its job: the memory may be the right one phrased differently, which is
the whole reason the pack exists. **Every** candidate labelled is the answer
saying that nothing here matched, and the useful response is another
wording rather than these results. A fraction says the same thing more
precisely: `[1/4 words]` across a whole answer is as thin as no match at all.

### Which words count

Only the ones that can tell one memory from another. A term matching more
than half the chunks separates nothing — BM25's own IDF crosses zero around
there, so the ranker already treats it as saying nothing — and it is
therefore left out of both the total and the count.

This is the whole fix for a defect that made the label useless in Spanish.
It used to read "did anything match at all", so a single `de` in the query
scored every memory in the corpus and the warning went silent:

```
query "quuxly frobnicate"      ->  8 hits, 7 labelled
query "de quuxly frobnicate"   ->  7 hits, 0 labelled      (before)
                                   7 hits, 7 labelled      (now)
```

Neither invented word is anywhere in the corpus. One stopword used to be
enough to hide that — and a question phrased the way a person asks one
nearly always carries a stopword, so the warning fired mainly for the
keyword queries that least needed it.

The test is frequency and deliberately **not** a stopword list. The index is
`unicode61` with no stopwords in any language; a list would have to be
written per language, and the obvious shortcut of dropping short words would
throw away `api`, `ci` and `n+1`, which are exactly the terms worth
matching. Frequency needs no list and no language.

Below twenty chunks the test is suspended: in a corpus of ten notes a word
in six of them is not a stopword, it is a small corpus, and discarding it
would leave the search with no evidence at all.

It is a label and not a filter, deliberately. Reach — documents BM25 never
surfaces at all — is what the pack measurably contributes; dropping
unmatched hits would cost the only thing it buys. `mem search` on the
command line does not print the label, because there the ranking is read by
a person who can see for themselves that nothing is related; `mem search
--explain` does print the count, next to the raw scores:

```
mem search "por que fallo el deploy" --explain
  1. [p] the deploy of OP-204 broke production  (bm25=-5.433, 3/3 words)
  7. [p] a note about something else            (bm25=-4.717, 1/3 words)
```

## Verified, and merely written down

A fact somebody checked and a hunch somebody typed used to be
indistinguishable once saved: same shape, same position, same weight when
they reached the model. `mem verify <id>` marks the first kind, and
`search_memory` labels it:

```
[id:41] [p] [verified] the deploy of OP-204 broke production
[id:77] [p] the deploy probably broke on the migration
```

Only the positive case is labelled. Marking the rest `[unverified]` would put
the word on nearly every line of nearly every answer, which is how a signal
stops being read, and it would read as an accusation against every note
written before there was any way to say otherwise. **Absence means "nobody
said", never "known to be wrong".**

**It moves no ranking**, and a test pins that. Promoting what is verified
would quietly bury a correct hunch, and nothing here moves a ranking on
something that has not been measured. What the mark changes is what the model
reads, not what it is shown.

Confirming is not correcting, so `verify` rewrites nothing — not the body,
not the date, not the id. Going through `--topic` instead would create a new
version and leave the old one superseded, which says the content changed when
only its standing did. The mark can also go on a superseded memory: whoever
walks `mem history` to see what a topic used to say is better served knowing
which of those versions had been confirmed.

From MCP it is one step: `save_memory` takes `verified: true` for something
the agent checked in that same session — a command whose output it read, a
file it opened. The mark is worth exactly as much as that honesty, which is
why the tool's description says what it is not for.

## Near-duplicates

`mem doctor` lists memories inside one project that share most of their
words — the same fact saved twice. It is information, never a problem, and
it reports rather than merges: which of two overlapping notes is the keeper
needs judgement about what they are for, and merging destroys the loser.
`mem supersede <new> <old>` keeps one, `mem forget <id>` retires the other.

The scale it was built for: near-duplicate pairs came to **0.21 %** of all
pairs over a real 682-memory corpus. That number is why nothing automatic
exists. The first run of the check over a 688-memory corpus found one pair
at 82 % overlap, a note saved again after a shell ate part of it and never
superseded — both still active, both still ranking.

It is not contradiction detection, and nothing here is. Two notes can share
every word and disagree on the one that matters; token overlap cannot see
the difference, and the only comprehension in reach is an embedding average
already measured to miss paraphrase.

## Narrowing the pool

`--scope key:value` narrows a search to a `project`, a `tag` or a `topic`,
and several combine with AND:

```bash
mem search "branching" --scope topic:branching-policy --scope tag:decision
```

A key it does not recognise is an error rather than a filter quietly
dropped: a dropped filter returns more than you asked for, which looks
exactly like a search that worked. A scope that holds nothing says so, which
is a different answer from "no results for this query".

Measured over LongMemEval-S, narrowing is a retrieval improvement and not
only a convenience — `multi-session` questions gain 8.9 points of recall@5
on BM25 and 6.0 on hybrid when the distractors are cut to a quarter. The
method, the rule fixed before the run and what the number does **not** prove
are in [`benchmarks.md`](benchmarks.md).

## Per-project isolation

Every memory belongs to a project, and `search --project` filters by it. If
`--project` is not passed when saving, it is derived from the repository you
are standing in, in this order:

1. **The name of the `origin` remote.** It comes first on purpose: a
   repository usually lives in a folder that is not named after it
   —`api-monolito` cloned into `API/Web`— and inside a worktree the folder
   is named after the branch. The remote is the only name that is stable
   across clones, worktrees and machines.
2. The name of the repository's folder, if there is no remote.
3. `no-project`, outside any repository.

An explicit `--project` always wins. The project used is reported when
saving, with the `(detected)` mark when it was not chosen by hand: an
implicit value nobody can see is the one nobody later understands why it
came out wrong.

`git` is not read as a program, the `.git` file is read directly. That way it
works even if git is not installed, and it does not cost a subprocess per save.

### What searches by project, and what does not

`search_memory`, over MCP, **searches the project the session is in**, worked
out from the working directory exactly as saving does. The agent can look
past it with `all_projects: true`, name another with `project`, or pass a
`project:` scope; all three win over the implicit one.

`mem search`, on the command line, does **not**: it searches everything
until you pass `--project`. The asymmetry is deliberate. A server runs for a
session with one working directory and one project, and the model never sees
that it could have narrowed; a person typing a command sees the answer and
narrows in the next line.

This used to be true of both, with the reasoning that *filtering by default
would hide memories without warning, which is worse than bringing back one
too many*. The second half of that was measured and is wrong. Over a pooled
corpus the right memory is in the top five 47 % of the time at 23,788
memories, against 90 % for the same corpus and the same questions searched
within a project — bringing back one too many is not free, it is most of the
ranking.

The first half was right, and it is why the narrowing is not a wall: **when
the implicit project comes back empty, the search widens to every project on
its own and says so in the answer.** Every hit prints the project it came
from, so a widened answer is legible rather than silent. Nothing becomes
unreachable; what changes is the order things are tried in.

## Tag taxonomy

`save_memory` (via MCP) and `migrate` assign tags automatically by
classifying the body of the memory against a small set of `{tag, pattern}`
rules. The default taxonomy is deliberately small and generic — meant for
anyone keeping engineering notes, with no vocabulary tied to a particular
stack or team: `decision`, `bug`, `convention`, `performance`, `testing`,
`docs`, `code-review`.

To replace it, put a `tags.json` file next to the database (in the same
directory as `mem.db`). Its rules **replace the default ones entirely**,
they are not added to them. Format: a list of `{tag, pattern}` objects,
where `pattern` is a regex (syntax of Rust's `regex` crate):

```json
[
  { "tag": "urgent",   "pattern": "(?i)\\burgent\\b|\\bASAP\\b" },
  { "tag": "customer", "pattern": "(?i)\\bcustomer\\b" }
]
```

A malformed file (invalid JSON, no rules, or with a `pattern` that does not
compile as a regex) makes the binary fail with a message naming the file and
the offending rule — it never falls back silently to the default rules.

