#!/usr/bin/env python3
"""Turns LongMemEval into memories anathem can import and an eval set it can measure.

LongMemEval (https://github.com/xiaowu0162/LongMemEval, MIT, (c) 2024 Di Wu)
gives every question its own chat history and marks which sessions hold the
evidence. That is the same shape anathem's eval harness wants: a query, and
the documents it should find.

The dataset is NOT redistributed with anathem. Download it yourself from the
Hugging Face dataset `xiaowu0162/longmemeval-cleaned` and point this script at
the file:

    python3 tools/longmemeval_to_memories.py longmemeval_s_cleaned.json bench/
    mem --db bench.db init
    mem --db bench.db import --from bench/memories
    mem --db bench.db eval --evalset bench/evalset.json --label "anathem bm25"

`--questions N` keeps N instances spread evenly through the file, corpus
included — not the first N, which in `longmemeval_m` would be nothing but
`single-session-*` questions. It exists
for `longmemeval_m_cleaned.json`, where each question drags around 500
haystack sessions and all 500 questions do not fit on a laptop: what makes -M
the harder benchmark is the depth of one haystack, not how many there are.

`--distractors FRACTION` keeps every evidence session and an evenly spaced
share of the rest. It is how the synthetic scope in `docs/benchmarks.md` is
built: the most a perfect scope could do is drop distractors while keeping
all the evidence, so a corpus shaped that way bounds what scoping could buy.
It is a bound and not a product — nothing in anathem builds a corpus this way
for a user.

What it writes:

  bench/memories/*.md   one file per history session, in the frontmatter
                        format `mem export` writes and `mem import` reads
  bench/evalset.json    one case per answerable question

Two decisions worth knowing:

  * Every memory is namespaced under its question's id, in both `project` and
    `ext_id`. LongMemEval session ids repeat across instances, and
    `memories.ext_id` is unique for the whole database: without the namespace
    every collision would be dropped on import, and the dropped one could be
    the evidence.

  * Each case names its project, so the search is confined to that question's
    own history. Pooling 500 haystacks and searching all of them measures
    something harder than the benchmark defines, and the number would not be
    comparable with anything published.
"""
import json
import pathlib
import re
import sys

# Instances whose id ends in this have no ground-truth evidence: the right
# answer is to abstain. There is nothing to retrieve, so they are not cases.
ABSTENTION_SUFFIX = "_abs"


def iso_date(raw):
    """`2023/05/01 (Mon) 09:00` -> `2023-05-01`.

    The dataset's own format carries a weekday in parentheses that no date
    parser wants. Anything unexpected falls back to the empty string rather
    than guessing: a wrong date would quietly reorder `mem recent`.
    """
    m = re.match(r"\s*(\d{4})/(\d{2})/(\d{2})", raw or "")
    return f"{m.group(1)}-{m.group(2)}-{m.group(3)}" if m else ""


def yaml_dq(s):
    """Quotes a frontmatter value the way `export::yaml_dq` does.

    `import::unquote` only undoes `\\"` and `\\\\`, so the value must not
    carry a real newline: it would end the line and the rest would be read as
    another key.
    """
    s = " ".join(str(s).split())
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def render(session):
    """One session as text, both speakers kept.

    Evidence lands in assistant turns too — `single-session-assistant` is one
    of the question types — so a corpus of only the user's half would make
    some questions unanswerable and the measurement meaningless.
    """
    return "\n\n".join(
        f"{t.get('role', 'user')}: {t.get('content', '').strip()}"
        for t in session
        if t.get("content", "").strip()
    )


def file_name(ext_id):
    """A flat, collision-free file name for an ext_id that carries a slash."""
    return re.sub(r"[^A-Za-z0-9_.-]", "_", ext_id) + ".md"


