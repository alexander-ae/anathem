#!/usr/bin/env python3
"""Answers a benchmark's questions with and without anathem's memory, and scores them.

`mem eval` measures retrieval: whether the evidence reached the model. This
measures the other half — whether a model then answers correctly — which is
the only axis on which the memory products' published numbers can be put
beside anything.

**anathem itself never calls a model**, and nothing here is part of it: this
is a benchmark harness that spends someone's API budget, and it lives in
`tools/` for the same reason `longmemeval_to_memories.py` does.

Four arms, one model answering all of them, so the difference is the memory
and not the model:

  full   the whole conversation in the prompt, no retrieval
  cards  exactly what `search_memory` returns, verbatim — 20 candidates with
         a 120-character excerpt — and nothing else
  cards+ the same, then one turn where the model names up to three memories
         to read in full through `get_memory`, then the answer. This is the
         two-stage protocol anathem is designed around
  none   the question alone, which is what a cold session knows

Scoring is done twice: by a judge model against the reference answer, and by
token F1 against it, which is what the LoCoMo paper itself reports. The
second costs nothing and says whether the judge and the overlap agree.

    export OPENAI_API_KEY=...
    python3 tools/answer_eval.py --db bench.db --evalset bench/evalset.json \\
        --model gpt-6-luna --judge gpt-6.1-sol --sample 200 --out bench/answers/ \\
        --budget-usd 3

The sample is **evenly spaced inside each question type**, not the first N:
the file is grouped, so a prefix would measure one stratum and call it the
benchmark. No seed to record; the same N gives anyone the same cases.
"""
import argparse
import json
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

#: Standard prices per million tokens, from OpenAI's price list, read
#: 2026-11-15. Only used to stop before the budget is gone; the bill is the
#: API's, not this table's.
PRICES = {
    "gpt-6-luna": (0.10, 0.50),
    "gpt-5.6-luna": (0.20, 1.20),
    "gpt-6.1-terra": (1.00, 5.00),
    "gpt-6.1-sol": (2.00, 10.00),
    "gpt-6-sol": (2.00, 10.00),
    "gpt-5": (1.25, 10.00),
    "gpt-5-mini": (0.25, 2.00),
    "gpt-4.1-mini": (0.40, 1.60),
}

SPENT = 0.0
ANSWER_CAP = None
SPENT_LOCK = threading.Lock()
BUDGET = None


class OutOfBudget(Exception):
    pass


def price(model, prompt_tokens, completion_tokens):
    pin, pout = PRICES.get(model, (10.0, 50.0))  # unknown model: assume the worst
    return prompt_tokens / 1e6 * pin + completion_tokens / 1e6 * pout


def charge(model, usage):
    global SPENT
    cost = price(model, usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0))
    with SPENT_LOCK:
        SPENT += cost
        if BUDGET is not None and SPENT > BUDGET:
            raise OutOfBudget(f"spent ${SPENT:.2f}, budget ${BUDGET:.2f}")


