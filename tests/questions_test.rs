use anathem::{db, export, import, ingest, model::NewMemory, search};
use std::path::Path;
use std::process::Command;

fn mem(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap();
    (o.status.success(),
     String::from_utf8_lossy(&o.stdout).to_string(),
     String::from_utf8_lossy(&o.stderr).to_string())
}

#[test]
fn a_question_finds_the_memory_its_body_would_not() {
    let d = tempfile::tempdir().unwrap();
    let (ok, out, err) = mem(d.path(), &[
        "save", "el proveedor de pagos es Adyen", "--project", "p",
        "--question", "¿quién cobra las suscripciones?",
    ]);
    assert!(ok, "{out} {err}");

    // None of these words is in the body.
    let (ok, out, _) = mem(d.path(), &["search", "cobra suscripciones", "--project", "p", "--json"]);
    assert!(ok, "{out}");
    assert!(out.contains("Adyen"), "the question brought it: {out}");

    let (ok, out, _) = mem(d.path(), &["show", "1"]);
    assert!(ok && out.contains("question: ¿quién cobra"), "{out}");
}

#[test]
fn the_question_never_appears_in_the_excerpt() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _, err) = mem(d.path(), &[
        "save", "el proveedor de pagos es Adyen", "--project", "p",
        "--question", "palabra que solo existe en la pregunta",
    ]);
    assert!(ok, "{err}");
    let (_, out, _) = mem(d.path(), &["search", "proveedor", "--project", "p"]);
    assert!(!out.contains("solo existe en la pregunta"),
        "the excerpt is the body, not the bait: {out}");
}

#[test]
fn questions_survive_reindex_and_the_export() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "el proveedor de pagos es Adyen".into(),
        body: "el proveedor de pagos es Adyen".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("a".into()),
        tags: vec![], refs: vec![],
    }).unwrap();
    anathem::questions::add(&conn, id, &["¿quién cobra?".to_string()]).unwrap();

    anathem::edit::reindex(&conn).unwrap();
    let q = search::Query { text: "cobra".into(), ..Default::default() };
    assert!(!search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().is_empty(),
        "reindex keeps the question indexed");

    let out = dir.path().join("out");
    export::to_markdown(&conn, &out).unwrap();
    let otro = db::open(&dir.path().join("otro.db")).unwrap();
    import::import_dir(&otro, &out, false).unwrap();
    let found = search::run(&otro,
        &search::Query { text: "cobra".into(), ..Default::default() },
        &search::SearchContext::bm25_only()).unwrap();
    assert!(!found.is_empty(), "and it travels in the export");
}
