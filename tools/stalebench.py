#!/usr/bin/env python3
"""Turns a StaleBench release split into memories anathem can import, an eval
set it can measure, and a submission the frozen evaluator can score.

StaleBench (https://github.com/thenexuslab/stalebench-release, Apache-2.0 code,
CC0 data; Findings of EACL 2027) measures whether a memory returns a current
value when several versions of the same fact exist. Its release ships, per
configuration and split:

  statements_<config>_<split>.jsonl   the arrival stream: id, text, year, t
  queries_<config>_<split>.jsonl      the value-agnostic questions
  key_<config>_<split>.jsonl          fact_ids and current_ids, for scoring only

The key is not used at retrieval time. Here it is read only to label the
eval set and, with --mode superseded, to mark the obsolete statements the way
anathem would after a correction.

Two write policies:

  --mode store-all   every statement stays active. This measures the ranking
                     the way the paper's store-all baseline does.
  --mode superseded  current statements are active and the rest are
                     superseded, which is what anathem's deterministic
                     supersession leaves behind. Stale rows are still stored
                     and exported; they are not returned by search.

Usage:

  python3 tools/stalebench.py release/ --config anchored --split test --out bench/
  mem --db bench.db init
  mem --db bench.db import --from bench/memories
  mem --db bench.db eval --evalset bench/evalset.json --out bench/anathem.json

  python3 tools/stalebench.py release/ --config anchored --split test \
    --submission bench/anathem.json --out bench/submission.jsonl
  python3 release/evaluate.py --key release/key_anchored_test.jsonl \
    --submission bench/submission.jsonl

The statements are not redistributed with anathem. Download the release
yourself and point this script at the directory that holds the three files.
"""
import json
import pathlib
import re
import sys

STATUS_ACTIVE = "active"
STATUS_SUPERSEDED = "superseded"


def yaml_dq(value):
    """Quote a frontmatter value the way `mem export` does.

    Import undoes only `\\"` and `\\\\`, so a real newline would end the line
    and the rest would be read as another key.
    """
    value = " ".join(str(value).split())
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def file_name(statement_id):
    return re.sub(r"[^A-Za-z0-9_.-]", "_", statement_id) + ".md"


def read_jsonl(path):
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                yield json.loads(line)


def release_files(release, config, split):
    """The three files of one configuration and split, with a useful error."""
    names = {
        "statements": f"statements_{config}_{split}.jsonl",
        "queries": f"queries_{config}_{split}.jsonl",
        "key": f"key_{config}_{split}.jsonl",
    }
    paths = {}
    for kind, name in names.items():
        path = release / name
        if not path.is_file():
            raise SystemExit(
                f"missing {path}; the release directory has to contain "
                f"{name} (config={config}, split={split})"
            )
        paths[kind] = path
    return paths


def created_of(statement):
    """A date for the memory. The year is all StaleBench gives; the month and
    day are not measured by anything here."""
    year = statement.get("year")
    try:
        year = int(year)
    except (TypeError, ValueError):
        year = 2020
    return f"{year:04d}-01-01"


def load_key(path):
    """The statements the key knows, the current ones, and the queries."""
    known = set()
    current = set()
    queries = {}
    for record in read_jsonl(path):
        fact_ids = set(record.get("fact_ids") or [])
        current_ids = set(record.get("current_ids") or [])
        queries[record["query_id"]] = (fact_ids, current_ids)
        known.update(fact_ids)
        current.update(current_ids)
    return known, current, queries


def write_memories(statements, known, current, out, project, mode):
    memories = out / "memories"
    memories.mkdir(parents=True, exist_ok=True)
    written = 0
    for statement in statements:
        sid = statement["id"]
        text = (statement.get("text") or "").strip()
        if not text:
            continue
        status = STATUS_ACTIVE
        if mode == "superseded" and sid in known and sid not in current:
            status = STATUS_SUPERSEDED
        title = " ".join(text.split())[:120]
        body = (
            "---\n"
            f"project: {yaml_dq(project)}\n"
            f"description: {yaml_dq(title)}\n"
            f"created: {yaml_dq(created_of(statement))}\n"
            f"status: {status}\n"
            "source: migrated\n"
            f"ext_id: {yaml_dq(sid)}\n"
            "---\n\n"
            f"{text}\n"
        )
        # newline="\n" explicit: on Windows Python would write CRLF and
        # `mem import` would silently read the file as having no frontmatter.
        with (memories / file_name(sid)).open(
            "w", encoding="utf-8", newline="\n"
        ) as fh:
            fh.write(body)
        written += 1
    return written


