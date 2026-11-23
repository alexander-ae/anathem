#!/usr/bin/env python3
"""Does `[anchor: ...]` change what a model does? A two-arm test on real history.

The anchor records that a memory was true against a function, and `mem verify
--check` says later whether that function still matches. What this measures is
the step after: given a memory whose anchor moved, does a model stop treating
the memory as fact? And given one whose anchor is fine, does the flag stay out
of the way?

Corpus, straight from a git history — no model generates it:

  * a memory says "the function `name` in `path` contains `N`", where N is a
    numeric literal in that function's body at an old commit;
  * at HEAD the same function either still contains N (the claim survived) or
    does not, or is gone (the claim is false);
  * the anchor hash is of the whole body, so a function whose body changed but
    whose N survived is a **false alarm**: the label says "moved" and the claim
    is still true. That case is the reason this exists — a flag that cries
    wolf is worse than none.

Groups per memory:

    false_claim        N changed or the function is gone at HEAD
    true_quiet         N survived and the body did not change
    true_noisy         N survived but the body changed (the label will say moved)

Arms: `label` off, and on with the exact string the MCP response emits.

    python3 tools/anchor_eval.py build --repo ~/proyectos/anathem --out /tmp/anchor-eval.json
    python3 tools/anchor_eval.py run --evalset /tmp/anchor-eval.json \\
        --base-url https://api.deepseek.com/v1 --model deepseek-flash \\
        --key-env DEEPSEEK_API_KEY --budget-usd 2

`build` needs no network and no key. `run` spends the caller's budget and
refuses to start when the estimate exceeds it.
"""
import argparse
import collections
import concurrent.futures
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.request

LABEL = "[anchor: the file changed]"
LABEL_GONE_FILE = "[anchor: the file is gone]"
LABEL_GONE_FUNC = "[anchor: the function is gone]"

PRICES = {
    # per million tokens, input/output. DeepSeek Flash, read 2026-11-23.
    "deepseek-flash": (0.15, 0.60),
    "deepseek-v4-pro": (0.66, 1.98),
}

CAVEAT = re.compile(
    r"(could have changed|may have changed|might have changed|out of date|outdated|"
    r"stale|can't confirm|cannot confirm|can not confirm|older version|changed since|"
    r"no longer|may not be accurate|not sure|cannot verify|can't verify|"
    r"desactualiz|puede haber cambiado|no puedo confirmar|ya no|versi[oó]n anterior)",
    re.I,
)


def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)


def body_hashes(text):
    """{function name: (block, hash)} for the functions a file defines."""
    out = {}
    for m in re.finditer(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)", text, re.M):
        name = m.group(1)
        i = text.find("{", m.end())
        if i < 0:
            continue
        depth = 0
        for j in range(i, len(text)):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    block = text[i:j + 1]
                    out[name] = (block, hashlib.sha256(block.encode()).hexdigest())
                    break
    return out


def numbers(block):
    """The numbers a claim can be built on, and that a reader can check."""
    return re.findall(r"\b(\d{2,4})\b", block)


def build(repo, min_days, max_days, per_group):
    """One item per function, from the oldest version of it in the window.

    Scanning newest-first and keeping the first sighting would classify every
    function by its most recent body, which is the one most likely to equal
    HEAD: the sample would be all `true_quiet` and the two interesting groups
    would never fill. So the *oldest* version in the window is the memory, and
    it is classified against HEAD afterwards.
    """
    log = git(repo, "log", "--format=@%ct %H", "--name-only").stdout
    commits, cur = [], None
    for line in log.splitlines():
        if line.startswith("@"):
            ts, sha = line[1:].split()
            cur = (int(ts), sha, [])
            commits.append(cur)
        elif line.strip() and cur is not None:
            cur[2].append(line.strip())
    head = max(c[0] for c in commits)

    oldest = {}
    for ts, sha, files in commits:
        age = (head - ts) / 86400
        if not (min_days <= age <= max_days):
            continue
        for path in files:
            if not path.endswith(".rs") or path.startswith("tests/"):
                continue
            r = git(repo, "show", f"{sha}:{path}")
            if r.returncode != 0:
                continue
            for name, (block, digest) in body_hashes(r.stdout).items():
                vals = numbers(block)
                if vals:
                    # newest first: the last write wins, which is the oldest
                    # version seen inside the window.
                    oldest[(path, name)] = (ts, block, digest, vals)

    head_src = {}

    def at_head(path):
        if path not in head_src:
            r = git(repo, "show", f"HEAD:{path}")
            head_src[path] = r.stdout if r.returncode == 0 else None
        return head_src[path]

    items = []
    counts = collections.Counter()
    for (path, name), (ts, block, digest, vals) in oldest.items():
        if counts.total() >= per_group * 3:
            break
        src = at_head(path)
        now = body_hashes(src) if src else {}
        if src is None:
            group, label, value = "false_claim", LABEL_GONE_FILE, vals[0]
        elif name not in now:
            group, label, value = "false_claim", LABEL_GONE_FUNC, vals[0]
        elif now[name][1] == digest:
            group, label, value = "true_quiet", "", vals[0]
        else:
            fresh = numbers(now[name][0])
            kept = [v for v in vals if v in fresh]
            gone = [v for v in vals if v not in fresh]
            if gone:
                # The body changed and this number did not survive it: the
                # claim is false, which is the case the flag exists for.
                group, label, value = "false_claim", LABEL, gone[0]
            else:
                # Every number survived: the label will say "changed" and the
                # claim is still true. That is the false alarm, and it has to
                # be counted rather than hidden.
                group, label, value = "true_noisy", LABEL, kept[0]
        if counts[group] >= per_group:
            continue
        counts[group] += 1
        age = (head - ts) / 86400
        items.append({
            "group": group,
            "path": path,
            "function": name,
            "value": value,
            "age_days": round(age),
            "memory": f"In `{path}`, the function `{name}` contains `{value}`.",
            "label": label,
            "question": f"What number does `{name}` in `{path}` contain? Answer in one short sentence.",
        })
    return items


