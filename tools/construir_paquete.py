#!/usr/bin/env python3
"""Construye un paquete de vocabulario para la capa vectorial de anathem.

Un paquete es el modelo estático recortado a los idiomas que te interesan:
el tokenizador sin las piezas que no vas a usar y la tabla de embeddings
cuantizada a int8. Ver docs/paquetes-de-vocabulario.md.

    python3 tools/construir_paquete.py --nombre en-es \
        --muestra textos_en.txt textos_es.txt --salida en-es.anathem-pack
"""
import argparse, hashlib, json, struct, sys
from pathlib import Path

MAGIC = b"ANPK"
VERSION = 1


def leer_muestra(rutas):
    for r in rutas:
        with open(r, encoding="utf-8") as f:
            for linea in f:
                linea = linea.strip()
                if linea:
                    yield linea


def construir(modelo, nombre, muestras, salida, lote=2000):
    from model2vec import StaticModel
    import numpy as np

    m = StaticModel.from_pretrained(modelo)
    tabla = m.embedding
    n_total, dims = tabla.shape
    tok_json = json.loads(m.tokenizer.to_str())
    if tok_json["model"]["type"] != "Unigram":
        sys.exit(f"solo se soporta Unigram; este modelo usa {tok_json['model']['type']}")
    vocab = tok_json["model"]["vocab"]

    # Siempre se conservan: los especiales, los de byte fallback (garantizan
    # que cualquier carácter sea representable) y el propio UNK.
    conservar = {tok_json["model"].get("unk_id", 1)}
    for i, (pieza, _) in enumerate(vocab):
        if pieza.startswith("<0x") and pieza.endswith(">"):
            conservar.add(i)
        elif pieza.startswith("[") and pieza.endswith("]"):
            conservar.add(i)
    fijos = len(conservar)

    textos, vistos = [], 0
    for linea in leer_muestra(muestras):
        textos.append(linea)
        if len(textos) >= lote:
            for enc in m.tokenizer.encode_batch(textos):
                conservar.update(i for i in enc.ids if i < n_total)
            vistos += len(textos)
            textos = []
    if textos:
        for enc in m.tokenizer.encode_batch(textos):
            conservar.update(i for i in enc.ids if i < n_total)
        vistos += len(textos)
    print(f"muestra: {vistos:,} líneas", file=sys.stderr)
    print(f"vocabulario: {len(conservar):,} de {n_total:,} "
          f"({100*len(conservar)/n_total:.0f}%, {fijos} fijos)", file=sys.stderr)

    idx = sorted(conservar)
    # Unigram re-segmenta solo con las piezas que queden, así que basta con
    # filtrar el vocabulario y reapuntar unk_id a su posición nueva.
    nuevo = dict(tok_json)
    nuevo["model"] = dict(tok_json["model"])
    nuevo["model"]["vocab"] = [vocab[i] for i in idx]
    donde = {viejo: i for i, viejo in enumerate(idx)}
    nuevo["model"]["unk_id"] = donde[tok_json["model"].get("unk_id", 1)]

    sub = tabla[idx].astype(np.float32)
    lo = sub.min(0)
    esc = (sub.max(0) - lo) / 255.0
    esc[esc == 0] = 1.0
    q = np.clip(np.rint((sub - lo) / esc), 0, 255).astype(np.uint8)

    tb = json.dumps(nuevo, ensure_ascii=False).encode("utf-8")
    nb = nombre.encode("utf-8")
    cuerpo = b"".join([
        struct.pack("<4sIIII", MAGIC, VERSION, dims, len(idx), len(nb)),
        nb,
        struct.pack("<Q", len(tb)), tb,
        esc.astype("<f4").tobytes(), lo.astype("<f4").tobytes(),
        q.tobytes(),
    ])
    Path(salida).write_bytes(cuerpo)
    h = hashlib.sha256(cuerpo).hexdigest()
    Path(str(salida) + ".sha256").write_text(f"{h}  {Path(salida).name}\n")
    print(f"{salida}: {len(cuerpo)/1e6:.1f} MB  sha256 {h[:16]}…", file=sys.stderr)
    return h


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--modelo", default="minishlab/potion-multilingual-128M")
    p.add_argument("--nombre", required=True, help="identificador del paquete, p.ej. en-es")
    p.add_argument("--muestra", required=True, nargs="+",
                   help="archivos de texto, una línea por pasaje, en los idiomas del paquete")
    p.add_argument("--salida", required=True)
    a = p.parse_args()
    construir(a.modelo, a.nombre, a.muestra, a.salida)
