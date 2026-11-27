#!/usr/bin/env python3
"""Writes two anticipated questions per memory into a copy of the corpus.

For #82: the measurement of whether questions at save time move recall. The
questions are generated once, offline, by the same model for every memory, so
the comparison between the two corpora is about the index and not about the
model.

    python3 tools/anticipate_questions.py <memories dir> <out dir>

Reads each `.md` frontmatter, asks the model for two questions in a person's
voice, and writes the same memory back with a `questions:` list. Needs
DEEPSEEK_API_KEY (or ANTHROPIC/OPENAI style keys are not used on purpose: one
model, one run).
"""
import concurrent.futures
import json
import os
import pathlib
import re
import sys
import urllib.request

BASE = os.environ.get("ANTICIPATE_BASE", "https://api.deepseek.com/v1")
MODEL = os.environ.get("ANTICIPATE_MODEL", "deepseek-flash")

SYSTEM = (
    "You write the questions a person would ask later that a piece of text answers. "
    "Two per text, one per line, no numbering, no quotes. Vary them: one names the "
    "concrete thing (a product, a place, a person, a command), the other describes the "
    "situation without naming it. At most 12 words each, in the language of the text."
)


def ask(text):
    payload = json.dumps({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": text[:6000]},
        ],
        "temperature": 0.3,
        "thinking": {"type": "disabled"},
        "max_tokens": 120,
    }).encode()
    req = urllib.request.Request(
        f"{BASE.rstrip('/')}/chat/completions", data=payload,
        headers={"Content-Type": "application/json",
                 "Authorization": f"Bearer {os.environ['DEEPSEEK_API_KEY']}"})
    with urllib.request.urlopen(req, timeout=90) as r:
        d = json.loads(r.read())
    out = d["choices"][0]["message"]["content"]
    qs = [re.sub(r"^\s*(?:[-*]|\d+[.)])\s*", "", l).strip().strip('"') for l in out.splitlines()]
    return [q for q in qs if 3 <= len(q) <= 160][:2]


def main(argv):
    src, dst = pathlib.Path(argv[1]), pathlib.Path(argv[2])
    dst.mkdir(parents=True, exist_ok=True)
    files = sorted(src.glob("*.md"))
    print(f"{len(files)} memories", file=sys.stderr)

    def one(p):
        text = p.read_text()
        parts = text.split("---\n")
        if len(parts) < 3:
            return None
        head, body = parts[1], "---\n".join(parts[2:])
        try:
            qs = ask(body)
        except Exception as e:  # noqa: BLE001 - reported and skipped
            print(f"failed {p.name}: {e}", file=sys.stderr)
            return None
        if not qs:
            return None
        lines = "".join(f'  - {json.dumps(q, ensure_ascii=False)}\n' for q in qs)
        (dst / p.name).write_text(f"---\n{head}questions:\n{lines}---\n{body}")
        return p.name

    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        done = sum(1 for r in pool.map(one, files) if r)
    print(f"{done} with questions -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv)
