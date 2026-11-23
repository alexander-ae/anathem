#!/usr/bin/env python3
"""How long an anchor survives, measured over real git histories.

An anchor is a promise that something was true against a file. This measures
how often that promise is already false at HEAD, and at two granularities:
the whole file, and the body of one function inside it. The second exists
because the first is what `mem save --anchor` used to record, and the table
this prints is why it no longer does:

    $ python3 tools/anchor_decay.py ~/proyectos/anathem
    file level      88.5 % of (commit, file) pairs stale at HEAD
    symbol level    100 % of function bodies unchanged within 7 days

Method, and what it is not:

  * Every commit that touched a file contributes one pair per file. A pair is
    stale when the file was touched again after it, or is gone at HEAD.
  * The symbol half is a sample: up to four functions defined in the version
    of the file at that commit, with tests/ skipped. It says nothing about
    functions the commit did not touch.
  * Repositories here are young and fast-moving, so these are not rates for
    "a codebase" in general. They are enough to decide the granularity of a
    hash, which is what the question was.

Usage:
    python3 tools/anchor_decay.py <repo> [<repo> ...] [--json]
"""
import collections
import hashlib
import json
import re
import statistics
import subprocess
import sys

BUCKETS = [(0, 7, "0-7 d"), (7, 30, "7-30 d"), (30, 90, "30-90 d"), (90, 365, "90-365 d"), (365, 10**6, ">1 y")]


def git(repo, *args, text=True):
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=text)


def parse_log(repo):
    """[(timestamp, commit, [path, ...]), ...], newest first."""
    out = git(repo, "log", "--format=@%ct %H", "--name-only").stdout
    commits, cur = [], None
    for line in out.splitlines():
        if line.startswith("@"):
            ts, sha = line[1:].split()
            cur = (int(ts), sha, [])
            commits.append(cur)
        elif line.strip() and cur is not None:
            cur[2].append(line.strip())
    return commits


def file_level(repo, commits, head):
    exists = set(git(repo, "ls-files").stdout.split())
    last = {}
    for ts, _sha, files in commits:
        for f in files:
            last[f] = max(last.get(f, 0), ts)
    rows = []
    for ts, _sha, files in commits:
        for f in files:
            stale = last[f] > ts or f not in exists
            rows.append(((head - ts) / 86400, stale))
    return rows


def show(repo, rev, path):
    r = git(repo, "show", f"{rev}:{path}")
    return r.stdout if r.returncode == 0 else None


def names(src):
    return sorted(set(re.findall(
        r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)", src, re.M)))


def block(src, name):
    m = re.search(rf"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+{name}\b", src, re.M)
    if not m:
        return None
    i = src.find("{", m.end())
    if i < 0:
        return None
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                return hashlib.sha256(src[i:j + 1].encode()).hexdigest()
    return None


def symbol_level(repo, commits, head, limit_files=None):
    rows = []
    cache = {}
    for ts, sha, files in commits:
        for f in files:
            if not f.endswith(".rs") or f.startswith("tests/"):
                continue
            src = show(repo, sha, f)
            if not src:
                continue
            if f not in cache:
                cache[f] = show(repo, "HEAD", f)
            head_src = cache[f]
            for name in names(src)[:4]:
                old = block(src, name)
                if head_src is None:
                    state = "gone"
                else:
                    new = block(head_src, name)
                    state = "gone" if new is None else ("same" if new == old else "changed")
                rows.append(((head - ts) / 86400, state))
    return rows


def by_bucket(rows, key):
    out = []
    for lo, hi, label in BUCKETS:
        sel = [r for r in rows if lo <= r[0] < hi]
        if sel:
            out.append((label, len(sel), sum(1 for r in sel if key(r)) / len(sel) * 100))
    return out


def main(argv):
    repos = [a for a in argv[1:] if not a.startswith("-")]
    as_json = "--json" in argv
    if not repos:
        print(__doc__.split("Usage:")[1].strip(), file=sys.stderr)
        return 2
    report = {}
    for repo in repos:
        commits = parse_log(repo)
        if not commits:
            continue
        head = max(c[0] for c in commits)
        files = file_level(repo, commits, head)
        syms = symbol_level(repo, commits, head)
        report[repo] = {
            "commits": len(commits),
            "span_days": (head - min(c[0] for c in commits)) / 86400,
            "file_pairs": len(files),
            "file_stale": sum(1 for r in files if r[1]) / len(files) * 100,
            "file_by_age": [{"bucket": b, "n": n, "stale": p} for b, n, p in by_bucket(files, lambda r: r[1])],
            "symbol_pairs": len(syms),
            "symbol_by_age": [
                {"bucket": b, "n": n,
                 "same": sum(1 for r in [x for x in syms if x[0] >= lo and x[0] < hi] if r[1] == "same") / n * 100}
                for lo, hi, b in BUCKETS
                for n in [len([x for x in syms if lo <= x[0] < hi])] if n
            ],
        }
    if as_json:
        print(json.dumps(report, indent=2))
        return 0
    for repo, r in report.items():
        print(f"\n{repo}  ({r['commits']} commits, {r['span_days']:.0f} days)")
        print(f"  file level:   {r['file_pairs']} pairs, {r['file_stale']:.1f} % stale at HEAD")
        for row in r["file_by_age"]:
            print(f"    {row['bucket']:>8}  n={row['n']:>5}  stale {row['stale']:5.1f} %")
        if r["symbol_pairs"]:
            print(f"  symbol level: {r['symbol_pairs']} (commit, function) pairs")
            for row in r["symbol_by_age"]:
                print(f"    {row['bucket']:>8}  n={row['n']:>5}  body unchanged {row['same']:5.1f} %")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
