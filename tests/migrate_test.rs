use anathem::{db, migrate};
use std::path::Path;

fn migrated_one(conn: &rusqlite::Connection, ext: &str) -> Option<(String, String, String)> {
    conn.query_row(
        "select project, title, created from memories where ext_id = ?1",
        [ext],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

#[test]
fn it_migrates_only_curated_ones_and_reports() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let rep = migrate::run(&conn, Path::new("tests/fixtures/export-sample.jsonl")).unwrap();

    assert_eq!(rep.migrated, 3, "aaa, bbb, eee");
    assert_eq!(rep.archived, 2, "ccc, ddd");
    assert_eq!(rep.failed, 2, "fff, ggg");

    assert!(migrated_one(&conn, "aaa").is_some());
    assert!(migrated_one(&conn, "ccc").is_none(), "no debe migrar session_turn");
    assert!(migrated_one(&conn, "fff").is_none(), "no debe migrar status failed");
}

#[test]
fn it_uses_the_date_from_the_save_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    migrate::run(&conn, Path::new("tests/fixtures/export-sample.jsonl")).unwrap();

    let (_, title, created) = migrated_one(&conn, "aaa").unwrap();
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
fn it_reports_curated_failures_for_manual_reimport() {
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
fn is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let p = Path::new("tests/fixtures/export-sample.jsonl");
    migrate::run(&conn, p).unwrap();
    migrate::run(&conn, p).unwrap();
    let n: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3, "correr dos veces no debe duplicar");
}
