use anathem::{chunk, db, ingest, model::NewMemory};

#[test]
fn short_text_yields_one_chunk() {
    let c = chunk::split("hola mundo", 500, 400);
    assert_eq!(c, vec!["hola mundo".to_string()]);
}

#[test]
fn respeta_tamano_y_paso() {
    let text: String = std::iter::repeat('a').take(1000).collect();
    let c = chunk::split(&text, 500, 400);
    assert_eq!(c.len(), 3, "1000 chars con paso 400 dan 3 chunks");
    assert_eq!(c[0].chars().count(), 500);
    assert_eq!(c[2].chars().count(), 200); // 800..1000
}

#[test]
fn it_does_not_split_multibyte_characters() {
    let text: String = std::iter::repeat('ñ').take(600).collect();
    let c = chunk::split(&text, 500, 400);
    assert_eq!(c[0].chars().count(), 500);
    assert!(c.iter().all(|s| s.chars().all(|ch| ch == 'ñ')));
}

#[test]
fn empty_text_yields_no_chunks() {
    assert!(chunk::split("", 500, 400).is_empty());
}

#[test]
fn ingesta_crea_memoria_chunks_tags_y_refs() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let m = NewMemory {
        project: "acme-billing".into(),
        title: "cache rechazado".into(),
        body: "El cache de PROJ-482 no se justifica. Medido en PR #4821.".into(),
        created: "2026-09-21".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: Some("QX5XT61pZrT8SivkMkEzo9".into()),
        tags: vec!["performance".into(), "code-review".into()],
        refs: vec![("ticket".into(), "PROJ-482".into()), ("pr".into(), "4821".into())],
    };
    let id = ingest::insert(&conn, &m).unwrap();
    assert!(id > 0);

    let n_chunks: i64 = conn
        .query_row("select count(*) from chunks where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(n_chunks, 1);

    let n_tags: i64 = conn
        .query_row("select count(*) from memory_tags where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(n_tags, 2);

    let n_refs: i64 = conn
        .query_row("select count(*) from memory_refs where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(n_refs, 2);

    // the FTS5 index must have been populated by trigger
    let n_fts: i64 = conn
        .query_row("select count(*) from chunks_fts where chunks_fts match 'justifica'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_fts, 1, "el trigger de FTS5 no pobló el índice");
}

#[test]
fn a_ref_with_an_invalid_kind_fails_instead_of_being_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let m = NewMemory {
        project: "acme-billing".into(),
        title: "t".into(),
        body: "cuerpo de prueba".into(),
        created: "2026-09-21".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: None,
        tags: vec![],
        refs: vec![("TYPO".into(), "1".into())],
    };

    let err = ingest::insert(&conn, &m).expect_err("un kind inválido debe fallar, no ignorarse");
    assert!(
        err.to_string().contains("TYPO"),
        "el error debe nombrar el kind ofensor: {err}"
    );

    // the whole transaction is discarded: not even the inserted memory remains
    let n_memorias: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n_memorias, 0, "un ref inválido debe hacer rollback de toda la transacción");
}

#[test]
fn duplicate_valid_refs_insert_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let m = NewMemory {
        project: "acme-billing".into(),
        title: "t".into(),
        body: "cuerpo de prueba".into(),
        created: "2026-09-21".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: None,
        tags: vec![],
        // the same (kind, value) pair twice, as can happen after extraction
        refs: vec![("ticket".into(), "PROJ-482".into()), ("ticket".into(), "PROJ-482".into())],
    };

    let id = ingest::insert(&conn, &m).unwrap();

    let n_refs: i64 = conn
        .query_row("select count(*) from memory_refs where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(n_refs, 1, "el duplicado válido no debe fallar ni duplicarse");
}
