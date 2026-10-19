#!/usr/bin/env python3
"""Congela el corpus de supermemory a JSONL. Un solo uso."""
import json, pathlib, sys, urllib.request

BASE = "http://localhost:6767"
KEY = pathlib.Path.home().joinpath("Apps/.supermemory/api-key").read_text().strip()
OUT = pathlib.Path("data/supermemory-export.jsonl")
FIELDS = ("id", "content", "title", "createdAt", "metadata", "status", "containerTags")


def post(path, payload):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(payload).encode(),
        headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode(), strict=False)


def get(path):
    req = urllib.request.Request(
        BASE + path, headers={"Authorization": f"Bearer {KEY}"}
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode(), strict=False)


def main():
    listing = post("/v3/documents/list", {"limit": 1000})
    ids = [m["id"] for m in listing["memories"]]
    total = listing["pagination"]["totalItems"]
    if len(ids) != total:
        sys.exit(f"paginación incompleta: {len(ids)} de {total}")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", encoding="utf-8") as fh:
        for n, doc_id in enumerate(ids, 1):
            doc = get(f"/v3/documents/{doc_id}")
            fh.write(json.dumps({k: doc.get(k) for k in FIELDS}, ensure_ascii=False) + "\n")
            if n % 50 == 0:
                print(f"  {n}/{total}", file=sys.stderr)
    print(f"escritos {len(ids)} documentos en {OUT}")


if __name__ == "__main__":
    main()
