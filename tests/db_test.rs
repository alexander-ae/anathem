use anathem::db;
use anathem::ingest;
use anathem::model::NewMemory;
use rusqlite;

#[test]
fn abre_y_crea_esquema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let conn = db::open(&path).unwrap();

    let tablas: Vec<String> = conn
        .prepare("select name from sqlite_master where type in ('table','view') order by name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    for t in ["memories", "memory_tags", "memory_refs", "chunks", "chunks_fts", "search_log"] {
        assert!(tablas.contains(&t.to_string()), "falta la tabla {t}; hay {tablas:?}");
    }
}

#[test]
fn fts5_esta_disponible() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    conn.execute_batch("create virtual table probe using fts5(x)")
        .expect("FTS5 no está compilado en esta build de SQLite");
}

#[test]
fn open_es_idempotente() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    db::open(&path).unwrap();
    db::open(&path).unwrap(); // no debe fallar al reabrir
}

#[test]
fn borrar_una_memoria_se_lleva_sus_vectores() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = ingest::insert(&conn, &NewMemory {
        project: "p".into(),
        title: "t".into(),
        body: "un cuerpo cualquiera que produce al menos un fragmento".into(),
        created: "2026-09-01".into(),
        event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap();
    let chunk: i64 = conn
        .query_row("select id from chunks where memory_id=?1", [id], |r| r.get(0))
        .unwrap();
    conn.execute("insert into chunk_vectors (chunk_id, vec) values (?1, ?2)",
                 rusqlite::params![chunk, vec![0u8; 16]]).unwrap();

    conn.execute("delete from memories where id=?1", [id]).unwrap();

    let quedan: i64 = conn
        .query_row("select count(*) from chunk_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(quedan, 0, "el cascade debe llevarse los vectores");
}

#[test]
fn meta_guarda_pares_clave_valor() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    conn.execute("insert into meta (clave, valor) values ('pack_nombre','en-es')", []).unwrap();
    let v: String = conn
        .query_row("select valor from meta where clave='pack_nombre'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "en-es");
}
