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
fn forget_removes_from_search_without_deleting() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "a");
    maint::forget(&conn, id).unwrap();

    let q = search::Query { text: "cache".into(), ..Default::default() };
    assert!(search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().is_empty(), "forget debe sacarla de la búsqueda");

    let n: i64 = conn.query_row("select count(*) from memories where id=?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "forget no debe borrar la fila");
}

#[test]
fn pin_marks_the_memory() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "a");
    maint::pin(&conn, id, true).unwrap();
    let p: i64 = conn.query_row("select pinned from memories where id=?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(p, 1);
}

#[test]
fn forget_on_a_missing_id_fails() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let err = maint::forget(&conn, 999_999).expect_err("forget sobre un id inexistente debe fallar");
    assert!(err.to_string().contains("999999"), "mensaje inesperado: {err}");
}

#[test]
fn pin_on_a_missing_id_fails() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let err = maint::pin(&conn, 999_999, true).expect_err("pin sobre un id inexistente debe fallar");
    assert!(err.to_string().contains("999999"), "mensaje inesperado: {err}");
}

#[test]
fn history_returns_the_supersession_chain() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let older = seed(&conn, "vieja");
    let newer = seed(&conn, "nueva");
    conn.execute("update memories set supersedes_id=?1 where id=?2", [older, newer]).unwrap();
    conn.execute("update memories set status='superseded' where id=?1", [older]).unwrap();

    let chain = maint::history(&conn, newer).unwrap();
    assert_eq!(chain.len(), 2, "cadena: {chain:?}");
    assert_eq!(chain[0].0, newer);
    assert_eq!(chain[1].0, older);
}

/// An explicit save has to behave the same whether it comes from the CLI or
/// from MCP: classify when no tags are given, always extract references, and
/// derive the title from the first non-empty line. It used to live duplicated
/// in two places and the CLI branch classified and extracted nothing.
#[test]
fn save_explicit_classifies_and_extracts_refs() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let text = "Se decidió revertir: había un bug en los tests de ABC-42 (PR #7).";

    let (id, used) = anathem::ingest::save_explicit(&conn, text, "p", Vec::new()).unwrap();
    assert!(!used.is_empty(), "no clasificó: {used:?}");

    let n_tags: i64 = conn.query_row(
        "select count(*) from memory_tags where memory_id=?1", [id], |r| r.get(0)).unwrap();
    let n_refs: i64 = conn.query_row(
        "select count(*) from memory_refs where memory_id=?1", [id], |r| r.get(0)).unwrap();
    assert!(n_tags > 0, "la memoria quedó sin tags en la base");
    assert!(n_refs >= 2, "no extrajo el ticket ABC-42 ni el PR #7: {n_refs} refs");
}

/// When explicit tags are passed they are respected and NOT classified over.
#[test]
fn save_explicit_respects_the_given_tags() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let (id, used) = anathem::ingest::save_explicit(
        &conn, "un bug en los tests", "p", vec!["mio".to_string()]).unwrap();
    assert_eq!(used, vec!["mio".to_string()]);
    let tags: Vec<String> = conn
        .prepare("select tag from memory_tags where memory_id=?1").unwrap()
        .query_map([id], |r| r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(tags, vec!["mio".to_string()], "clasificó encima de los tags dados");
}

/// The CLI and MCP have to use the SAME default project. The CLI used to use
/// "default" and MCP "sin-proyecto", so memories saved from the terminal and
/// memories saved by the agent landed in different projects and could not see
/// each other: not for supersession, not for filtered searches.
#[test]
fn the_default_project_is_the_same_in_cli_and_mcp() {
    // Both paths have to derive the same project when standing in the same
    // place. This used to be checked by looking at whether the help text
    // mentioned the constant; now that the value is derived, it is checked
    // by really running both paths, which is what the invariant always meant
    // to say.
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("carpeta-cualquiera");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(
        repo.join(".git/config"),
        "[remote \"origin\"]\n\turl = git@host:grupo/proyecto-esperado.git\n",
    )
    .unwrap();
    let db = d.path().join("m.db");

    // The CLI path
    std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .current_dir(&repo)
        .args(["--db", db.to_str().unwrap(), "save", "guardada por la cli"])
        .output()
        .unwrap();

    // The MCP path, with the server standing in the same directory
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
    let projects: Vec<String> = conn
        .prepare("select project from memories order by id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        projects,
        vec!["proyecto-esperado".to_string(), "proyecto-esperado".to_string()],
        "la cli y mcp tienen que derivar el mismo proyecto desde el mismo directorio"
    );
}
