use anathem::{anchor, db, ingest, model::NewMemory};
use rusqlite::Connection;

fn seed(conn: &Connection, ext: &str, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "demo".into(),
        title: body.chars().take(60).collect(),
        body: body.into(),
        created: "2026-11-01".into(),
        event_start: None, event_end: None,
        source: "explicit".into(),
        ext_id: Some(ext.into()),
        tags: vec![], refs: vec![],
    }).unwrap()
}

#[test]
fn an_anchor_reports_ok_then_moved_then_missing() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "fn run() { 1 }\n").unwrap();

    let id = seed(&conn, "a", "run returns 1");
    anchor::set(&conn, id, "a.rs:fn run", dir.path()).unwrap();
    assert_eq!(
        anchor::check(&conn, id, dir.path()).unwrap().as_deref(),
        Some(anchor::OK),
        "the file has not changed"
    );

    std::fs::write(&file, "fn run() { 2 }\n").unwrap();
    assert_eq!(
        anchor::check(&conn, id, dir.path()).unwrap().as_deref(),
        Some(anchor::MOVED),
        "the file changed under the memory"
    );

    std::fs::remove_file(&file).unwrap();
    assert_eq!(
        anchor::check(&conn, id, dir.path()).unwrap().as_deref(),
        Some(anchor::MISSING),
        "the file is gone"
    );

    let (n, stale) = anchor::counts(&conn).unwrap();
    assert_eq!((n, stale), (1, 1), "one anchor, and it is not ok");
}

#[test]
fn an_anchor_to_a_file_that_is_not_there_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "a", "run returns 1");

    let err = anchor::set(&conn, id, "nope.rs", dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("not a file"),
        "an anchor nobody can check is not an anchor: {err}"
    );
    assert_eq!(anchor::get(&conn, id).unwrap(), None, "and nothing was written");
}

#[test]
fn a_memory_without_an_anchor_is_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "a", "run returns 1");

    assert_eq!(anchor::check(&conn, id, dir.path()).unwrap(), None);
    assert_eq!(anchor::counts(&conn).unwrap(), (0, 0));
}

#[test]
fn the_symbol_is_parsed_only_when_it_is_one() {
    assert_eq!(
        anchor::parse("src/search.rs:fn run_explained"),
        ("src/search.rs".to_string(), Some("fn run_explained".to_string()))
    );
    assert_eq!(anchor::parse("src/a.rs"), ("src/a.rs".to_string(), None));
    // A Windows drive letter is not a symbol.
    assert_eq!(anchor::parse("C:\\src\\a.rs"), ("C:\\src\\a.rs".to_string(), None));
}

#[test]
fn a_symbol_anchor_ignores_changes_elsewhere_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let file = dir.path().join("f.rs");
    std::fs::write(&file, "fn a() { 1 }\nfn b() { 2 }\n").unwrap();

    let id = seed(&conn, "a", "b returns 2");
    anchor::set(&conn, id, "f.rs:b", dir.path()).unwrap();
    assert_eq!(anchor::check(&conn, id, dir.path()).unwrap().as_deref(), Some(anchor::OK));

    // The whole point: another function changing is not this memory going bad.
    std::fs::write(&file, "fn a() { 99 }\nfn b() { 2 }\n").unwrap();
    assert_eq!(
        anchor::check(&conn, id, dir.path()).unwrap().as_deref(),
        Some(anchor::OK),
        "the anchored function did not change"
    );

    std::fs::write(&file, "fn a() { 99 }\nfn b() { 3 }\n").unwrap();
    assert_eq!(anchor::check(&conn, id, dir.path()).unwrap().as_deref(), Some(anchor::MOVED));

    // Renamed: the file is there and the function is not.
    std::fs::write(&file, "fn a() { 99 }\nfn c() { 3 }\n").unwrap();
    assert_eq!(anchor::check(&conn, id, dir.path()).unwrap().as_deref(), Some(anchor::MISSING));
}

#[test]
fn a_symbol_that_is_only_called_is_not_a_definition() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let file = dir.path().join("f.rs");
    std::fs::write(&file, "fn main() {\n    run();\n}\n").unwrap();

    let id = seed(&conn, "a", "run is called");
    // `run` appears first as a call; the block it must not hash is main's.
    anchor::set(&conn, id, "f.rs:run", dir.path()).unwrap();
    std::fs::write(&file, "fn main() {\n    run();\n    extra();\n}\n").unwrap();
    assert_eq!(
        anchor::check(&conn, id, dir.path()).unwrap().as_deref(),
        Some(anchor::MISSING),
        "a call is not a definition, and the symbol the anchor names does not exist"
    );
}
