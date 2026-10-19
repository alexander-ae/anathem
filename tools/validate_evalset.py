#!/usr/bin/env python3
"""Valida eval/evalset.json contra el corpus congelado."""
import json, pathlib, sys, collections

ES = json.loads(pathlib.Path("eval/evalset.json").read_text(encoding="utf-8"))

# El corpus completo (390 documentos) y, aparte, el subconjunto CURADO.
# anathem solo va a indexar los curados, asi que el ground truth no puede
# apuntar a un session_turn ni a un batch de conversation: seria un caso
# que anathem nunca podria acertar, y estariamos midiendo una decision de
# diseno en vez de una falla de recuperacion.
CURATED_TYPES = {"project-knowledge", "project-config", "manual", "learned-pattern"}
corpus, curated, doc_type = set(), set(), {}
for line in pathlib.Path(ES["corpus"]).open(encoding="utf-8"):
    row = json.loads(line)
    did = row["id"]
    kind = (row.get("metadata") or {}).get("type")
    corpus.add(did)
    doc_type[did] = kind
    if kind in CURATED_TYPES:
        curated.add(did)

errors, cases = [], ES["cases"]
seen_ids, seen_queries = set(), set()
KINDS = {"factual", "procedural", "conventional", "temporal"}
TOPICS = {"perf", "code-review", "testing", "migraciones", "ux-design",
          "tooling-memoria", "infra", "cruzado"}

if not ES.get("ground_truth_scope"):
    errors.append("falta el campo ground_truth_scope de nivel superior")
if not ES.get("normalizacion"):
    errors.append("falta el campo normalizacion de nivel superior")

for c in cases:
    cid = c.get("id", "<sin id>")
    if cid in seen_ids:
        errors.append(f"{cid}: id duplicado")
    seen_ids.add(cid)
    q = (c.get("query") or "").strip().lower()
    if not q:
        errors.append(f"{cid}: query vacia")
    if q in seen_queries:
        errors.append(f"{cid}: query duplicada")
    seen_queries.add(q)
    # 'palabras' existe para que Task 3 y Task 9 puedan estratificar las metricas
    # por largo de consulta ademas de por 'parafrastica': si las parafrasticas
    # fueran sistematicamente mas largas, un efecto de largo se leeria como
    # efecto semantico y torceria D7. Tiene que quedar auditable, no solo mitigado.
    palabras = len((c.get("query") or "").split())
    if c.get("palabras") != palabras:
        errors.append(f"{cid}: palabras={c.get('palabras')!r} pero la query tiene {palabras}")
    exp = c.get("expect_docs") or []
    if not exp:
        errors.append(f"{cid}: expect_docs vacio")
    if len(exp) > 8:
        errors.append(f"{cid}: {len(exp)} expect_docs, maximo 8")
    if len(set(exp)) != len(exp):
        errors.append(f"{cid}: expect_docs con repetidos")
    for d in exp:
        if d not in corpus:
            errors.append(f"{cid}: doc {d} no esta en el corpus")
        elif d not in curated:
            errors.append(
                f"{cid}: doc {d} es de tipo {doc_type[d]!r}, fuera del ground_truth_scope "
                f"(anathem solo indexa los curados)")
    if c.get("kind") not in KINDS:
        errors.append(f"{cid}: kind invalido {c.get('kind')!r}")
    if c.get("topic") not in TOPICS:
        errors.append(f"{cid}: topic invalido {c.get('topic')!r}")
    for flag in ("parafrastica", "usa_identificador"):
        if not isinstance(c.get(flag), bool):
            errors.append(f"{cid}: {flag} tiene que ser booleano")

if len(cases) < 45:
    errors.append(f"solo {len(cases)} casos, se esperaban ~50")

parafrasticas = sum(1 for c in cases if c.get("parafrastica") is True)
identificadores = sum(1 for c in cases if c.get("usa_identificador") is True)
if parafrasticas < 15:
    errors.append(f"solo {parafrasticas} casos parafrasticos, se exigen 15")
if identificadores < 5:
    errors.append(f"solo {identificadores} casos con identificador exacto, se exigen 5")

def mediana(xs):
    xs = sorted(xs)
    m = len(xs) // 2
    return xs[m] if len(xs) % 2 else (xs[m - 1] + xs[m]) / 2

# .get con fallback: un caso al que le falte 'palabras' ya sumo su error arriba,
# y el resumen no tiene que reventar antes de imprimirlo.
def pal(c):
    return c.get("palabras") if isinstance(c.get("palabras"), int) else len((c.get("query") or "").split())

pal_par = [pal(c) for c in cases if c.get("parafrastica")]
pal_res = [pal(c) for c in cases if not c.get("parafrastica")]

longitudes = sorted(len(c.get("expect_docs") or []) for c in cases)
print(f"casos: {len(cases)}")
print("por kind:", dict(collections.Counter(c.get("kind") for c in cases)))
print("por topic:", dict(collections.Counter(c.get("topic") for c in cases)))
print(f"parafrasticas: {parafrasticas}")
print(f"con identificador exacto: {identificadores}")
if pal_par and pal_res:
    print(f"mediana de palabras: parafrasticas {mediana(pal_par)} / resto {mediana(pal_res)}"
          f" (rango {min(pal_par + pal_res)}-{max(pal_par + pal_res)})")
print("mediana expect_docs:", longitudes[len(cases) // 2])
print("max expect_docs:", longitudes[-1])
print(f"documentos distintos referenciados: {len({d for c in cases for d in c['expect_docs']})}"
      f" de {len(curated)} curados ({len(corpus)} en el corpus)")
if errors:
    print("\nERRORES:", file=sys.stderr)
    for e in errors:
        print("  " + e, file=sys.stderr)
    sys.exit(1)
print("\nOK")
