#!/usr/bin/env python3
"""Builds a vocabulary pack for anathem's vector layer.

A pack is the static model trimmed to the languages you care about: the
tokenizer without the pieces you will not use, and the embedding table
quantized to int8. See docs/vocabulary-packs.md for what that buys and what
it costs.

    pip install model2vec numpy
    python3 tools/build_pack.py --name en-es \
        --sample texts_en.txt texts_es.txt --out en-es.anathem-pack

The first run downloads the base model — `minishlab/potion-multilingual-128M`
by default, about 512 MB — from Hugging Face. The pack it writes is a small
fraction of that: `en-es` comes out at 22 MB.

It writes the pack and its `.sha256` beside it. Keep them together: `mem
model install` refuses a pack it cannot check.
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
        sys.exit(f"only Unigram is supported; this model uses {tok_json['model']['type']}")
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
    # The Spanish spellings stay accepted alongside the English ones, the
    # same way the CLI kept its own: a note or a script that already has
    # them written down keeps working.
    p.add_argument("--model", "--modelo", dest="model",
                   default="minishlab/potion-multilingual-128M",
                   help="base model to trim; it is downloaded on first use (~512 MB)")
    p.add_argument("--name", "--nombre", dest="name", required=True,
                   help="the pack's identifier, e.g. en-es. It decides the installed file's name")
    p.add_argument("--sample", "--muestra", dest="sample", required=True, nargs="+",
                   help="text files, one passage per line, in the pack's languages")
    p.add_argument("--out", "--salida", dest="out", required=True,
                   help="where to write the .anathem-pack; its .sha256 goes beside it")
    a = p.parse_args()
    construir(a.model, a.name, a.sample, a.out)
