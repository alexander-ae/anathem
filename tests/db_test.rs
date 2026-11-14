use anathem::db;
use anathem::ingest;
use anathem::model::NewMemory;
use rusqlite;

#[test]
fn it_opens_and_creates_the_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let conn = db::open(&path).unwrap();

    let tables: Vec<String> = conn
        .prepare("select name from sqlite_master where type in ('table','view') order by name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    for t in ["memories", "memory_tags", "memory_refs", "chunks", "chunks_fts", "search_log"] {
        assert!(tables.contains(&t.to_string()), "table {t} is missing; there are {tables:?}");
    }
}

#[test]
fn fts5_esta_disponible() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    conn.execute_batch("create virtual table probe using fts5(x)")
        .expect("FTS5 is not compiled into this SQLite build");
}

#[test]
fn open_es_idempotente() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    db::open(&path).unwrap();
    db::open(&path).unwrap(); // no debe fallar al reabrir
}

#[test]
fn deleting_a_memory_takes_its_vectors_with_it() {
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
    assert_eq!(quedan, 0, "the cascade has to take the vectors");
}

#[test]
fn meta_stores_key_value_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    conn.execute("insert into meta (key, value) values ('pack_nombre','en-es')", []).unwrap();
    let v: String = conn
        .query_row("select value from meta where key='pack_nombre'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "en-es");
}
