#!/usr/bin/env python3
"""Baseline de supermemory sobre eval/evalset.json."""
import json, pathlib, statistics, time, urllib.request, datetime

BASE = "http://localhost:6767"
KEY = pathlib.Path.home().joinpath("Apps/.supermemory/api-key").read_text().strip()
ES = json.loads(pathlib.Path("eval/evalset.json").read_text(encoding="utf-8"))


def search(q, limit=50):
    req = urllib.request.Request(
        BASE + "/v3/search",
        data=json.dumps({
            "q": q, "containerTags": ["my-project-tag"], "limit": limit,
            "threshold": 0, "documentThreshold": 0,
        }).encode(),
        headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"},
    )
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=60) as r:
        body = json.loads(r.read().decode(), strict=False)
    ms = (time.perf_counter() - t0) * 1000
    out = []
    for x in body.get("results", []):
        d = x.get("documentId")
        if d and d not in out:
            out.append(d)
    return out, ms


per_case, rr, lat = [], [], []
h5 = h50 = 0
for c in ES["cases"]:
    got, ms = search(c["query"])
    lat.append(ms)
    exp = set(c["expect_docs"])
    hit5 = [i for i, d in enumerate(got[:5]) if d in exp]
    hit50 = [i for i, d in enumerate(got[:50]) if d in exp]
    h5 += 1 if hit5 else 0
    h50 += 1 if hit50 else 0
    rr.append(1 / (hit50[0] + 1) if hit50 else 0.0)
    per_case.append({
        "id": c["id"], "kind": c.get("kind"),
        "hit_at_5": bool(hit5),
        "first_rank": (hit50[0] + 1) if hit50 else None,
        "returned": got[:10],
        "parafrastica": c.get("parafrastica"),
        "palabras": c.get("palabras"),
    })

n = len(ES["cases"])
res = {
    "system": "supermemory /v3/search",
    "date": datetime.date.today().isoformat(),
    "n_cases": n,
    "recall_at_5": h5 / n,
    "recall_at_50": h50 / n,
    "mrr": statistics.fmean(rr),
    "p50_ms": statistics.median(lat),
    "per_case": per_case,
}
out = pathlib.Path(f"eval/results/supermemory-{res['date']}.json")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(res, indent=2, ensure_ascii=False), encoding="utf-8")
print(f"recall@5={res['recall_at_5']:.0%}  recall@50={res['recall_at_50']:.0%}  "
      f"MRR={res['mrr']:.3f}  p50={res['p50_ms']:.0f}ms  -> {out}")