def kept_sessions(ids, sessions, dates, evidence, fraction):
    """Every evidence session, plus an evenly spaced share of the rest.

    This is what the research issue calls a synthetic scope: the best case a
    perfect scoping could reach is all of the evidence and fewer of the
    distractors. Dropping an evidence session would not be a bound on that —
    it would be a different and worse benchmark — so evidence is never a
    candidate for removal.

    Evenly spaced rather than sampled, for the reason `--questions` is:
    there is no seed to write down, and two people running the same command
    get the same corpus.
    """
    if fraction is None:
        return list(zip(ids, sessions, dates))

    rows = list(zip(ids, sessions, dates))
    evidence = set(evidence or [])
    keep = [r for r in rows if r[0] in evidence]
    others = [r for r in rows if r[0] not in evidence]
    n = int(len(others) * fraction)
    total = len(others)
    if n and total:
        keep += [others[(i * total) // n] for i in range(n)]
    # Back into the original order, so the corpus does not depend on which
    # sessions happened to be evidence.
    order = {sid: i for i, sid in enumerate(ids)}
    keep.sort(key=lambda r: order[r[0]])
    return keep


def convert(instances, out, distractors=None):
    memories = out / "memories"
    memories.mkdir(parents=True, exist_ok=True)
    cases = []
    # ext_id -> body already written. LongMemEval lists the same session
    # twice inside one instance in 13 of its 500 cases. Writing it twice is
    # harmless when the body matches; writing a *different* body over it
    # loses a memory, and the lost one can be the evidence the question is
    # scored on, so that stops the run instead of skewing the number.
    seen = {}
    duplicates = 0
    conflicts = []

    for inst in instances:
        qid = inst["question_id"]
        evidence = inst.get("answer_session_ids") or []
        answerable = not qid.endswith(ABSTENTION_SUFFIX) and evidence

        ids = inst.get("haystack_session_ids") or []
        sessions = inst.get("haystack_sessions") or []
        dates = inst.get("haystack_dates") or []
        # `dates` is shorter than `ids` in some instances; padded here so the
        # three can travel together through the filter below.
        dates = list(dates) + [""] * (len(ids) - len(dates))

        for sid, session, date in kept_sessions(ids, sessions, dates, evidence, distractors):
            body = render(session)
            if not body:
                continue
            ext_id = f"{qid}/{sid}"
            if ext_id in seen:
                if seen[ext_id] == body:
                    duplicates += 1
                else:
                    conflicts.append(ext_id)
                continue
            seen[ext_id] = body
            created = iso_date(date)
            title = " ".join(body.split())[:120]
            text = (
                "---\n"
                f"project: {yaml_dq(qid)}\n"
                f"description: {yaml_dq(title)}\n"
                f"created: {yaml_dq(created)}\n"
                "status: active\n"
                "source: migrated\n"
                f"ext_id: {yaml_dq(ext_id)}\n"
                "---\n\n"
                f"{body}\n"
            )
            # newline="\n" explicito: en Windows, Python traduce \n a \r\n al
            # escribir, y `mem import` lee el archivo como si no tuviera
            # frontmatter y lo saltea en silencio.
            with (memories / file_name(ext_id)).open(
                    "w", encoding="utf-8", newline="\n") as fh:
                fh.write(text)

        if answerable:
            cases.append({
                "id": qid,
                "query": inst["question"],
                "project": qid,
                "expect_docs": [f"{qid}/{s}" for s in evidence],
                "kind": inst.get("question_type", ""),
            })

    (out / "evalset.json").write_text(
        json.dumps({
            "corpus": "LongMemEval (https://github.com/xiaowu0162/LongMemEval), MIT, (c) 2024 Di Wu",
            "cases": cases,
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return len(seen), len(cases), duplicates, conflicts


def main(argv):
    args, questions, distractors = [], None, None
    rest = argv[1:]
    while rest:
        a = rest.pop(0)
        if a == "--questions":
            if not rest:
                print("--questions needs a number", file=sys.stderr)
                return 2
            questions = int(rest.pop(0))
        elif a == "--distractors":
            if not rest:
                print("--distractors needs a fraction between 0 and 1", file=sys.stderr)
                return 2
            distractors = float(rest.pop(0))
            if not 0.0 <= distractors <= 1.0:
                print(
                    f"--distractors {distractors} is not a fraction between 0 and 1",
                    file=sys.stderr,
                )
                return 2
        else:
            args.append(a)

    if len(args) != 2:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        print(
            f"\nusage: {argv[0]} <longmemeval file.json> <output dir> "
            "[--questions N] [--distractors FRACTION]",
            file=sys.stderr,
        )
        return 2

    src = pathlib.Path(args[0])
    instances = json.loads(src.read_text(encoding="utf-8"))
    if isinstance(instances, dict):
        instances = [instances]

    if questions is not None and questions < len(instances):
        # Evenly spaced, NOT the first N. The file is sorted by question
        # type: the first 100 instances of longmemeval_m are entirely
        # `single-session-*`, the two types that are easiest to retrieve,
        # while the file as a whole is 54% `multi-session` and
        # `temporal-reasoning`. A prefix would measure the easy stratum and
        # call it the benchmark.
        #
        # Spacing rather than a random sample so there is no seed to write
        # down: the same N hands anyone repeating the run the same corpus.
        #
        # Why subset at all: LongMemEval-M carries around 476 haystack
        # sessions per question, and all 500 questions of that is 237,655
        # sessions. What makes -M the harder benchmark is the depth of one
        # haystack, not how many there are, so fewer questions keeps the
        # comparison and drops the cost.
        total = len(instances)
        instances = [instances[(i * total) // questions] for i in range(questions)]

    written, cases, duplicates, conflicts = convert(
        instances, pathlib.Path(args[1]), distractors
    )
    if conflicts:
        print(
            "the same session id carries two different bodies, so one would be\n"
            "silently lost — and the lost one can be the evidence a question is\n"
            "scored on:\n  " + "\n  ".join(sorted(set(conflicts))),
            file=sys.stderr,
        )
        return 1

    skipped = len(instances) - cases
    extra = f", {duplicates} duplicate sessions collapsed" if duplicates else ""
    print(
        f"{written} memories, {cases} cases, "
        f"{skipped} instances skipped (abstention or no evidence){extra}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