def complete(model, messages, max_tokens=200, retries=4):
    """One chat completion, retried once when the model thought instead of answering.

    No `temperature`: the 2026 models reject the parameter outright (400),
    and asking for determinism is pointless when the request is a question
    with one right answer.

    `max_completion_tokens` covers the reasoning tokens as well as the answer,
    and a reasoning model handed a cap that fits a small one spends it all
    thinking and returns an empty string. That is not a rare edge: it was 23
    of 48 answers on the first run with `gpt-5` at 400. So an empty answer
    with `finish_reason == "length"` is retried once with four times the room,
    and only then given up on.
    """
    body = {"model": model, "messages": messages, "max_completion_tokens": max_tokens}
    data = json.dumps(body).encode()
    for attempt in range(retries):
        req = urllib.request.Request(
            "https://api.openai.com/v1/chat/completions", data=data,
            headers={"Authorization": f"Bearer {os.environ['OPENAI_API_KEY']}",
                     "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                payload = json.load(r)
            charge(model, payload.get("usage") or {})
            choice = payload["choices"][0]
            text = choice["message"].get("content") or ""
            if not text.strip() and choice.get("finish_reason") == "length" and max_tokens < 4000:
                return complete(model, messages, max_tokens * 4, retries)
            return text
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 502, 503, 504) and attempt < retries - 1:
                time.sleep(2 ** attempt)
                continue
            raise
        except OutOfBudget:
            raise
    raise RuntimeError("out of retries")


# --------------------------------------------------------------------------
# MCP: the memory tools, exactly as an agent calls them.

class Mem:
    """One `mem serve` process, spoken to over stdio."""

    def __init__(self, db):
        self.proc = subprocess.Popen(
            ["mem", "--db", db, "serve"], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, text=True, bufsize=1)
        self.n = 0
        self.log = threading.Lock()
        self.call("initialize", {"protocolVersion": "2024-11-05",
                                 "clientInfo": {"name": "answer_eval"}}, notify=False)
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0",
                                          "method": "notifications/initialized"}) + "\n")
        self.proc.stdin.flush()

    def call(self, method, params, notify=True):
        with self.log:
            self.n += 1
            req = {"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}
            self.proc.stdin.write(json.dumps(req) + "\n")
            self.proc.stdin.flush()
            if not notify:
                return None
            while True:
                line = self.proc.stdout.readline()
                if not line:
                    raise RuntimeError("mem serve closed its stdout")
                payload = json.loads(line)
                if payload.get("id") == self.n:
                    return payload

    def search(self, query, project, limit=0):
        args = {"query": query, "project": project}
        # No `limit` at all lets the database's `config.json` decide, which is
        # what a real installation does; passing one overrides `candidates`.
        if limit:
            args["limit"] = limit
        r = self.call("tools/call", {"name": "search_memory", "arguments": args})
        return r["result"]["content"][0]["text"]

    def body(self, memory_id):
        r = self.call("tools/call", {"name": "get_memory",
                                     "arguments": {"id": memory_id}})
        return r["result"]["content"][0]["text"]

    def bodies(self, ids):
        r = self.call("tools/call", {"name": "get_memory",
                                     "arguments": {"ids": list(ids)}})
        return r["result"]["content"][0]["text"]

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()


# --------------------------------------------------------------------------

def conversations(db):
    """project -> its memories' bodies in insertion order, oldest first.

    The `full` arm needs the whole history in one prompt, and it is the same
    text that was imported, so the database is the source of truth here as
    well: whatever is in it is what the retriever could have found.
    """
    conn = sqlite3.connect(db)
    out = {}
    for project, body in conn.execute(
            "select project, body from memories where status = 'active' order by id"):
        out.setdefault(project, []).append(body)
    return {k: "\n\n".join(v) for k, v in out.items()}


def stratified(cases, n):
    """Every k-th case inside each kind, so every stratum keeps its share.

    The per-kind counts are apportioned exactly — largest remainder, no
    randomness — so the sample holds exactly `n` cases and no stratum is
    dropped by a rounding error at the end of the list.
    """
    by_kind = {}
    for c in cases:
        by_kind.setdefault(c.get("kind") or "no kind", []).append(c)
    total = len(cases)
    kinds = sorted(by_kind)
    if n >= total:
        return [c for k in kinds for c in by_kind[k]]
    base, rest = {}, []
    for k in kinds:
        exact = n * len(by_kind[k])
        base[k] = exact // total
        rest.append((-exact % total, k))
    for _, k in sorted(rest)[: n - sum(base.values())]:
        base[k] += 1

    picked = []
    for k in kinds:
        group, want = by_kind[k], base[k]
        if want == 0:
            continue
        step = len(group) / want
        picked += [group[int(i * step)] for i in range(want)]
    return picked


ANSWER_SYSTEM = (
    "You answer questions about past conversations between two people. "
    "Use only the material given. If it does not contain the answer, reply "
    "exactly: not in memory. Answer in as few words as possible: a date, a "
    "name, a phrase. Do not explain."
)

PICK_SYSTEM = (
    "You are reading a list of numbered memory candidates. Reply with a JSON "
    "array of at most 3 ids whose full text you need in order to answer the "
    "question. Reply [] if the candidates already answer it."
)

JUDGE_SYSTEM = (
    "You grade an answer against a reference answer. Reply with one word, "
    "yes or no. 'yes' if the answer says the same thing as the reference, "
    "allowing different wording, a longer or shorter form, or extra detail "
    "that does not contradict it. 'no' otherwise, including when the answer "
    "is evasive, says it does not know, or names a different entity or date."
)


def tokens(s):
    # LoCoMo writes some reference answers as numbers ("answer": 3), so this
    # takes anything and stringifies it: an `int` here used to raise inside
    # the F1, which silently dropped the case from the arm's denominator.
    return re.findall(r"[a-z0-9]+", str(s).lower())


def f1(pred, ref):
    """Token-level F1, the LoCoMo paper's own QA metric, minus the stemming."""
    p, r = tokens(pred), tokens(ref)
    if not p or not r:
        return 0.0
    common = {}
    for t in p:
        common[t] = common.get(t, 0) + 1
    hits = 0
    for t in r:
        if common.get(t, 0) > 0:
            common[t] -= 1
            hits += 1
    if not hits:
        return 0.0
    precision, recall = hits / len(p), hits / len(r)
    return 2 * precision * recall / (precision + recall)


def answer(model, arm, question, material, mem, project, batch_get=False):
    """One case in one arm. Returns (answer, ids_read)."""
    read = []
    if arm == "none":
        user = f"Question: {question}\n\nMaterial: (none)"
        return complete(model, [{"role": "system", "content": ANSWER_SYSTEM},
                                {"role": "user", "content": user}], 400), read
    if arm == "full":
        user = f"Material:\n{material}\n\nQuestion: {question}"
        return complete(model, [{"role": "system", "content": ANSWER_SYSTEM},
                                {"role": "user", "content": user}], 400), read
    cards = material  # the verbatim `search_memory` text
    if arm == "cards":
        user = f"Material:\n{cards}\n\nQuestion: {question}"
        return complete(model, [{"role": "system", "content": ANSWER_SYSTEM},
                                {"role": "user", "content": user}], 400), read
    # cards+
    pick = complete(model, [{"role": "system", "content": PICK_SYSTEM},
                            {"role": "user", "content": f"{cards}\n\nQuestion: {question}"}],
                    300)
    ids = [int(x) for x in re.findall(r"\d+", pick)][:3]
    bodies = []
    if ids and batch_get:
        # One call for the lot: the tool now takes `ids`, and three round
        # trips are what makes an agent skip the step that matters.
        try:
            bodies.append(mem.bodies(ids))
            read.extend(ids)
        except Exception:
            pass
    for mid in ([] if read else ids):
        try:
            bodies.append(mem.body(mid))
            read.append(mid)
        except Exception:
            pass
    material = cards + ("\n\nFull text of the memories you asked for:\n" + "\n\n".join(bodies)
                        if bodies else "")
    user = f"Material:\n{material}\n\nQuestion: {question}"
    return complete(model, [{"role": "system", "content": ANSWER_SYSTEM},
                            {"role": "user", "content": user}], 400), read


def judge(model, question, ref, said):
    user = (f"Question: {question}\nReference answer: {ref}\n"
            f"Answer to grade: {said}")
    out = complete(model, [{"role": "system", "content": JUDGE_SYSTEM},
                           {"role": "user", "content": user}], 64)
    return out.strip().lower().startswith("yes")


def main(argv):
    global BUDGET
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", required=True)
    ap.add_argument("--evalset", required=True)
    ap.add_argument("--locomo", required=True,
                    help="the dataset file, for the reference answers the eval set omits")
    ap.add_argument("--model", default="gpt-6-luna")
    ap.add_argument("--judge", default="gpt-6.1-sol")
    ap.add_argument("--sample", type=int, default=200)
    ap.add_argument("--arms", default="full,cards,cards+,none")
    ap.add_argument("--out", default="bench/answers")
    ap.add_argument("--budget-usd", type=float, default=3.0)
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--limit", type=int, default=0,
                    help="candidates per search; 0 leaves it to config.json")
    ap.add_argument("--batch-get", action="store_true",
                    help="read the picked bodies with one `ids` call instead of one per id")
    a = ap.parse_args(argv[1:])
    # Answering and judging share one budget, and the first version let
    # answering spend all of it: the answers were paid for and then every
    # judge call hit the cap, which looks exactly like a model that got every
    # question wrong. A quarter is held back for scoring.
    global ANSWER_CAP

    if not os.environ.get("OPENAI_API_KEY"):
        print("OPENAI_API_KEY is not set: this script cannot run without one.", file=sys.stderr)
        return 2
    BUDGET = a.budget_usd
    ANSWER_CAP = a.budget_usd * 0.75

    cases = json.load(open(a.evalset, encoding="utf-8"))["cases"]
    picked = stratified(cases, a.sample)
    refs = references(a.locomo)
    convs = conversations(a.db)
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    arms = [x for x in a.arms.split(",") if x]
    BUDGET = ANSWER_CAP
    mem = Mem(a.db)
    results, lock = [], threading.Lock()
    queue = list(picked)
    done = [0]

    def work():
        while True:
            with lock:
                if not queue:
                    return
                c = queue.pop(0)
            ref = refs.get(c["id"])
            if ref is None or c["project"] not in convs:
                continue
            row = {"id": c["id"], "kind": c.get("kind"), "query": c["query"], "ref": ref}
            for arm in arms:
                try:
                    material = ""
                    if arm == "full":
                        material = convs[c["project"]]
                    elif arm in ("cards", "cards+"):
                        material = mem.search(c["query"], c["project"], a.limit)
                    said, read = answer(a.model, arm, c["query"], material, mem,
                                        c["project"], a.batch_get)
                    row[arm] = {"answer": said.strip(), "read": read, "f1": f1(said, ref)}
                except OutOfBudget:
                    # Keep the answers already paid for and stop spending:
                    # the run is written out with the arms it reached.
                    row[arm] = {"answer": None, "read": [], "f1": 0.0}
                    with lock:
                        queue.clear()
                except Exception as e:
                    row[arm] = {"answer": None, "read": [], "f1": 0.0, "error": str(e)}
            with lock:
                results.append(row)
                done[0] += 1
                if done[0] % 25 == 0:
                    print(f"  {done[0]}/{len(picked)} cases, ${SPENT:.3f}", flush=True)

    threads = [threading.Thread(target=work) for _ in range(a.workers)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    mem.close()

    # The paid-for answers are written before a single judge call, so a crash,
    # a Ctrl-C or a budget stop during scoring cannot lose what was bought.
    (out / "answers-raw.json").write_text(
        json.dumps({"model": a.model, "n": len(results), "arms": arms, "rows": results},
                   ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    BUDGET = a.budget_usd
    print(f"answering done: {len(results)} cases, ${SPENT:.3f}; judging", flush=True)
    todo = [(row, arm) for row in results for arm in arms]
    lock2 = threading.Lock()

    def scorer():
        while True:
            with lock2:
                if not todo:
                    return
                row, arm = todo.pop(0)
            if row[arm].get("answer") is None:
                row[arm]["correct"] = None
                continue
            try:
                row[arm]["correct"] = judge(a.judge, row["query"], row["ref"], row[arm]["answer"])
            except OutOfBudget:
                row[arm]["correct"] = None
            except Exception as e:
                row[arm]["correct"] = None
                row[arm]["judge_error"] = str(e)

    # Judging is one cheap call per answer and there are four per case; done
    # one at a time it costs more wall-clock than the answers did.
    scorers = [threading.Thread(target=scorer) for _ in range(a.workers)]
    for t in scorers:
        t.start()
    for t in scorers:
        t.join()

    (out / "answers.json").write_text(
        json.dumps({"model": a.model, "judge": a.judge, "n": len(results),
                    "spent_usd": round(SPENT, 4), "arms": arms, "rows": results},
                   ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    kinds = sorted({r["kind"] for r in results})
    print(f"\n{'arm':<8}{'n':>5}{'judged':>9}{'F1':>8}" + "".join(f"{k[:9]:>11}" for k in kinds))
    for arm in arms:
        ok = [r[arm]["correct"] for r in results if r[arm]["correct"] is not None]
        line = f"{arm:<8}{len(results):>5}{(sum(ok) / len(ok) * 100 if ok else 0):>8.1f}%"
        line += f"{sum(r[arm]['f1'] for r in results) / len(results) * 100:>7.1f}"
        for k in kinds:
            sub = [r[arm]["correct"] for r in results
                   if r["kind"] == k and r[arm]["correct"] is not None]
            line += f"{(sum(sub) / len(sub) * 100 if sub else 0):>10.1f}%"
        print(line)
    print(f"\nmodel {a.model}, judge {a.judge}, spent ${SPENT:.3f}, "
          f"written to {out/'answers.json'}")
    return 0


def references(locomo):
    """case id -> reference answer, read from the dataset the eval set came from."""
    out = {}
    for sample in json.load(open(locomo, encoding="utf-8")):
        for i, qa in enumerate(sample["qa"]):
            if qa.get("answer") not in (None, ""):
                out[f"{sample['sample_id']}-{i}"] = str(qa["answer"])
    return out


if __name__ == "__main__":
    sys.exit(main(sys.argv))
