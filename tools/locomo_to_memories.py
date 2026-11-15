#!/usr/bin/env python3
"""Turns LoCoMo into memories anathem can import and an eval set it can measure.

LoCoMo (https://github.com/snap-research/locomo) is the benchmark from
*Evaluating Very Long-Term Conversational Memory of LLM Agents* (Maharana et
al., ACL 2024). Ten conversations between two people, each running across 19
to 32 sessions and 370 to 690 turns, with 1,986 questions written against
them. Unlike LongMemEval it marks the evidence **per turn**: `D12:3` means
turn 3 of session 12.

**It is not redistributed here, and its licence is stricter than
LongMemEval's.** `data/locomo10.json` is CC BY-NC 4.0: the numbers measured
with this script can be published with attribution, the corpus cannot be
shipped with them. Download it yourself, and point this script at the file.

    python3 tools/locomo_to_memories.py locomo10.json bench/
    mem --db bench.db init
    mem --db bench.db import --from bench/memories
    mem --db bench.db eval --evalset bench/evalset.json --label "anathem bm25"

What it writes, in the frontmatter format `mem export` writes and `mem
import` reads:

  bench/memories/*.md   one file per session (or per turn, see below)
  bench/evalset.json    one case per answerable question

## The two units, and why the flag exists

`--unit session` (the default) puts one memory per session, which is what a
memory is in anathem and what the LongMemEval conversion does: a whole
conversation, both speakers, as one note. It is also the easier measurement —
27 sessions per question instead of 600 turns.

`--unit turn` puts one memory per dialog turn, which is the setting the
paper's own retrieval-augmented evaluation uses: its context ids are dia_ids.
Six hundred candidates per question, and a much harder number. Both are
honest; what they are not is interchangeable, and quoting one against the
other is how a benchmark gets misread.

## Four decisions the conversion makes

**Category 5 is not part of the set.** Those 446 questions are adversarial —
they have a `adversarial_answer` and no correct answer — and the 1,540-question
subset everyone quotes (including the vendors) is exactly the other four
categories. Four more questions carry no evidence at all and are skipped,
which is why 1,536 cases come out of 1,540.

**The images travel as captions.** The dataset ships no images, only a BLIP
caption per image turn, and 37.8% of the evidence turns carry one. The
paper's own evaluation reads the caption in place of the image; dropping it
would delete evidence and then blame the ranking for missing it.

**Every conversation is its own project.** A question is written against one
conversation, so its search is confined to that one, in both units. Pooling
the ten conversations and searching all of them measures something the
benchmark never defines.

**The date goes into the body, not only into `created`.** A third of LoCoMo's
questions ask *when* something happened, and the reference answers are written
the way the dataset writes its timestamps ("7 May 2023"). `get_memory` returns
the body and not the frontmatter, so a date that only lived in `created` would
never reach the model, and every temporal question would be unanswerable for a
reason that has nothing to do with retrieval. The paper's own evaluation
prefixes each turn with its session's date; this does the same, once per
session and once per turn.
"""
import json
import pathlib
import re
import sys

KINDS = {1: "multi-hop", 2: "temporal", 3: "open-domain", 4: "single-hop"}
#: Category 5 is adversarial: the question has a wrong answer planted for it
#: and no correct one, so it is not a case.
ADVERSARIAL = 5
MONTHS = {
    "Jan": "01", "Feb": "02", "Mar": "03", "Apr": "04", "May": "05", "Jun": "06",
    "Jul": "07", "Aug": "08", "Sep": "09", "Oct": "10", "Nov": "11", "Dec": "12",
}
DIA_ID = re.compile(r"D(\d+):(\d+)")


def iso_date(raw):
    """`1:56 pm on 8 May, 2023` -> `2023-05-08`.

    Anything unexpected falls back to the empty string rather than guessing:
    a wrong date would quietly reorder `mem recent` and `--since`.
    """
    m = re.search(r"(\d{1,2})\s+(\w+),?\s+(\d{4})", raw or "")
    if not m:
        return ""
    mon = MONTHS.get(m.group(2)[:3])
    return f"{m.group(3)}-{mon}-{int(m.group(1)):02d}" if mon else ""


def human_date(raw):
    """`1:56 pm on 8 May, 2023` -> `8 May 2023`.

    The date goes into the body, not only into `created`, because a third of
    LoCoMo's questions ask *when* something happened and the reference answers
    are written the same way ("7 May 2023"). A memory whose body carries no
    date cannot answer one of them, and `get_memory` returns the body and not
    the frontmatter, so the metadata alone would never reach the model.
    """
    m = re.search(r"(\d{1,2})\s+(\w+),?\s+(\d{4})", raw or "")
    if not m:
        return ""
    return f"{int(m.group(1))} {m.group(2)[:3]}. {m.group(3)}".replace(".", "")


def yaml_dq(s):
    """Quotes a frontmatter value the way `export::yaml_dq` does."""
    s = " ".join(str(s).split())
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def turn_text(turn):
    """One turn, with the image caption the dataset gives instead of the image."""
    text = f"{turn.get('speaker', '?')}: {turn.get('text', '').strip()}"
    caption = (turn.get("blip_caption") or "").strip()
    return f"{text} [shares {caption}]" if caption else text


