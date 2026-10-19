use anathem::{db, ingest, maint, model::NewMemory, search};

fn seed(conn: &rusqlite::Connection, ext: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(), title: "cache rechazado".into(),
        body: "El cache del breakdown no se justifica.".into(),
        created: "2026-09-21".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some(ext.into()),
        tags: vec![], refs: vec![],
    }).unwrap()
}

#[test]
fn forget_saca_de_la_busqueda_sin_borrar() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "a");
    maint::forget(&conn, id).unwrap();

    let q = search::Query { text: "cache".into(), ..Default::default() };
    assert!(search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().is_empty(), "forget debe sacarla de la búsqueda");

    let n: i64 = conn.query_row("select count(*) from memories where id=?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "forget no debe borrar la fila");
}

#[test]
fn pin_marca_la_memoria() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "a");
    maint::pin(&conn, id, true).unwrap();
    let p: i64 = conn.query_row("select pinned from memories where id=?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(p, 1);
}

#[test]
fn forget_en_id_inexistente_falla() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let err = maint::forget(&conn, 999_999).expect_err("forget sobre un id inexistente debe fallar");
    assert!(err.to_string().contains("999999"), "mensaje inesperado: {err}");
}

#[test]
fn pin_en_id_inexistente_falla() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let err = maint::pin(&conn, 999_999, true).expect_err("pin sobre un id inexistente debe fallar");
    assert!(err.to_string().contains("999999"), "mensaje inesperado: {err}");
}

#[test]
fn history_devuelve_la_cadena_de_supersession() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let vieja = seed(&conn, "vieja");
    let nueva = seed(&conn, "nueva");
    conn.execute("update memories set supersedes_id=?1 where id=?2", [vieja, nueva]).unwrap();
    conn.execute("update memories set status='superseded' where id=?1", [vieja]).unwrap();

    let cadena = maint::history(&conn, nueva).unwrap();
    assert_eq!(cadena.len(), 2, "cadena: {cadena:?}");
    assert_eq!(cadena[0].0, nueva);
    assert_eq!(cadena[1].0, vieja);
}

/// El guardado explícito debe comportarse igual venga de la CLI o de MCP:
/// clasificar cuando no se dan tags, extraer referencias siempre, y derivar
/// el título de la primera línea no vacía. Antes vivía duplicado en dos
/// lugares y la rama de la CLI no clasificaba ni extraía nada.
#[test]
fn guardar_explicito_clasifica_y_extrae_refs() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let texto = "Se decidió revertir: había un bug en los tests de ABC-42 (PR #7).";

    let (id, usados) = anathem::ingest::guardar_explicito(&conn, texto, "p", Vec::new()).unwrap();
    assert!(!usados.is_empty(), "no clasificó: {usados:?}");

    let n_tags: i64 = conn.query_row(
        "select count(*) from memory_tags where memory_id=?1", [id], |r| r.get(0)).unwrap();
    let n_refs: i64 = conn.query_row(
        "select count(*) from memory_refs where memory_id=?1", [id], |r| r.get(0)).unwrap();
    assert!(n_tags > 0, "la memoria quedó sin tags en la base");
    assert!(n_refs >= 2, "no extrajo el ticket ABC-42 ni el PR #7: {n_refs} refs");
}

/// Si se pasan tags explícitos, se respetan y NO se clasifica encima.
#[test]
fn guardar_explicito_respeta_los_tags_dados() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let (id, usados) = anathem::ingest::guardar_explicito(
        &conn, "un bug en los tests", "p", vec!["mio".to_string()]).unwrap();
    assert_eq!(usados, vec!["mio".to_string()]);
    let tags: Vec<String> = conn
        .prepare("select tag from memory_tags where memory_id=?1").unwrap()
        .query_map([id], |r| r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(tags, vec!["mio".to_string()], "clasificó encima de los tags dados");
}

/// La CLI y MCP deben usar el MISMO proyecto por defecto. Antes la CLI usaba
/// "default" y MCP "sin-proyecto", así que las memorias guardadas desde la
/// terminal y las guardadas por el agente caían en proyectos distintos y no
/// se veían entre sí: ni para supersession ni para búsquedas filtradas.
#[test]
fn el_proyecto_por_defecto_es_el_mismo_en_cli_y_mcp() {
    // Los dos caminos tienen que derivar el mismo proyecto parados en el
    // mismo lugar. Antes esto se comprobaba mirando si la ayuda mencionaba
    // la constante; ahora que el valor se deriva, se comprueba ejecutando
    // los dos caminos de verdad, que es lo que la invariante siempre quiso
    // decir.
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("carpeta-cualquiera");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(
        repo.join(".git/config"),
        "[remote \"origin\"]\n\turl = git@host:grupo/proyecto-esperado.git\n",
    )
    .unwrap();
    let db = d.path().join("m.db");

    // Camino de la CLI
    std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .current_dir(&repo)
        .args(["--db", db.to_str().unwrap(), "save", "guardada por la cli"])
        .output()
        .unwrap();

    // Camino de MCP, con el servidor parado en el mismo directorio
    let mut s = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .current_dir(&repo)
        .args(["--db", db.to_str().unwrap(), "serve"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let e = s.stdin.as_mut().unwrap();
        writeln!(e, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"t","version":"1"}}}}}}"#).unwrap();
        writeln!(e, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"save_memory","arguments":{{"text":"guardada por mcp"}}}}}}"#).unwrap();
        e.flush().unwrap();
    }
    {
        use std::io::BufRead;
        let o = std::io::BufReader::new(s.stdout.as_mut().unwrap());
        let _: Vec<_> = o.lines().take(2).collect();
    }
    let _ = s.kill();

    let conn = rusqlite::Connection::open(&db).unwrap();
    let proyectos: Vec<String> = conn
        .prepare("select project from memories order by id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        proyectos,
        vec!["proyecto-esperado".to_string(), "proyecto-esperado".to_string()],
        "la cli y mcp tienen que derivar el mismo proyecto desde el mismo directorio"
    );
}
