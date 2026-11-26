# What the literature says, and what it changed here

Last reviewed: **2026-11-26** for the retrieval section, **2026-11-15** for
the agent-memory entries. "Reviewed" means the abstracts were read and every
figure below was checked against the paper's own page — not that the papers
were read in full, and not that anything here was reproduced.

**An entry is here because it changed a decision or a measurement, not
because it agrees.** A page that only collects supporting papers is
post-hoc justification, and the argument of this project is that it does
not do that. So the papers that cut against it are here too, and they come
first.

Every number below is the paper's own. Nothing in this file was measured
here. What *was* measured here is in [`benchmarks.md`](benchmarks.md) and
[`search.md`](search.md), and the comparisons with other products are in
[`comparison-with-supermemory.md`](comparison-with-supermemory.md),
[`comparison-with-engram.md`](comparison-with-engram.md) and
[`comparison-with-openviking.md`](comparison-with-openviking.md).

## The ones that argue against this

**arXiv 2611.01718 — *Delivered, Not Used: Evaluating Agent Memory at Its
Point of Use*.** The strongest argument against the category this project
belongs to. Across 8,000 SWE-bench Verified episodes from three memory
systems, resolve rates stayed within **0.4 percentage points** of a
no-memory baseline; most delivered entries were irrelevant, most relevant
ones went unused, and in two of the three systems most deliveries repeated
what the agent could already see. It also found outcomes hide what
happened: 23.2 % of episodes with beneficial memory use still failed.

The honest answer is not "but it helps". It is that anathem instruments
delivery against use — `search_log.followed_up`, `mem stats`, `mem log` —
and reports what it cannot measure. *State: adopted as a constraint on what
this project is allowed to claim.*

**arXiv 2610.29823 — *MemoryTax: The Bitter Lesson of Agent Memory*.**
Designed memory is a costly patch for a weaker model, and it has a ceiling
set by the model that *writes* it: "a stronger reader does not recover what
a weaker writer left out". anathem has no writer model, which is both the
answer and the limit — its value is cross-session and cross-machine, not
within the current session. The paper is explicit about its own scale: one
LoCoMo conversation, one trial per cell. *State: adopted.*

**arXiv 2610.30755 — *Is Complex Agentic Memory Always Needed? Naive RAG Is
Stronger Than We Thought*.** Raw-RAG — dense retrieval and reranking over
raw history — ranks first in six of seven benchmark-model settings across
LoCoMo, LongMemEval, MEME and MemoryAgentBench, beating the strongest
agentic memory system by **0.13–5.53 points**. It supports this
architecture and undercuts the premise that memory needs designing at all.
*State: adopted, uncomfortably.*

**arXiv 2610.26433 — *The Geometry of Agent Memory*.** Once difficulty and
system identity are controlled, embedding proximity adds **+0.002, 95 % CI
[−0.010, 0.014]** — an interval spanning zero, so no established
improvement. Recorded so that nobody here proposes a similarity-based
confidence score again. *State: adopted as a prohibition.*

**arXiv 2611.03930 — *What Is This Memory Useful For?*** Measures
task-conditioned utility instead of one importance score. On single-call
factual recall the mean paired reward gain exceeds matched controls by
**+0.33 (95 % interval +0.21 to +0.46)** — and that gain does **not**
extend to the software-engineering and scientific-coding benchmarks tested,
where even raw memories showed no measurable effect. The second half is the
part worth carrying: the kind of task decides whether memory helps at all.
*State: open.*

## The ones that changed something

