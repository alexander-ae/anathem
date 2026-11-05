#!/usr/bin/env python3
"""recall@5 by question type, in the three definitions `docs/benchmarks.md` uses.

`mem eval` prints one of them: recall@5 as "any evidence session in the top
5". The tables in `docs/benchmarks.md` also quote the other two, and this is
what computes them, so the published numbers can be recomputed instead of
taken on trust:

  * **any** — at least one evidence session in the top 5. The generous bound,
    and what `mem eval` reports.
  * **fraction** — the mean fraction of a question's evidence inside the top
    5. The textbook Recall@k for a query with several relevant documents, and
    the row to quote next to anybody else's number.
  * **every** — every evidence session in the top 5. What answering a
    multi-session question actually needs.

All three come from the `per_case` block that `mem eval --out` writes, where
`returned` keeps the ext_ids in rank order.

    mem --db bench.db eval --evalset bench/evalset.json \\
        --label "bm25" --out bench/bm25.json
    python3 tools/eval_metrics.py bench/evalset.json bench/bm25.json bench/hybrid.json

Several result files are printed side by side, in the order given.
"""
import collections
import json
import sys


def measures(path, evalset):
    """(result, {kind: [fraction]}, {kind: [0 or 1]}) for one result file."""
    cases = {c["id"]: c for c in json.load(open(evalset, encoding="utf-8"))["cases"]}
    r = json.load(open(path, encoding="utf-8"))
    fraction = collections.defaultdict(list)
    every = collections.defaultdict(list)
    for c in r["per_case"]:
        expected = set(cases[c["id"]]["expect_docs"])
        if not expected:
            continue
        top5 = set(c["returned"][:5])
        kind = c["kind"] or "no kind"
        fraction[kind].append(len(expected & top5) / len(expected))
        every[kind].append(1.0 if expected <= top5 else 0.0)
    return r, fraction, every


def mean(d, kind):
    v = d.get(kind)
    return sum(v) / len(v) * 100 if v else None


def main(argv):
    if len(argv) < 3:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        print(
            f"\nusage: {argv[0]} <evalset.json> <result.json> [result.json ...]",
            file=sys.stderr,
        )
        return 2

    evalset = argv[1]
    rows = [measures(p, evalset) for p in argv[2:]]
    kinds = sorted({k for _, f, _ in rows for k in f})
    if not kinds:
        print("no scored cases: does the eval set carry expect_docs?", file=sys.stderr)
        return 1
    width = max(len(k) for k in kinds) + 2

    for title, which in (("fraction of the evidence", 1), ("every evidence session", 2)):
        print(f"\nrecall@5, {title}")
        print(f"{'':<{width}}{'n':>6}" + "".join(f"{r['system']:>22}" for r, _, _ in rows))
        for k in kinds:
            line = f"{k:<{width}}{len(rows[0][1].get(k, [])):>6}"
            for row in rows:
                v = mean(row[which], k)
                line += f"{v:>20.1f} %" if v is not None else f"{'-':>22}"
            print(line)
        total = sum(len(v) for v in rows[0][1].values())
        line = f"{'all':<{width}}{total:>6}"
        for row in rows:
            vs = [x for v in row[which].values() for x in v]
            line += f"{sum(vs) / len(vs) * 100:>20.1f} %"
        print(line)

    print()
    for r, _, _ in rows:
        print(
            f"{r['system']}: n={r['n_cases']}  recall@5(any)={r['recall_at_5'] * 100:.1f}%  "
            f"MRR={r['mrr']:.3f}  p50={r['p50_ms']:.0f}ms"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