def ask(base_url, key, model, system, user, max_tokens=200):
    payload = json.dumps({
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "temperature": 0,
        # Flash reasons by default, and reasoning tokens count against
        # max_tokens: the first run returned empty answers for that reason.
        "thinking": {"type": "disabled"},
        "max_tokens": max_tokens,
    }).encode()
    req = urllib.request.Request(
        f"{base_url.rstrip('/')}/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"},
    )
    with urllib.request.urlopen(req, timeout=90) as r:
        d = json.loads(r.read())
    return d["choices"][0]["message"]["content"], d.get("usage", {})


def score(answer, item):
    """One label per answer, not three independent flags.

    A caveat that also repeats the number is a caveat: counting both would
    report every cell as 50 % of everything, which is what the first run did.
    """
    a = answer.lower()
    if CAVEAT.search(a):
        return "caveat"
    if item["value"] in a:
        return "asserted"
    return "other"


def run(args):
    data = json.load(open(args.evalset))
    items = data["items"]
    price_in, price_out = PRICES.get(args.model, (1.0, 3.0))
    est_in = len(items) * 2 * 400 / 1e6 * price_in
    est_out = len(items) * 2 * 60 / 1e6 * price_out
    print(f"{len(items)} memories x 2 arms = {len(items) * 2} calls, "
          f"estimated ${est_in + est_out:.3f}")
    if est_in + est_out > args.budget_usd:
        print(f"over --budget-usd {args.budget_usd}: not starting", file=sys.stderr)
        return 2
    key = os.environ.get(args.key_env)
    if not key:
        print(f"no {args.key_env} in the environment: nothing was spent", file=sys.stderr)
        return 2
    # No instruction to doubt, and no age in the prompt: both made the model
    # hedge in both arms, which measures the instruction and not the label.
    system = (
        "You are a coding agent with a local memory of notes and no access to the files. "
        "Answer the question from the memory you are given, in one short sentence."
    )
    if args.explain_label:
        # The protocol has to say what the line means; a suffix nobody has
        # been told about is decoration.
        system += (
            " A memory may carry a line such as `[anchor: the function changed]` or "
            "`[anchor: the file is gone]`. It means the note was checked against code that "
            "has changed since, so its value may no longer hold: do not state it as fact."
        )
    jobs = []
    for item in items:
        for arm in ("off", "on"):
            jobs.append((item, arm))
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {pool.submit(ask, args.base_url, key, args.model, system,
                               (f"Memory:\n{item['memory']}"
                                + (f"\n{item['label']}" if arm == "on" and item["label"] else "")
                                + "\n\nQuestion: " + item["question"])): (item, arm)
                   for item, arm in jobs}
        for fut in concurrent.futures.as_completed(futures):
            item, arm = futures[fut]
            try:
                answer, usage = fut.result()
            except Exception as e:  # noqa: BLE001 - reported, not swallowed
                print(f"failed {item['path']}:{item['function']}: {e}", file=sys.stderr)
                continue
            results.append({"item": item, "arm": arm, "answer": answer, "usage": usage,
                            "score": score(answer, item)})
    agg = collections.defaultdict(collections.Counter)
    for r in results:
        agg[(r["item"]["group"], r["arm"])][r["score"]] += 1
    print(f"\n{'group':<14}{'arm':<6}{'n':>4}{'asserted':>10}{'caveat':>9}{'other':>8}")
    for group in ("false_claim", "true_quiet", "true_noisy"):
        for arm in ("off", "on"):
            c = agg[(group, arm)]
            n = c["asserted"] + c["caveat"] + c["other"]
            if n:
                print(f"{group:<14}{arm:<6}{n:>4}{c['asserted']/n*100:>9.0f}%"
                      f"{c['caveat']/n*100:>8.0f}%{c['other']/n*100:>7.0f}%")
    if args.out:
        json.dump({"model": args.model, "results": results}, open(args.out, "w"), indent=1)
        print(f"\n-> {args.out}")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--repo", required=True)
    b.add_argument("--out", required=True)
    b.add_argument("--min-days", type=int, default=30)
    b.add_argument("--max-days", type=int, default=400)
    b.add_argument("--per-group", type=int, default=20)
    r = sub.add_parser("run")
    r.add_argument("--evalset", required=True)
    r.add_argument("--base-url", default="https://api.deepseek.com/v1")
    r.add_argument("--model", default="deepseek-flash")
    r.add_argument("--key-env", default="DEEPSEEK_API_KEY")
    r.add_argument("--budget-usd", type=float, default=2.0)
    r.add_argument("--workers", type=int, default=4)
    r.add_argument("--explain-label", action="store_true",
                   help="tell the model what the [anchor: ...] line means")
    r.add_argument("--out", default="")
    args = ap.parse_args()
    if args.cmd == "build":
        items = build(args.repo, args.min_days, args.max_days, args.per_group)
        json.dump({"repo": args.repo, "items": items}, open(args.out, "w"), indent=1)
        c = collections.Counter(i["group"] for i in items)
        print(f"{len(items)} memories -> {args.out}  {dict(c)}")
        return 0
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
