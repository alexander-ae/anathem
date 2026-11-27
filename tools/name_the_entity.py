#!/usr/bin/env python3
"""Does "name the entity in your query" produce a query that finds more?

#81: the protocol tells the model to query with the nouns the note would use,
but not to resolve «the client» or «that service» into a name. MeMo does that
as an explicit stage. This measures whether the sentence is worth its tokens,
by asking a model to write the search query two ways and running both against
the same memory.

    python3 tools/name_the_entity.py <evalset.json> <db> --n 40

Arm A: "write a search query for this question". Arm B: the same, plus the
sentence under test. The metric is the memory's rank in the results of the
query each arm wrote — not whether the model answered well, because the query
is the only thing the sentence changes.
"""
import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
import urllib.request

BASE = os.environ.get("NAME_BASE", "https://api.deepseek.com/v1")
MODEL = os.environ.get("NAME_MODEL", "deepseek-flash")

BASE_SYSTEM = (
    "You turn a question into a search query for a tool that stores notes. "
    "Answer with the query only: a few words, no punctuation, no explanation."
)
EXTRA = (
    " If the question refers to a person, a company or a thing indirectly — "
    "'the client', 'that service', 'the person from the meeting' — name it in "
    "the query using the words the conversation used."
)


def ask(system, question):
    payload = json.dumps({
        "model": MODEL,
        "messages": [{"role": "system", "content": system},
                     {"role": "user", "content": question}],
        "temperature": 0,
        "thinking": {"type": "disabled"},
        "max_tokens": 60,
    }).encode()
    req = urllib.request.Request(
        f"{BASE.rstrip('/')}/chat/completions", data=payload,
        headers={"Content-Type": "application/json",
                 "Authorization": f"Bearer {os.environ['DEEPSEEK_API_KEY']}"})
    with urllib.request.urlopen(req, timeout=90) as r:
        d = json.loads(r.read())
    q = d["choices"][0]["message"]["content"].strip().splitlines()[0]
    return re.sub(r"^[\"'`]|[\"'`]$", "", q).strip()


def rank(db, query, project, expect):
    out = subprocess.run(
        ["mem", "--db", str(db), "search", query, "--project", project, "--json"],
        capture_output=True, text=True).stdout
    try:
        hits = json.loads(out)
    except json.JSONDecodeError:
        return None
    ids = [str(h.get("id")) for h in (hits.get("results") or hits if isinstance(hits, list) else [])]
    # The eval set names ext_ids; the JSON search returns memory ids, so the
    # caller passes the numbers it expects. See the caller.
    return ids, expect


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("evalset")
    ap.add_argument("db")
    ap.add_argument("--n", type=int, default=40)
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    cases = json.load(open(args.evalset))["cases"]
    step = max(1, len(cases) // args.n)
    cases = cases[::step][: args.n]

    rows = []
    for c in cases:
        a = ask(BASE_SYSTEM, c["query"])
        b = ask(BASE_SYSTEM + EXTRA, c["query"])
        rows.append({"id": c["id"], "question": c["query"],
                     "expected": c["expect_docs"], "plain": a, "named": b})
        print(f"{c['id']}: {a!r} | {b!r}", file=sys.stderr)

    if args.out:
        json.dump(rows, open(args.out, "w"), ensure_ascii=False, indent=1)
    print(json.dumps({"queries": len(rows)}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
