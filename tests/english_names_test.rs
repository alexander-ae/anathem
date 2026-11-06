//! The two Spanish names that survived the move to English, both on disk
//! rather than in the source: `paquetes/` and `sin-proyecto` (#18).
//!
//! Neither causes a bug, so the whole risk is in the migration and the whole
//! reward is consistency on a surface people see. That makes the tests about
//! what must NOT break: an installation that already has the old names keeps
//! working, and nothing goes looking for a pack where there is none.

use anathem::{db, ingest, model::NewMemory, packstore, project};

#[test]
fn a_fresh_installation_calls_the_directory_packs() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(packstore::packs_dir(d.path()), d.path().join("packs"));
}

#[test]
fn an_installation_that_already_has_paquetes_keeps_using_it() {
    // There is a 22 MB file in there, registered in `meta` by name and
    // possibly memory-mapped by a running server. Deciding it now lives
    // somewhere else would turn a working installation into one that
    // silently searches with bm25 only.
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("paquetes")).unwrap();
    assert_eq!(packstore::packs_dir(d.path()), d.path().join("paquetes"));
}

#[test]
fn once_packs_exists_it_wins_even_if_the_old_one_is_still_there() {
    // The way out: whoever renames the directory by hand gets the new name
    // honoured, and the leftover does not drag them back.
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("paquetes")).unwrap();
    std::fs::create_dir(d.path().join("packs")).unwrap();
    assert_eq!(packstore::packs_dir(d.path()), d.path().join("packs"));
}

#[test]
fn the_project_of_last_resort_is_named_in_english() {
    assert_eq!(project::NO_PROJECT, "no-project");
}

#[test]
fn saving_outside_a_repository_writes_the_english_name() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "sin-proyecto".into(), title: "t".into(), body: "cuerpo".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    // Anything still producing the old string — an export taken yesterday, a
    // script, another machine's database — lands on the new name instead of
    // splitting the corpus in two.
    let p: String = conn.query_row("select project from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(p, "no-project");
}

#[test]
fn an_existing_database_has_its_old_project_rewritten_on_open() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("t.db");
    {
        let conn = db::open(&path).unwrap();
        conn.execute(
            "insert into memories (project, title, body, created, updated, source)
             values ('sin-proyecto', 't', 'cuerpo', '2026-09-01', '2026-09-01', 'explicit')",
            []).unwrap();
    }
    let conn = db::open(&path).unwrap();
    let n: i64 = conn.query_row(
        "select count(*) from memories where project = 'no-project'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "opening it has to move the old rows over");
    let old: i64 = conn.query_row(
        "select count(*) from memories where project = 'sin-proyecto'", [], |r| r.get(0)).unwrap();
    assert_eq!(old, 0);
}

#[test]
fn doctor_says_how_to_move_off_the_old_directory_name() {
    // Nothing is moved automatically, so the person has to be told — and
    // told the one condition that makes it safe, since a running server has
    // the file mapped.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("t.db");
    // With no database, `doctor` reports that and stops before it ever
    // looks at the packs directory — correctly, so the fixture needs one.
    db::open(&db).unwrap();
    std::fs::create_dir(d.path().join("paquetes")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .args(["--db", db.to_str().unwrap(), "doctor"])
        .output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("packs"), "it has to name the new directory: {text}");
    assert!(text.contains("paquetes"), "and the old one: {text}");
}
