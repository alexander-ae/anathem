# Vocabulary packs

anathem's search works without any model: BM25 over FTS5, zero downloads. A
vocabulary pack adds a vector layer that finds memories written in one
language when you ask in another.

**Languages are not modules.** The model is a single table with a shared
vector space, distilled over all languages at once, and that joint distillation
is what aligns a term with its translation. You cannot add a language to an
existing pack. What you choose is **which portion of the table gets
downloaded**, and adding a language means generating a new pack that includes
it.

## Why a pack is so much smaller

The base model has 500,353 vocabulary entries and weighs 512 MB in fp32.
English and Spanish together exercise **15 % of those entries**. Trimming the
rest and quantizing to int8 gives a 22.5 MB pack that performs as well as the
full model:

| | disk | mMARCO en | mMARCO es | real corpus es | real corpus en |
|---|---|---|---|---|---|
| full model, int8 | 128 MB | 69 % | 48 % | 60 % | 62 % |
| `en-es` pack | **22.5 MB** | 68 % | 48 % | 62 % | 64 % |

(recall@5; the differences are within measurement noise)

The trimming does not lose quality because the tokenizer is a **Unigram with
byte fallback**. When pieces are removed, the algorithm re-segments the text
among the ones that remain instead of producing unknowns, and any character
without a piece of its own falls back to the byte tokens, which the generator
always keeps. On top of that, the quantization is recalibrated over the rows
that survive, so the int8 grid ends up finer exactly where it matters.

None of this would apply to a transformer: its weights are shared across all
tokens and do not admit per-word trimming.

## Generating a pack for other languages

You need a sample of generic text in the languages you want to cover: one
plain text file per language, one line per passage. Any public corpus will do
— Wikipedia, mMARCO, MIRACL — with tens of thousands of lines. The sample only
determines which vocabulary pieces are kept, so it is worth making it varied
and **not the corpus you are going to index afterwards**.

    python3 tools/build_pack.py \
        --nombre en-es-pt \
        --muestra textos_en.txt textos_es.txt textos_pt.txt \
        --salida en-es-pt.anathem-pack

It writes the pack and a `.sha256` next to it. The size grows much less than
linearly with the number of languages, because they share pieces: going from
`en-es` to the full Latin script (which also covers Portuguese, French,
Italian and German) takes the table from 19.5 MB to 70.7 MB.

After generating one, measure it before publishing it. A pack with a badly
chosen sample does not fail: it performs worse, and silently.

## Changing packs forces a reindex

Two different packs produce vectors that are not comparable with each other.
anathem records in the database which pack the index was built with and
**reindexes when it detects a change, it never mixes**. Reindexing costs about
0.3 seconds per 10,000 chunks, so changing packs is cheap.

## The file format

A single file, all little endian:

| field | type | notes |
|---|---|---|
| magic | 4 bytes | `ANPK` |
| version | u32 | 1 |
| dimensions | u32 | 256 in the base model |
| tokens | u32 | rows of the table |
| name length | u32 | |
| name | UTF-8 bytes | identifier, e.g. `en-es` |
| tokenizer length | u64 | |
| tokenizer | UTF-8 bytes | trimmed `tokenizer.json` |
| scales | f32 × dimensions | one per dimension |
| minimums | f32 × dimensions | one per dimension |
| table | u8 × tokens × dimensions | row major |

To recover a vector: `value = byte * scale[d] + minimum[d]`. The embedding of
a text is the average of the rows of its tokens, normalized.

The `.sha256` covers the whole file, and it has to travel beside the pack:
`mem model install` verifies it, rejects the pack if it does not match, and
refuses to install one that arrives without it. Moving a pack somewhere else
means moving two files, not one. `--no-verify` installs anyway, for the case
where you know where the pack came from and the sidecar was lost on the way.

What this gives is integrity, not authenticity: whoever wrote the pack wrote
the sidecar. It catches a truncated copy, not a substituted one.

## What having several sessions open costs


Each agent launches its own `mem serve`, so the memory cost multiplies per
session. Two things keep it small:

- **The pack is only opened when searching.** A session that never searches
  stays at about 9 MB.
- **And it is released after a while without searching.** The tokenizer
  costs about 55 MB of private heap, and a session that searched once and
  then sat still would keep paying for it until it closed.

Measured over a real session:

```
at startup              9 MB
after searching        64 MB   (65 ms)
after 5 min idle       12 MB
on searching again     64 MB   (60 ms)
```

So sitting still costs 12 MB and coming back costs 60 ms, once. The delay is
tuned in `config.json`; with `0` it is never released:

```json
{ "release_pack_after": 300 }
```

Releasing it includes asking the allocator to return the pages to the
system. Without that step the `drop` frees the memory for the program but
glibc keeps it in its arena and the RSS does not go down: measured, 64 MB
became 57 instead of 12. That call is glibc's; on musl, macOS and Windows it
does not exist, and there releasing the pack frees the mapping all the same
but the rest is left to the platform allocator's discretion.

