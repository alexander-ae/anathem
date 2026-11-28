#!/usr/bin/env python3
"""How much of what a question needs would a greeting carry, by recency?

The greeting is a window: pinned memories first, then the newest within a
window, capped at `handoff_items`. That is orientation, not state, and this
measures the difference on a corpus that has dates and known evidence.

LongMemEval gives, per question, a dated history and the session that holds
the answer. For each question at its own date, build the greeting out of the
sessions before it and ask whether the evidence session is in it. Compare
that with the same number of items chosen by relevance — that is what
`search_memory` does, measured separately in `docs/benchmarks.md`.

    python3 tools/greeting_coverage.py longmemeval_s_cleaned.json

The answer, on LongMemEval-S: five items by recency reach 23.8 %, five items
by relevance reach 97.2 %. Recency is a very inefficient state — 80 % needs
half the history and 96 % needs all of it — and no selection rule that does
not look at the question gets close. Which is why the greeting is a window on
purpose, and why the marks added to each line say what the line is for.
"""

import json
import statistics
import sys
from datetime import datetime

# The greeting's own default, and the sizes worth seeing around it.
CAPS = (5, 8, 10, 20, 30, 40)


def moment(stamp: str) -> datetime:
    """`2023/05/30 (Tue) 23:40` -> datetime."""
    day, rest = stamp.split(" (", 1)
    return datetime.strptime(f"{day} {rest.split(')')[1].strip()}", "%Y/%m/%d %H:%M")


def policy_window(history, n):
    """The last n sessions, which is what the greeting does."""
    return history[-n:]


def policy_one_per_day(history, n):
    """The last session of each of the n most recent distinct days.

    The obvious alternative when the complaint is "it only covers two days":
    trade sessions for days. Measured, it loses, because a corpus clusters
    several sessions on a day and the window already spans those.
    """
    out, seen = [], set()
    for session in reversed(history):
        day = moment(session[1]).date()
        if day in seen:
            continue
        seen.add(day)
        out.append(session)
        if len(out) == n:
            break
    return out


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else "longmemeval_s_cleaned.json"
    with open(path) as fh:
        questions = json.load(fh)

    cases = []
    for q in questions:
        asked = moment(q["question_date"])
        history = [
            (sid, date)
            for sid, date in zip(q["haystack_session_ids"], q["haystack_dates"])
            if moment(date) < asked
        ]
        cases.append((history, set(q["answer_session_ids"])))

    total = len(cases)
    ages = []
    for q in questions:
        asked = moment(q["question_date"])
        dates = dict(zip(q["haystack_session_ids"], q["haystack_dates"]))
        ages.append(min((asked - moment(dates[s])).total_seconds() / 86400 for s in q["answer_session_ids"]))
    ages.sort()

    print(f"{total} questions, evidence age in days:")
    for bound in (1, 7, 30):
        within = sum(1 for a in ages if a <= bound)
        print(f"  at most {bound:>2} d: {within:>3} = {100 * within / total:5.1f} %")
    print(f"  median {statistics.median(ages):.1f}, p90 {ages[int(0.9 * len(ages))]:.1f}")

    print()
    print("cap  window  one-per-day  days-spanned")
    for cap in CAPS:
        window = sum(1 for h, g in cases if g & {s for s, _ in policy_window(h, cap)})
        per_day = sum(1 for h, g in cases if g & {s for s, _ in policy_one_per_day(h, cap)})
        days = statistics.median(
            len({moment(d).date() for _, d in policy_window(h, cap)}) for h, _ in cases
        )
        print(
            f"{cap:>3}  {100 * window / total:5.1f}%  {100 * per_day / total:10.1f}%  {days:>11.0f}"
        )

    whole = sum(1 for h, g in cases if g & {s for s, _ in h})
    print(f"{'all':>3}  {100 * whole / total:5.1f}%")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
