use anathem::{db, ingest, model::NewMemory, search, supersede};

fn seed(conn: &rusqlite::Connection, ext: &str, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(), title: body.chars().take(40).collect(), body: body.into(),
        created: "2026-09-21".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: Some(ext.into()),
        tags: vec![], refs: vec![],
    }).unwrap()
}

#[test]
fn it_proposes_similar_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "vieja", "El cache del breakdown se justifica por rendimiento");
    seed(&conn, "otra", "El design system define los colores de la plataforma");

    let c = supersede::candidates(&conn, "El cache del breakdown NO se justifica", "p", 5).unwrap();
    assert!(!c.is_empty(), "no propuso candidatos");
    assert_eq!(c[0].ext_id.as_deref(), Some("vieja"));
}

#[test]
fn link_marks_the_older_one_and_removes_it_from_search() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let older = seed(&conn, "vieja", "El cache del breakdown se justifica");
    let newer = seed(&conn, "nueva", "El cache del breakdown no se justifica, medido");
    supersede::link(&conn, newer, older).unwrap();

    let st: String = conn.query_row("select status from memories where id=?1", [older], |r| r.get(0)).unwrap();
    assert_eq!(st, "superseded");
    let sup: i64 = conn.query_row("select supersedes_id from memories where id=?1", [newer], |r| r.get(0)).unwrap();
    assert_eq!(sup, older);

    let q = search::Query { text: "cache breakdown justifica".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert!(hits.iter().all(|h| h.ext_id.as_deref() != Some("vieja")), "la superseded sigue apareciendo");
}

#[test]
fn it_does_not_propose_itself() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "unica", "El cache del breakdown no se justifica");
    let c = supersede::candidates(&conn, "El cache del breakdown no se justifica", "p", 5).unwrap();
    assert!(c.iter().all(|h| h.memory_id != id) || c.is_empty(),
            "se propuso a sí misma: {c:?}");
}

// -- I1: `link` with a missing `newer` -------------------------------------

#[test]
fn link_with_a_missing_newer_one_fails_and_leaves_the_older_alone() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let older = seed(&conn, "vieja", "El cache del breakdown se justifica");

    let err = supersede::link(&conn, 999_999, older)
        .expect_err("link con nueva inexistente debe fallar, no reportar éxito");
    assert!(err.to_string().contains("999999"), "mensaje inesperado: {err}");

    // the older one must not be left orphaned: neither marked superseded
    // nor carrying a supersedes_id, because the whole call has to abort
    // before touching the database.
    let (status, supersedes): (String, Option<i64>) = conn.query_row(
        "select status, supersedes_id from memories where id = ?1",
        [older],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(status, "active", "vieja quedó superseded apuntando a nada");
    assert_eq!(supersedes, None);
}

// -- I2: `link` must not allow building a cycle -----------------------------

#[test]
fn link_refuses_to_close_a_two_step_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let siete = seed(&conn, "siete", "El endpoint de comites ignora el filtro de estado");
    let ocho = seed(&conn, "ocho", "El endpoint de comites ignora el filtro de estado activo");

    supersede::link(&conn, siete, ocho).unwrap();
    let err = supersede::link(&conn, ocho, siete)
        .expect_err("cerrar el ciclo debe fallar, no dejar ambas superseded");

    assert!(
        err.to_string().to_lowercase().contains("ciclo")
            || err.to_string().to_lowercase().contains("activ"),
        "mensaje inesperado: {err}"
    );

    // exactly one active version has to survive
    let active_ones: i64 = conn.query_row(
        "select count(*) from memories where id in (?1, ?2) and status = 'active'",
        [siete, ocho],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(active_ones, 1, "el ciclo dejó sin ninguna versión activa (o dos)");
}

// -- I3: candidates() by id, not by content, with a case that tells them apart --

#[test]
fn candidates_does_not_discard_preexisting_duplicates_from_the_same_project() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let text = "El endpoint de boletas duplica el cobro de cuota mensual";

    // two preexisting active memories, same project, bodies identical to
    // each other -- the real case is 17/18, but those are in different
    // projects; here we reproduce it within one project on purpose, which is
    // the case the by-id rule has to cover even if it is not in today's
    // migrated corpus.
    let dup_a = seed(&conn, "dup_a", text);
    let dup_b = seed(&conn, "dup_b", text);
    // a third one, freshly inserted, with the same body -- it is the one
    // candidates() has to treat as "self" and exclude.
    let recien_insertada = seed(&conn, "nueva", text);

    let c = supersede::candidates(&conn, text, "p", 5).unwrap();

    // the previous rule (exclude by body identical to the query) would have
    // discarded dup_a, dup_b AND the freshly inserted one -- all three share
    // the query's body -- leaving `c` without either of the real duplicates.
    // The by-id rule discards only the one with the highest id.
    assert!(c.iter().any(|h| h.memory_id == dup_a), "no propuso dup_a: {c:?}");
    assert!(c.iter().any(|h| h.memory_id == dup_b), "no propuso dup_b: {c:?}");
    assert!(c.iter().all(|h| h.memory_id != recien_insertada),
            "se propuso a sí misma: {c:?}");
}