def session_numbers(conv):
    return sorted(
        int(k.split("_")[1]) for k in conv if re.fullmatch(r"session_\d+", k)
    )


def evidence_ids(evidence):
    """The dia_ids in one question's `evidence` field, split on `;` and spaces.

    Six of the 1,540 questions carry the list in one string instead of a list
    — `["D8:6; D9:17"]`, `["D9:1 D4:4 D4:6"]` — and one carries `D`, which
    is not an id at all. Reading the field as it comes loses the second half
    of those questions' evidence, and dropping the question loses all of it,
    so the tokens are split and the malformed ones counted rather than
    hidden.
    """
    ids, malformed = [], []
    for entry in evidence or []:
        for token in re.split(r"[;,\s]+", str(entry)):
            if not token:
                continue
            if DIA_ID.fullmatch(token):
                ids.append(token)
            else:
                malformed.append(token)
    return ids, malformed


def render(frontmatter, body):
    head = "".join(f"{k}: {v}\n" for k, v in frontmatter)
    return f"---\n{head}---\n\n{body}\n"


def convert(samples, out, unit):
    memories = out / "memories"
    memories.mkdir(parents=True, exist_ok=True)
    cases = []
    written = 0
    counts = {}
    dropped = []

    for sample in samples:
        conv = sample["conversation"]
        sid = sample["sample_id"]

        for n in session_numbers(conv):
            turns = conv.get(f"session_{n}") or []
            created = iso_date(conv.get(f"session_{n}_date_time"))
            when = human_date(conv.get(f"session_{n}_date_time"))
            if unit == "turn":
                for turn in turns:
                    if not turn.get("text", "").strip():
                        continue
                    body = f"[{when}] {turn_text(turn)}" if when else turn_text(turn)
                    ext_id = f"{sid}/{turn['dia_id']}"
                    write(memories, ext_id, sid, created, body)
                    written += 1
                continue
            spoken = "\n".join(turn_text(t) for t in turns if t.get("text", "").strip())
            if not spoken:
                continue
            body = f"[{when}]\n{spoken}" if when else spoken
            write(memories, f"{sid}/session_{n}", sid, created, body, spoken)
            written += 1

        for i, qa in enumerate(sample["qa"]):
            if qa["category"] == ADVERSARIAL:
                continue
            evidence, malformed = evidence_ids(qa.get("evidence"))
            dropped.extend(malformed)
            if not evidence:
                continue
            if unit == "turn":
                expect = [f"{sid}/{e}" for e in evidence]
            else:
                sessions = sorted({int(DIA_ID.fullmatch(e).group(1)) for e in evidence})
                expect = [f"{sid}/session_{n}" for n in sessions]
            kind = KINDS.get(qa["category"], str(qa["category"]))
            counts[kind] = counts.get(kind, 0) + 1
            cases.append({
                "id": f"{sid}-{i}",
                "query": qa["question"],
                "project": sid,
                "expect_docs": expect,
                "kind": kind,
            })

    (out / "evalset.json").write_text(
        json.dumps({
            "corpus": "LoCoMo (https://github.com/snap-research/locomo), "
                      "CC BY-NC 4.0, (c) 2024 Maharana et al.",
            "unit": unit,
            "malformed_evidence_tokens": dropped,
            "cases": cases,
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return written, len(cases), counts, dropped


def write(memories, ext_id, project, created, body, summary=None):
    """`summary` is what the description is cut from, when the body opens with
    something that is not a summary of it — the date, in this conversion."""
    name = re.sub(r"[^A-Za-z0-9_.-]", "_", ext_id) + ".md"
    text = render([
        ("project", yaml_dq(project)),
        ("description", yaml_dq(" ".join((summary or body).split())[:120])),
        ("created", yaml_dq(created)),
        ("status", "active"),
        ("source", "migrated"),
        ("ext_id", yaml_dq(ext_id)),
    ], body)
    # newline="\n" explicitly: on Windows Python would translate it to \r\n and
    # `mem import` would read the file as having no frontmatter, dropping it
    # silently.
    with (memories / name).open("w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def main(argv):
    args, unit = [], "session"
    rest = argv[1:]
    while rest:
        a = rest.pop(0)
        if a == "--unit":
            if not rest:
                print("--unit needs `session` or `turn`", file=sys.stderr)
                return 2
            unit = rest.pop(0)
            if unit not in ("session", "turn"):
                print(f"--unit {unit} is not `session` or `turn`", file=sys.stderr)
                return 2
        else:
            args.append(a)

    if len(args) != 2:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        print(f"\nusage: {argv[0]} <locomo10.json> <output dir> "
              "[--unit session|turn]", file=sys.stderr)
        return 2

    src = pathlib.Path(args[0])
    samples = json.loads(src.read_text(encoding="utf-8"))
    if isinstance(samples, dict):
        samples = [samples]

    written, cases, counts, dropped = convert(samples, pathlib.Path(args[1]), unit)
    kinds = ", ".join(f"{k} {v}" for k, v in sorted(counts.items()))
    print(f"{written} memories, {cases} cases (unit={unit}: {kinds})")
    if dropped:
        print(f"{len(dropped)} malformed evidence tokens ignored: "
              + ", ".join(sorted(set(dropped))))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