def write_evalset(queries, key, out, project):
    cases = []
    for query in queries:
        qid = query["query_id"]
        fact_ids, current_ids = key[qid]
        stale_ids = sorted(fact_ids - current_ids)
        cases.append({
            "id": qid,
            "query": query["question"],
            "project": project,
            "expect_docs": sorted(current_ids),
            "stale_docs": stale_ids,
            "kind": query.get("relation") or "",
        })
    evalset = out / "evalset.json"
    evalset.write_text(
        json.dumps({
            "corpus": "StaleBench (https://github.com/thenexuslab/stalebench-release), "
                      "Apache-2.0 code, CC0 data",
            "cases": cases,
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return len(cases), evalset


def write_submission(result_path, out):
    """`mem eval --out` -> the frozen evaluator's one-ranking-per-query JSONL.

    `returned` is already in rank order and carries the statement ids as
    ext_ids. Ten entries are enough: StaleBench scores the top five.
    """
    result = json.loads(pathlib.Path(result_path).read_text(encoding="utf-8"))
    lines = []
    for case in result.get("per_case", []):
        lines.append(json.dumps({
            "query_id": case["id"],
            "ranking": [name for name in case.get("returned", []) if name],
        }, ensure_ascii=False))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")
    return out, len(lines)


def main(argv):
    args = list(argv[1:])
    release = None
    config = "anchored"
    split = "test"
    out = pathlib.Path("stalebench-bench")
    project = None
    mode = "store-all"
    submission = None

    while args:
        arg = args.pop(0)
        if arg in ("-h", "--help"):
            print(__doc__)
            return 0
        if arg == "--config" and args:
            config = args.pop(0)
        elif arg == "--split" and args:
            split = args.pop(0)
        elif arg == "--out" and args:
            out = pathlib.Path(args.pop(0))
        elif arg == "--project" and args:
            project = args.pop(0)
        elif arg == "--mode" and args:
            mode = args.pop(0)
        elif arg == "--submission" and args:
            submission = args.pop(0)
        elif release is None:
            release = pathlib.Path(arg)
        else:
            raise SystemExit(f"unexpected argument: {arg}")

    if release is None:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        return 2
    if mode not in ("store-all", "superseded"):
        raise SystemExit("--mode has to be store-all or superseded")

    if project is None:
        project = f"stalebench-{config}-{split}"

    if submission:
        path = out if out.suffix == ".jsonl" else out / "submission.jsonl"
        path, n = write_submission(submission, path)
        print(f"{n} rankings -> {path}")
        return 0

    out.mkdir(parents=True, exist_ok=True)
    files = release_files(release, config, split)
    known, current, key = load_key(files["key"])
    statements = list(read_jsonl(files["statements"]))
    queries = list(read_jsonl(files["queries"]))
    missing = [q["query_id"] for q in queries if q["query_id"] not in key]
    if missing:
        raise SystemExit(f"the key has no entry for {len(missing)} queries, e.g. {missing[0]}")

    written = write_memories(statements, known, current, out, project, mode)
    cases, evalset = write_evalset(queries, key, out, project)
    print(f"{written} statements -> {out / 'memories'}")
    print(f"{cases} cases -> {evalset}")
    print(f"project: {project}; mode: {mode}")
    print("\nnext:")
    print(f"  mem --db {out / 'bench.db'} init")
    print(f"  mem --db {out / 'bench.db'} import --from {out / 'memories'}")
    print(f"  mem --db {out / 'bench.db'} eval --evalset {evalset} "
          f"--out {out / 'anathem.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
