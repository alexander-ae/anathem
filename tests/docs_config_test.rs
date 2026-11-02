//! What the documentation shows as a `config.json` has to be a config.json
//! the binary accepts, written with the canonical key names.
//!
//! This exists because the README documented `{ "candidatos": 20, "extracto":
//! 120 }` — two keys that never existed. Anyone who copied that block got a
//! tool that exits 1 on every command, with `deny_unknown_fields` rejecting
//! the file. It is the same failure the validation messages had: naming a key
//! nobody can use.
//!
//! Scanning the docs rather than pinning one example on purpose: the point is
//! that the docs cannot drift from the struct again.

use anathem::config;

/// The keys the struct accepts today.
const CANONICAL: &[&str] = &[
    "bm25_weight", "candidates", "excerpt",
    "handoff_items", "handoff_window", "release_pack_after",
];

/// Spellings that must not appear in the documentation. The first three are
/// deprecated aliases that still load; the last three never existed and only
/// ever produced an error.
const NOT_IN_DOCS: &[&str] = &[
    "traspaso_items", "traspaso_ventana", "soltar_paquete_tras",
    "candidatos", "extracto", "peso_bm25",
];

fn docs() -> Vec<std::path::PathBuf> {
    let mut v = vec![std::path::PathBuf::from("README.md")];
    for e in std::fs::read_dir("docs").unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "md") {
            v.push(p);
        }
    }
    v
}

/// Every ```json block that mentions a config key, with where it came from.
fn config_blocks() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in docs() {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, bloque) in text.split("```json").skip(1).enumerate() {
            let Some(cuerpo) = bloque.split("```").next() else { continue };
            let menciona = CANONICAL.iter().chain(NOT_IN_DOCS)
                .any(|k| cuerpo.contains(&format!("\"{k}\"")));
            if menciona {
                out.push((format!("{}, bloque json #{}", f.display(), n + 1), cuerpo.to_string()));
            }
        }
    }
    out
}

#[test]
fn every_documented_config_is_one_the_binary_accepts() {
    let bloques = config_blocks();
    assert!(!bloques.is_empty(), "no se encontró ningún ejemplo de config.json en los docs");
    for (donde, cuerpo) in bloques {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("config.json"), &cuerpo).unwrap();
        config::load(d.path()).unwrap_or_else(|e| {
            panic!("{donde} muestra un config.json que el binario rechaza: {e}\n{cuerpo}")
        });
    }
}

#[test]
fn the_docs_do_not_teach_deprecated_or_nonexistent_keys() {
    for (donde, cuerpo) in config_blocks() {
        for k in NOT_IN_DOCS {
            assert!(
                !cuerpo.contains(&format!("\"{k}\"")),
                "{donde} documenta `{k}`, que no es el nombre canónico:\n{cuerpo}"
            );
        }
    }
}