**arXiv 2608.12888 — *When Your Agent Opens the Chat App*.** Builds no
semantic structure: the archive is left unmodified, indexed lexically at
turn granularity, with an agent running iterative keyword search. **58.2**
mean accuracy against HippoRAG 2 at **53.2** on MemoryAgentBench, and
93.2 / 89.3 on LongMemEval-S/M. Its four chat-native controls are
session-aware rank fusion, local context expansion, temporal narrowing and
skipping inspected sessions; anathem had the first two and not the third.
*State: adopted — `--since` / `--until`, shipped in 0.10.0 (#54).*

**arXiv 2611.06545 — *When Do Summaries Become Instructions?*** Self-written
handoff summaries assert permissions the record does not support:
unsupported **restrictions** in 1.2 % and 2.4 % of readable summaries
against unsupported **authorizations** in 0.1 % and 1.3 %. Saying in the
system prompt where requirements come from cut unsupported restrictions
from **5.0 % to 1.8 %**, and from 1.8 % to 0.3 % on held-out templates
(p < 10⁻⁹). The restriction direction is the frequent one and the silent
one. *State: adopted — one sentence in the MCP greeting, 0.10.0 (#55); see
[`security.md`](security.md).*

**arXiv 2610.18052 — *Can Agents Forget When Asked?*** Across six memory
systems and three backbones, **no system passes more than 1.5 %** of forget
requests end to end, failing in both directions: leaving the content
extractable, or deleting more than was asked. anathem's `forget` was the
first failure exactly. *State: adopted — `mem delete`, 0.10.0 (#49).*

**arXiv 2610.36308 — *When Retrieval Is Not Enough: Obsolete Facts in Agent
Memory*.** With the correct fact already retrieved, replacing one unrelated
statement with an obsolete competing one drops accuracy from **100 % to
44.6 %** on Qwen2.5 and to 87.8 % on Qwen3-14B. An obsolete statement does
more damage than unrelated evidence of the same size. anathem's search
filters `status = 'active'`, so a superseded memory never competes with its
replacement. *State: adopted, and it was already the design.*

**arXiv 2610.10265 — *Stale, Misattributed, or Late*.** Without update
resolution, **70.3 %** of prompts expose a superseded value; serving only
the active value of each correctly keyed slot eliminates observed stale
exposure. That is the gain this project's topic keys and `supersede` were
already buying. *State: adopted, already the design.*

**arXiv 2611.19752 — *StaleBench: Isolating Stale Retrieval in Agent Memory,
and Pricing the Fix*.** 892 real updating facts from Wikidata, with currency
labels from the source and a frozen evaluator. Store-all retrieval puts both
a current and a stale version in the top five for **99.1 %** of queries;
the keep-latest oracle reaches **0.0 %**. Its point is that retrieval
metrics can rank policies backwards while `contamination@5` does not.
*State: adopted and measured here, #70; the store-all and superseded rows
are in [`benchmarks.md`](benchmarks.md).*

**arXiv 2610.09348 — *Relevance Is Not Sufficiency*.** Top-k results can
each be relevant and still jointly omit the complementary fact an answer
needs. Budgeted Flat Reconstruction raises judged accuracy on LongMemEval-S
from **72.4 % to 82.2 %** and Turn Hit to 91.4 %. anathem deduplicates to
the best chunk per memory, so twenty candidates are twenty memories; the
other redundancy — twenty shades of one topic — was tested here and found
to be structurally impossible (see [`search.md`](search.md)). *State:
rejected for this corpus, with the numbers, #51.*

**arXiv 2611.04003 — *WASP-Mem*.** Prescriptive knowledge organisation —
human-defined schemas and **explicit deduplication rules** — hurts rather
than helps, and one round of human diagnosis beats four rounds of
self-judgment by **7.0–10.9 %**. It is the reason the near-duplicate
report stays a line a person reads in `mem doctor` rather than a rule
firing on the agent's write path. *State: adopted — #47 closed on it.*

**arXiv 2611.07689 — *MCPHallu*.** Across 358 tasks on 36 production MCP
servers and 14 agents over 5,012 trajectories, the mean score is **0.642**,
and failures concentrate on Unreachable Goal: **0.458** mean, **23.2 %** of
runs passing, where agents keep calling tools or claim success instead of
recognising the goal cannot be reached. It benchmarks agents, not servers,
so it is not a bug list for anathem — it is the reason the `search_memory`
text was measured against the case it has no stop condition for. Measured
here: in 36 of 36 runs the agent said the fact was not there and none
answered anyway. *State: checked, text left alone, #56.*

**arXiv 2611.10170 — *Where Did the Evidence Go? Diagnosing Proof-Set Loss in
Long-Term Conversational Memory*.** Tracks the support turns a question needs
through the funnel — retrieval, reranking, final context — and finds the loss
in two places, not one. Its EviCover selects turns jointly by checking that
the question's evidence requirements are covered: among 60 candidate turns,
support recall rises from **0.526 to 0.575**, and in the final ten-turn context
from **0.349 to 0.379**. And then the sentence that matters here: greater
evidence recall **did not** establish a correctness gain. *State: adopted as a
constraint, and reproduced.* Our own graduated payload raised strict coverage
from 85.8% to 91.1% and lowered answers from 68.0% to 65.3%; the dissociation
is real and it is why `docs/benchmarks.md` now reports coverage and answers as
two separate measurements of the same corpus.

**arXiv 2611.11840 — *Correct Answers from Insufficient Histories*.** An answer
can match the reference while the visible history does not determine it: on
203 deliberately insufficient instances, Qwen3-32B matched the hidden
reference **87** times and abstained on seven. *State: adopted as a caveat.*
Every accuracy figure in `docs/benchmarks.md` includes some answers that were
right by guess, and the section says so; only coverage bounds it.

**arXiv 2610.41172 — *Context as Infrastructure: Knowledge Compilation for
Retrieval in Agentic Systems with Harnesses*.** A harness with BM25/grep alone
answers 85% of BrowseComp-Plus and 91% of multi-hop FRAMES — and, under a
competent harness, LLM-compiled structural artifacts gave **no detectable
accuracy gain** on FRAMES, while a page-outline structure degraded at scale.
What compilation does buy is tokens: a navigation tree of claim-evidence leaves
cut query tokens by **91%** when the evidence suffices, and added 1.9% when it
did not. *State: adopted as the argument for not compiling.* It is the paper
behind the position in
[`comparison-with-livingbrain.md`](comparison-with-livingbrain.md): a compiled
page is a supply layer, not a better retriever.

**arXiv 2610.22588 — *SAG2: Embedding-Free Multi-Hop Retrieval with BM25 and
Decision Models*.** The closest published shape to anathem's storage: a SQLite
lexical index, nothing generative at index time, no embeddings. What it adds is
a **decision model at query time** that scores whether a candidate supplies a
fact in the answer chain, and uses its terms to reach passages the first query
did not. Recall@10 of 86.62% on MuSiQue against BM25 alone, at about a fifth of
the cost of the LLM-based system it replaces. *State: open, and it is the
honest boundary of this project.* The structure is ours; the component that
makes multi-hop work is a model, and anathem's answer to that is the model
already in the conversation — which is what the `search_memory` text asks it to
use, and what `docs/benchmarks.md` measures as the arm that decides for itself.

## The ones still open

**arXiv 2611.02912 — *A State Change Is Local, Its Consequences Are Not*.**
When an upstream fact changes, the beliefs and plans derived from it stay
accepted although they lost their support. StateGraph gains **6.9** points
of Impact-F1 at cascade depth 1 and **26.0** at depth 3+ over a CUPMem
reimplementation. The gain is in the *invalidation*, not in the edges —
which is why storing edges without it was rejected here. *State: rejected
as proposed, #48; the problem is real and unsolved.*

**arXiv 2611.07631 — *MEMOIR*.** Static importance scores are never revised
after use, so retrieval keeps favouring stale memories; retrospective
attribution plus a UCB exploration bonus gives **+11.92** points of
evidence recall@5 over bare cosine. The attribution is judge-gated — a
model decides which memories supported the answer — and anathem has no
model in the loop. *State: rejected, #50, because the signal does not
transfer and nothing here could measure it.*

**arXiv 2608.22767 — *The Retriever Should Remember* (EARM).** Reranking
scores are reusable experience: a sparse query–memory matrix, completed and
reused, improves answer accuracy over semantic retrieval by **up to
6.62 %** and stays effective when only **17.5 %** of candidates get a
direct LLM relevance score. Same obstacle as MEMOIR. *State: rejected,
#50.*

**arXiv 2610.36153 — *Stratum: Repository Memory for Coding Agents*.** The
strongest single citation for this project's premise, and the only one
measured on real repositories: across **752** bug-fix commits the median
fix edits **two files**, and those plus their direct imports are under
**10 %** of the repository — "the bottleneck is selection, not capacity".
On eight real bugs, a fresh agent inheriting a prior session's memory
*written by a different model* found the root-cause file in **21 of 24**
trials against **6 of 24** for a retrieval-only baseline, at 4.5× lower
median token cost. That is cross-model handoff, which is what `mem serve`
plus `mem recent` do. *State: open — nothing here is measured this way.*

**arXiv 2611.02361 — *Agent Memory as Rate-Limited Filtering*.** Open
models of 7–32 B reading full history fall from 72–86 % after 8 or 16
events to **48–55 %** after 256; reasoning-trained 32 B models reading an
explicit current state reach **96 %** and **98 %**. Read carefully: those
are different models under different conditions, not a head-to-head at 256
events, and the paper says the crossover point is reader-dependent.
*State: open.*

**arXiv 2610.33274 — *FlexA-Mem: Budget-Adaptive Agentic Memory*.** Writes
each unit of history once into an immutable store and renders a fresh
memory per call, with no LLM call in the rendering. **+11.5 %** over the
second-best method on AppWorld challenge tasks at 4k, and +12.2 % over
A-Mem with a summary on 147 held-out tasks. The immutable store is what
anathem already is; the budget-adaptive rendering is not. *State: open.*

**arXiv 2610.27377 — *REM: Reusable Experience Memory for Coding
Agents*.** The bottleneck in experience reuse is not retrieval fidelity but
how experience is stored and governed across its lifecycle. Retrieval
match@1 **68.7 %** against 55.6 % for the best prior system, end-task
resolution 27.6 % against 24.9 %, at 17 % lower cost. *State: open.*

**arXiv 2610.38521 — *Security Impact of Coding Agent's Memory Files*.** A
malicious memory file moves the rate of secure completions by **−8 to +6
percentage points**, with no consistent pattern across agents or variants.
The abstract names `CLAUDE.md` and the agents tested are Claude Code and
OpenCode; whether `AGENTS.md` was covered is **not stated**, so this entry
does not claim it was. *State: open; relevant to
[`security.md`](security.md).*

## Retrieval: why BM25, and what would replace it

Reviewed **2026-11-26**. These entries are about the ranking algorithm
itself rather than about memory. The short version: BM25 is not the best
ranker in absolute quality, but it is the best first stage that needs no
model, and the next gains here are its parameters, its tokenizer, its depth
and its fusion accounting — not a different family. What would reverse
that is written at the end of the section.

**arXiv 2104.08663 — *BEIR: A Heterogenous Benchmark for Zero-shot
Evaluation of Information Retrieval Models*.** Eighteen datasets and ten
systems. BM25 is "a robust baseline"; re-ranking and late-interaction
models average best zero-shot, "however, at high computational costs",
while dense and sparse retrievers are cheaper but "often underperform".
*State: adopted — it is why the first stage is lexical and a neural stage
is opt-in, not the default.*

**arXiv 2403.06789 — *SPLADE-v3: New baselines for SPLADE*.** Learned sparse
retrieval is statistically more effective than BM25 and SPLADE++, with more
than **40 MRR@10** on MS MARCO dev and **+2 %** out-of-domain on BEIR. It
still needs a transformer at index and query time. *State: rejected for
now; the cost that would reverse it is a local model that fits the pack's
22.5 MB and the query latency budget.*

**arXiv 2610.30612 — *The Fusion Tax: A Diagnostic Analysis of When Hybrid
Retrieval Hurts*.** Fusion beats dense-only in aggregate on every corpus
with the primary encoder, yet ranks relevant documents below the better of
its two components on **13–48 % of queries** (8–21 % below dense-only). RRF
carries the higher tax wherever one side is clearly stronger, and per-query
routing does not transfer reliably across corpora. *State: adopted as a
constraint; `mem eval --compare` now measures the per-query tax, #77.*

**arXiv 2605.10848 — *Rethinking Agentic Search with Pi-Serini: Is Lexical
Retrieval Sufficient?*.** A well-configured BM25 with sufficient depth and a
capable agent beats released dense-retriever agents on BrowseComp-Plus
(**83.1 %** answer accuracy, **94.7 %** evidence recall). In the ablation,
moving Anserini's default `(k1, b) = (0.9, 0.4)` to `(16–25, 1.0)` raised
answer accuracy from **64.0 % to 82.0 %** and evidence recall from **84.6 %
to 95.7 %**; increasing retrieval depth added **25.3 %** evidence recall
over the shallow setting. *State: measured — `k1`/`b`/`delta` and
`retrieval_depth` shipped in #75 and #78; on LongMemEval neither tuning nor
deeper pools improved over FTS5's defaults, so both defaults stayed.*

**arXiv 2605.18561 — *Improving BM25 Code Retrieval Under Fixed Generic
Tokenization: Adaptive q-Log Odds as a Drop-In BM25 Fix*.** Under a frozen
generic tokenizer, BM25's IDF under-separates the identifier tail. Replacing
the outer logarithm with a q-logarithm raised NDCG@10 on CoIR CodeSearchNet
Go from **0.2575 to 0.4874** (+89.3 %), while the effect is near zero on
BEIR text. The ablation is the part that transfers: identifier-aware
tokenization "largely removes" the gain. *State: adopted — #76 ships the
adjacent-token phrase; the q-log transform itself is not proposed.*

**arXiv 2604.01733 — *From BM25 to Corrective RAG: Benchmarking Retrieval
Strategies for Text-and-Table Documents*.** Over 23,088 financial queries,
BM25 beats state-of-the-art dense retrieval, and the best pipeline is hybrid
plus neural reranking (Recall@5 **0.816**, MRR@3 **0.605**), well above any
single-stage method; query expansion and adaptive retrieval give limited
benefit for precise queries. *State: adopted — BM25 first; reranking only
if it is opt-in and measured.*

**Lv & Zhai, CIKM 2011 — *Lower-bounding term frequency normalization*.**
The paper behind BM25+: BM25 over-penalizes long documents because TF
normalization is not lower-bounded. Anathem's chunks are near-uniform, so
the expected gain is small. *State: measured — #75 put BM25+ in the grid;
on LongMemEval no setting beat FTS5's defaults, so the default stays.*

**Kamphuis et al., ECIR 2020 — *Which BM25 Do You Mean? A Large-Scale
Reproducibility Study of Scoring Variants*.** The name "BM25" covers
variants and parameter settings that rank materially differently; a result
is reproducible only when both are stated. *State: adopted as a caution —
this file and the code should name the variant and the parameters.*

**arXiv 2204.12755 — *A Thorough Examination on Zero-shot Dense
Retrieval*.** Dense retrievers are "shown to be not as competitive as
traditional sparse retrieval models (e.g., BM25) in a zero-shot retrieval
setting"; the paper isolates the factors, including the source training set
and target bias. *State: adopted — it qualifies the in-domain wins of dense
retrieval.*

**What would reverse the decision.** A local transformer that fits the
pack's 22.5 MB and keeps p50 near 110 ms, or an eval where BM25 has
saturated and the vector layer cannot close the gap. The four levers that
did not need one — parameters, identifiers, depth and per-query fusion cost
— shipped in #75–#78; none beat the defaults on LongMemEval, so the default
path stands. What remains is the same measurement on a corpus where the two
sides disagree, and the private notes corpus for tuning.

## The table

| paper | claim | figure | what it changed here | state |
|---|---|---|---|---|
| 2611.01718 | memory often changes no outcome | within 0.4 pp of no memory, 8,000 episodes | what this project may claim | adopted |
| 2610.29823 | memory is a patch for a weak writer | — (one conversation) | the limit is stated, not hidden | adopted |
| 2610.30755 | raw history + reranking wins | 0.13–5.53 points, 6 of 7 settings | supports the architecture | adopted |
| 2610.26433 | embedding proximity predicts nothing | +0.002, CI [−0.010, 0.014] | no similarity-based confidence score | prohibition |
| 2611.03930 | utility is task-conditioned | +0.33 recall tasks; none on SWE | — | open |
| 2608.12888 | lexical + agent control rivals structure | 58.2 vs 53.2 | `--since` / `--until` | adopted (#54) |
| 2611.06545 | summaries assert unsupported policy | 5.0 % → 1.8 % | one sentence in the greeting | adopted (#55) |
| 2610.18052 | nobody keeps a forget promise | ≤1.5 % end to end | `mem delete` | adopted (#49) |
| 2610.36308 | an obsolete fact beside the current one | 100 % → 44.6 % | confirmed `status = 'active'` | already so |
| 2610.10265 | stale values reach the prompt | 70.3 % of prompts | confirmed topic keys | already so |
| 2611.19752 | store-all retrieval keeps both versions | contamination 0.991 → 0.000 | StaleBench measured | adopted (#70) |
| 2610.09348 | relevance is not sufficiency | 72.4 % → 82.2 % | per-topic cap: impossible here | rejected (#51) |
| 2611.04003 | prescriptive organisation hurts | 7.0–10.9 % | dedup stays a report | adopted (#47) |
| 2611.07689 | agents do not stop on unreachable goals | 0.458, 23.2 % pass | measured; text unchanged | checked (#56) |
| 2611.02912 | consequences outlive their premise | +6.9 / +26.0 Impact-F1 | edges without invalidation | rejected (#48) |
| 2611.07631 | importance is never revised | +11.92 recall@5 | judge-gated, cannot transfer | rejected (#50) |
| 2608.22767 | reranking scores are reusable | up to 6.62 % | same obstacle | rejected (#50) |
| 2610.36153 | selection, not capacity | 21/24 vs 6/24 | cross-model handoff | open |
| 2611.02361 | explicit state outlasts long context | 48–55 % vs 96–98 % | — | open |
| 2610.33274 | one immutable store, rendered per call | +11.5 % | the store already matches | open |
| 2610.27377 | the lifecycle is the bottleneck | 68.7 % vs 55.6 % | — | open |
| 2610.38521 | memory files move security either way | −8 to +6 pp | — | open |
| 2611.10170 | the evidence funnel loses in two places | support recall 0.526→0.575; no correctness gain | coverage and answers measured apart | adopted |
| 2611.11840 | right by guess over insufficient history | 87 of 203 matched | the accuracy caveat | adopted |
| 2610.41172 | compilation is a supply layer, not a retriever | no accuracy gain on FRAMES; −91% tokens | the argument against compiling | adopted |
| 2610.22588 | lexical index, no LLM at index time, model at query time | recall@10 86.62% MuSiQue | the boundary: multi-hop wants a model | open |
| 2104.08663 | BM25 is a robust zero-shot baseline | rerank/late interaction best, expensive | BM25 first; neural opt-in | adopted |
| 2403.06789 | SPLADE-v3 beats BM25 | >40 MRR@10; +2 % OOD | not the default: needs a transformer | rejected for now |
| 2610.30612 | fusion helps in aggregate, hurts per query | 13–48 % demoted | `mem eval --compare` measures it | measured (#77) |
| 2605.10848 | tuned BM25 plus depth beats dense agents | 64→82 % accuracy; +25.3 % recall | settings shipped; no gain on LongMemEval | checked (#75, #78) |
| 2605.18561 | identifier tokenization is the transferable fix | +89.3 % NDCG@10 on code; ~0 on BEIR text | adjacent-token phrases | adopted (#76) |
| 2604.01733 | BM25 beats dense on financial documents | R@5 0.816 hybrid + rerank | BM25 first; rerank opt-in | adopted |
| 2204.12755 | dense underperforms BM25 zero-shot | — | qualifies the dense wins | adopted |
| Lv & Zhai 2011 | BM25 over-penalizes long documents | — | BM25+ in the grid; no gain here | checked (#75) |
| Kamphuis 2020 | variants and parameters change rankings | — | name both in every claim | adopted |
