#!/usr/bin/env python3
"""Checks an eval set before you spend an hour measuring against it.

The format is the one `mem eval --evalset` reads and
`tools/longmemeval_to_memories.py` writes:

    {"corpus": "...", "cases": [
       {"id": "q1", "query": "...", "project": "p", "expect_docs": ["<ext_id or id:N>"],
        "kind": "multi-session"}]}

Structure is checked always. Pass `--db` as well and every `expect_docs`
entry is looked up in that database, which is the check worth having: a case
whose evidence is not in the corpus can never be answered, so it drags the
score down for a reason that has nothing to do with retrieval. That failure
is silent — the run completes, the number is simply wrong — and it is the one
a converter bug produces.

    python3 tools/validate_evalset.py bench/evalset.json
    python3 tools/validate_evalset.py bench/evalset.json --db bench.db

Exits 0 when the set is usable, 1 when it is not, 2 on a usage error.
"""
import collections
import json
import pathlib
import sqlite3
import sys


def load(path):
    try:
        data = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    except FileNotFoundError:
        print(f"no such file: {path}", file=sys.stderr)
        raise SystemExit(2)
    except json.JSONDecodeError as e:
        print(f"{path} is not valid JSON: {e}", file=sys.stderr)
        raise SystemExit(2)
    if not isinstance(data, dict) or not isinstance(data.get("cases"), list):
        print(f"{path} has no `cases` array", file=sys.stderr)
        raise SystemExit(2)
    return data


def check_structure(cases):
    errors = []
    seen_ids, seen_queries = set(), {}
    for n, c in enumerate(cases):
        cid = c.get("id") or f"<case {n}, no id>"
        if not c.get("id"):
            errors.append(f"{cid}: no id")
        elif c["id"] in seen_ids:
            errors.append(f"{cid}: duplicate id")
        seen_ids.add(c.get("id"))

        q = (c.get("query") or "").strip()
        if not q:
            errors.append(f"{cid}: empty query")
        else:
            # Two cases with the same query are two chances to be counted
            # for the same retrieval, which quietly weights it twice.
            key = " ".join(q.lower().split())
            if key in seen_queries:
                errors.append(f"{cid}: same query as {seen_queries[key]}")
            else:
                seen_queries[key] = cid

        exp = c.get("expect_docs")
        if not isinstance(exp, list) or not exp:
            errors.append(f"{cid}: expect_docs is empty, so nothing can score")
        elif len(set(exp)) != len(exp):
            errors.append(f"{cid}: expect_docs repeats an ext_id")
    return errors


def check_against_db(cases, db):
    """Every expectation has to exist, and has to be searchable.

    An entry is an ext_id, or `id:<number>` for a memories.id — the only way
    to name a memory anathem saved itself, since save_memory assigns no
    ext_id.
    """
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    except sqlite3.Error as e:
        print(f"could not open {db}: {e}", file=sys.stderr)
        raise SystemExit(2)
    try:
        rows = dict(
            conn.execute(
                "select ext_id, status from memories where ext_id is not null"
            ).fetchall()
        )
        rows.update(
            (f"id:{i}", s)
            for i, s in conn.execute("select id, status from memories").fetchall()
        )
    except sqlite3.Error as e:
        print(f"{db} does not look like an anathem database: {e}", file=sys.stderr)
        raise SystemExit(2)
    finally:
        conn.close()

    errors = []
    for c in cases:
        for d in c.get("expect_docs") or []:
            status = rows.get(d)
            if status is None:
                errors.append(f"{c.get('id')}: {d} is not in the database")
            elif status != "active":
                # Search filters `status = 'active'`, so one that is not
                # cannot come back however good the ranking is.
                errors.append(f"{c.get('id')}: {d} is {status}, so search will never return it")
    return errors, len(rows)


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    db = None
    rest = argv[1:]
    while rest:
        a = rest.pop(0)
        if a == "--db":
            if not rest:
                print("--db needs a path", file=sys.stderr)
                return 2
            db = rest.pop(0)
            if db in args:
                args.remove(db)
    if len(args) != 1:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        print(f"\nusage: {argv[0]} <evalset.json> [--db <mem.db>]", file=sys.stderr)
        return 2

    data = load(args[0])
    cases = data["cases"]
    errors = check_structure(cases)

    in_db = None
    if db:
        more, in_db = check_against_db(cases, db)
        errors += more

    print(f"cases: {len(cases)}")
    kinds = collections.Counter(c.get("kind") or "no kind" for c in cases)
    width = max(len(k) for k in kinds)
    for k, n in sorted(kinds.items()):
        print(f"  {k:<{width}}  {n:>5}  ({n / len(cases) * 100:.1f} %)")
    lengths = sorted(len(c.get("expect_docs") or []) for c in cases)
    if lengths:
        print(f"expect_docs: median {lengths[len(lengths) // 2]}, max {lengths[-1]}")
    referenced = {d for c in cases for d in (c.get("expect_docs") or [])}
    by_id = {d for d in referenced if d.startswith("id:")}
    print(f"distinct memories referenced: {len(referenced)}"
          + (f" ({len(by_id)} of them by id)" if by_id else ""))
    if in_db is not None:
        print(f"memories the database can match: {in_db}")
    if data.get("corpus"):
        print(f"corpus: {data['corpus']}")
    if not db:
        print("\nStructure only. Pass --db to check the evidence is really there.")

    if errors:
        print(f"\n{len(errors)} problems:", file=sys.stderr)
        for e in errors[:40]:
            print("  " + e, file=sys.stderr)
        if len(errors) > 40:
            print(f"  ... and {len(errors) - 40} more", file=sys.stderr)
        return 1
    print("\nOK")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
