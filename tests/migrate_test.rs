use anathem::{db, migrate};
use std::path::Path;

fn migrada(conn: &rusqlite::Connection, ext: &str) -> Option<(String, String, String)> {
    conn.query_row(
        "select project, title, created from memories where ext_id = ?1",
        [ext],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

#[test]
fn migra_solo_curadas_y_reporta() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let rep = migrate::run(&conn, Path::new("tests/fixtures/export-sample.jsonl")).unwrap();

    assert_eq!(rep.migradas, 3, "aaa, bbb, eee");
    assert_eq!(rep.archivadas, 2, "ccc, ddd");
    assert_eq!(rep.fallidas, 2, "fff, ggg");

    assert!(migrada(&conn, "aaa").is_some());
    assert!(migrada(&conn, "ccc").is_none(), "no debe migrar session_turn");
    assert!(migrada(&conn, "fff").is_none(), "no debe migrar status failed");
}

#[test]
fn usa_la_fecha_del_prefijo_save() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    migrate::run(&conn, Path::new("tests/fixtures/export-sample.jsonl")).unwrap();

    let (_, title, created) = migrada(&conn, "aaa").unwrap();
    assert_eq!(created, "2026-09-21");
    assert!(!title.contains("[SAVE:"), "el prefijo quedó en el título: {title}");
    assert!(title.starts_with("El cache de PROJ-482"), "título: {title}");
}

#[test]
fn asigna_tags_y_refs_al_migrar() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    migrate::run(&conn, Path::new("tests/fixtures/export-sample.jsonl")).unwrap();

    let tags: Vec<String> = conn
        .prepare("select tag from memory_tags t join memories m on m.id=t.memory_id where m.ext_id='aaa' order by tag")
        .unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert!(tags.contains(&"performance".to_string()), "tags: {tags:?}");
    assert!(tags.contains(&"code-review".to_string()), "tags: {tags:?}");

    let refs: Vec<String> = conn
        .prepare("select value from memory_refs r join memories m on m.id=r.memory_id where m.ext_id='aaa' order by value")
        .unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert!(refs.contains(&"PROJ-482".to_string()), "refs: {refs:?}");
}

#[test]
fn reporta_curadas_en_failed_para_reimportar_a_mano() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let rep = migrate::run(&conn, Path::new("tests/fixtures/export-sample.jsonl")).unwrap();

    assert!(
        rep.curated_failed.iter().any(|(ext_id, _)| ext_id == "fff"),
        "fff es project-knowledge failed, debe listarse: {:?}",
        rep.curated_failed
    );
    assert!(
        rep.curated_failed.iter().all(|(ext_id, _)| ext_id != "ggg"),
        "ggg es session_turn (no curado), no debe listarse: {:?}",
        rep.curated_failed
    );
}

#[test]
fn es_idempotente() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let p = Path::new("tests/fixtures/export-sample.jsonl");
    migrate::run(&conn, p).unwrap();
    migrate::run(&conn, p).unwrap();
    let n: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3, "correr dos veces no debe duplicar");
}
