# anathem versus Engram

[Engram](https://github.com/Gentleman-Programming/engram) is the closest thing
to anathem that exists: a Go binary, SQLite with FTS5, no daemon needed, no
Node, no Python, no Docker, an MCP server for any agent. The same underlying
decisions, taken separately.

And as a product it is further along: v3.7.2, releases for Linux, macOS and
Windows on both architectures, installation via Homebrew, a TUI, an HTTP API,
a CLI, and 27 MCP tools against anathem's 5.

So the honest question is whether anathem has any reason to exist. Earlier
versions of this document answered it with 449 memories and 50 queries written
by hand over a private corpus. That corpus was never published, so nobody could
repeat the measurement, and this version replaces it with two public datasets
and one harness for both systems. The old numbers are still at the end, marked
for what they were.

## The setup

Both systems got the same corpus and the same questions, one search per
question, scoped to that question's own history in both — Engram's `project`,
anathem's `project`:

| corpus | questions | memories | licence |
|---|---:|---:|---|
| [LongMemEval-S](benchmarks.md) (session unit) | 470 | 23,854 | MIT |
| LoCoMo (session unit) | 1,536 | 272 | CC BY-NC 4.0 |
| LoCoMo (turn unit) | 1,536 | 5,882 | CC BY-NC 4.0 |

Engram 3.7.2, `linux_amd64`, checksum verified, in its own data directory. Its
memories were imported from the same markdown corpus anathem imported, through
`engram import` — not through 449 separate `engram save` calls, which was the
weakest point of the earlier measurement.

Two things about that import are worth saying because they change the corpus
Engram searched. It collapses sessions whose text is identical within a
project word for word: 5,390 of the 23,854 in LongMemEval ended up as 18,464
observations. And Engram has no evaluation harness, so its searches went
through its HTTP API (`/search`, compact format) and the returned observation
ids were mapped back to the dataset's session ids by SHA-256 of the content, so
a hit on the surviving copy counts for every session that carried the same
text. Engram was asked for 20 candidates per query; anathem's harness returns
10.

**One asymmetry, and it favours Engram:** its queries were stripped of English
stopwords first. Without that step its own default returns nothing at all (see
below). Anathem was given the questions exactly as the datasets write them.
The stopword list is the author's, chosen while looking at these corpora, so
Engram's figures are a ceiling and not a neutral floor.

## What Engram needs to be asked

| configuration | LongMemEval-S | LoCoMo session | LoCoMo turn |
|---|---:|---:|---:|
| `--match all` (its default) | 2.1 % | 3.1 % | 0.0 % |
| `--match any` | 17.7 % | 38.1 % | 11.0 % |
| `--match all`, stopwords removed | 19.4 % | 28.2 % | 8.7 % |
| `--match any`, stopwords removed | **80.2 %** | **81.1 %** | **50.3 %** |

Recall@5 in all four rows. The default requires every word of the question,
stopwords included, so a normally phrased question — "When did Caroline go to
the LGBTQ support group?" — returns nothing: **96 to 100 % of questions came
back empty** in the default mode. With `any` and the stopwords gone, the same
engine answers 80 % of LongMemEval.

That is the practical finding of this document, and it cuts both ways. An
agent that asks the way a person asks gets no results from Engram. An agent
that has been taught to ask in keywords gets a competitive retrieval engine.
That preprocessing is not free and not automatic: it is a step Engram does not
take for you.

## The numbers

Recall@5 is the metric to read. Recall@10 is the deepest slot both can be
compared on — anathem's harness hands back ten candidates, and Engram's list was
cut to ten although it was asked for twenty — and the pair shows whether the
evidence was missed or merely misplaced. `empty` is the share of questions that
came back with nothing.

**LongMemEval-S**, 470 questions over 23,854 memories:

| | r@1 | r@5 | r@10 | MRR | empty |
|---|---:|---:|---:|---:|---:|
| anathem BM25 | **87.2 %** | **96.2 %** | **97.7 %** | **0.912** | 0 % |
| Engram, `any`, stopwords removed | 71.5 % | 80.2 % | 83.6 % | 0.755 | 0 % |
| Engram, `any` | 2.3 % | 17.7 % | 31.9 % | 0.110 | 0 % |
| Engram, default | 2.1 % | 2.1 % | 2.1 % | 0.021 | 98 % |

**LoCoMo, session unit**, 1,536 questions over 272 memories:

| | r@1 | r@5 | r@10 | MRR | empty |
|---|---:|---:|---:|---:|---:|
| anathem BM25 | **65.6 %** | **88.8 %** | 95.4 % | **0.761** | 0 % |
| anathem, vocabulary pack | 59.1 % | 86.7 % | **94.6 %** | 0.713 | 0 % |
| Engram, `any`, stopwords removed | 57.6 % | 81.1 % | 88.9 % | 0.684 | 0 % |
| Engram, `any` | 16.0 % | 38.1 % | 55.4 % | 0.277 | 0 % |
| Engram, default | 2.9 % | 3.1 % | 3.1 % | 0.030 | 96 % |

**LoCoMo, turn unit**, 1,536 questions over 5,882 memories:

| | r@1 | r@5 | r@10 | MRR | empty |
|---|---:|---:|---:|---:|---:|
| anathem BM25 | **30.6 %** | **52.2 %** | **60.9 %** | **0.409** | 0 % |
| Engram, `any`, stopwords removed | 30.0 % | 50.3 % | 58.1 % | 0.392 | 0 % |
| Engram, `any` | 5.6 % | 11.0 % | 13.7 % | 0.080 | 0 % |
| Engram, default | 0.0 % | 0.0 % | 0.0 % | 0.000 | 100 % |

The anathem rows reproduce the published ones — 96.2 against 96.0 in
`benchmarks.md`, 88.8 and 52.2 exactly on LoCoMo — so the corpora are the same
ones, not a favourable reconstruction.

By question type on LongMemEval-S:

| type | n | anathem | Engram |
|---|---:|---:|---:|
| knowledge-update | 72 | 100 % | 91.7 % |
| multi-session | 121 | 97.5 % | 85.1 % |
| single-session-assistant | 56 | 100 % | 73.2 % |
| single-session-preference | 30 | 70.0 % | **76.7 %** |
| single-session-user | 64 | 100 % | 76.6 % |
| temporal-reasoning | 127 | 95.3 % | 74.8 % |

Engram wins one bucket, the preferences, by 6.7 points with n=30. That is the
kind of difference this sample produces by accident, and it is the one place
where the direction is against anathem, so it stays in the table.

## What the numbers say

**Anathem is ahead by 16.0 points in LongMemEval, 7.7 in LoCoMo sessions and
1.9 in LoCoMo turns.** The margin shrinks as the unit of memory gets finer, and
the reason is architectural rather than mysterious: Engram indexes a whole
memory as one FTS5 document, so a 4,000-character session dilutes every term's
weight; anathem indexes 1,000-character chunks and returns the memory they
come from, with a 300-character excerpt. When a memory is a single turn, there is nothing to dilute and the two
are within noise of each other — 52.2 against 50.3, and 30.6 against 30.0 at
rank 1.

**Engram is not blind past five, which is the other half of the picture.**
Asked for twenty results instead of ten, its best configuration reaches 85.7 %
on LongMemEval, 96.0 % on LoCoMo sessions and 64.3 % on turns: the evidence is
often in the list and the first five are where it is lost. That is the shape a
shallow ranker makes, and it is why the answer is "anathem ranks better" and
not "Engram cannot find it".

**Engram's ranking is not the problem, and the earlier version of this
document said it was.** It orders by FTS5 `bm25()` with the title weighted five
times the content and the topic key three, plus small boosts for pinned,
recently seen and repeatedly confirmed memories — 10 %, 6 % and 4 %. The
recency the earlier version of this document read into its results was a
coincidence of those five rows, not a naive `ORDER BY created_at`. What Engram lacks is depth for
long memories, not a ranker.

**What it also lacks is a refusal to work without preprocessing**, and that is
a product decision with a cost this table makes visible: 96 % of questions
return nothing under its default. It is not a bug to fix in this repository's
opinion, but it is the difference between a memory an agent reaches for and one
it stops asking.

**The token cost still favours anathem.** Asked for 20 candidates, Engram's
compact response is about 12 KB (~3,000 tokens) and anathem's is about 5 KB
(~1,300): the previews are the same 300 characters on both sides, and the rest
is metadata per result. The earlier figure of "twice the tokens" holds, now on
equal terms.

## The caveats

- **The stopword list is mine**, chosen with these corpora in view. Engram's
  best row is optimistic by an amount this document cannot measure.
- **The corpus is shaped for anathem.** Engram's normal ingestion is an
  agent-written summary, a title and a few sentences, not a raw 4,000-character
  transcript. This measures both engines on the same memories, which is what a
  head-to-head means, but it does not measure Engram as it is meant to be fed.
  Whoever runs the next experiment should save with an agent through MCP and
  compare that.
- **Latency is not in these tables.** Anathem's p50 was 2,537 ms in this run
  against 375 ms in `benchmarks.md` on the same corpus. That turned out to be
  the scoped search scoring the whole corpus and the document-frequency test
  walking a term's postings: 1.3.1 fixes both and the same command now reports
  110 ms, with every ranking unchanged. Quoting the run's figure would be
  quoting a number the release after it disproved. Engram answered in 4 to 294
  ms depending on configuration and corpus.
- **n=470 and n=1,536, no confidence intervals.** Differences of a few points
  are noise; of fifteen are not.
- **LongMemEval-S's 470 questions are synthetic** and its sessions are
  dialogues. LoCoMo's are written against 600-turn conversations. Neither is a
  corpus of technical notes, which is what anathem is actually used on.

## The earlier comparison, and why it is gone

The first version of this document measured 449 memories from the author's own
database and 50 queries he wrote. On that corpus, anathem reached recall@5 of
52 % and recall@20 of 74 %, Engram 54 % at 20 in its best configuration (and
4 % by default), Engram
cost 2,458 tokens against 1,303, and its searches took 435 ms against 8 ms.
Those numbers are still the only ones measured on real technical notes, and
they point the same way as the public ones. They are not headline figures
because nobody else can download that corpus: it was the same weakness this
repository criticises in
[supermemory](comparison-with-supermemory.md), and repeating it in a public
document was not worth the twelve points.

## When to pick which

**Engram** if you want something finished and you do not mind phrasing things
with keywords instead of questions. Homebrew, a TUI, an HTTP API, import and
export, 27 MCP tools, and a store you can also use as a workspace. For a
single-language corpus queried with concrete nouns, its rank-1 is competitive.

**anathem** if you ask in prose, if your corpus mixes languages, or if you care
about token cost and the evidence surviving the twenty candidates. Its edges
here are 16.0 and 7.7 points on the two session-level corpora, and one line in
`config.json` versus a stack of subcommands on the other side.
