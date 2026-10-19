//! Ronda de arreglos 1 de Task 5: un paquete corrupto no puede tumbar la
//! búsqueda. Este test invoca el binario real, así como `primer_uso_test.rs`.

use std::process::Command;

fn mem(db: &std::path::Path, args: &[&str]) -> (String, String, bool) {
    let salida = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&salida.stdout).to_string(),
        String::from_utf8_lossy(&salida.stderr).to_string(),
        salida.status.success(),
    )
}

/// Simula una instalación corrupta o a medio escribir del paquete: la fila
/// de `meta` dice que hay un paquete `mini-es`, pero el archivo que debería
/// contenerlo es basura. Buscar no puede fallar por eso: tiene que avisar
/// por stderr y seguir devolviendo resultados de bm25.
#[test]
fn paquete_corrupto_degrada_a_bm25_sin_tumbar_la_busqueda() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    mem(&db, &["init"]);
    mem(&db, &["save", "El cache del breakdown no se justifica por staleness"]);

    {
        let conn = anathem::db::open(&db).unwrap();
        anathem::vecstore::registrar_pack(&conn, "mini-es", "deadbeef", 4).unwrap();
    }
    let paquetes = dir.path().join("paquetes");
    std::fs::create_dir_all(&paquetes).unwrap();
    std::fs::write(paquetes.join("mini-es.anathem-pack"), b"esto no es un paquete valido").unwrap();

    let (out, err, ok) = mem(&db, &["search", "cache staleness"]);
    assert!(ok, "un paquete corrupto no puede tumbar la búsqueda: stderr={err}");
    assert!(out.contains("cache"), "la búsqueda debería seguir devolviendo resultados de bm25: {out}");
    assert!(
        err.contains("mini-es"),
        "el aviso debe nombrar el paquete que no se pudo abrir: {err}"
    );
    assert!(
        err.to_lowercase().contains("bm25"),
        "el aviso debe decir que la búsqueda quedó degradada a bm25: {err}"
    );
}
