use anathem::{chunk, db, ingest, model::NewMemory};

#[test]
fn texto_corto_da_un_chunk() {
    let c = chunk::split("hola mundo", 500, 400);
    assert_eq!(c, vec!["hola mundo".to_string()]);
}

#[test]
fn respeta_tamano_y_paso() {
    let texto: String = std::iter::repeat('a').take(1000).collect();
    let c = chunk::split(&texto, 500, 400);
    assert_eq!(c.len(), 3, "1000 chars con paso 400 dan 3 chunks");
    assert_eq!(c[0].chars().count(), 500);
    assert_eq!(c[2].chars().count(), 200); // 800..1000
}

#[test]
fn no_parte_caracteres_multibyte() {
    let texto: String = std::iter::repeat('ñ').take(600).collect();
    let c = chunk::split(&texto, 500, 400);
    assert_eq!(c[0].chars().count(), 500);
    assert!(c.iter().all(|s| s.chars().all(|ch| ch == 'ñ')));
}

#[test]
fn texto_vacio_no_da_chunks() {
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

    // el índice FTS5 debe haberse poblado por trigger
    let n_fts: i64 = conn
        .query_row("select count(*) from chunks_fts where chunks_fts match 'justifica'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_fts, 1, "el trigger de FTS5 no pobló el índice");
}

#[test]
fn ref_con_kind_invalido_falla_en_vez_de_ignorarse_en_silencio() {
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

    // la transacción entera se descarta: ni siquiera queda la memoria insertada
    let n_memorias: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n_memorias, 0, "un ref inválido debe hacer rollback de toda la transacción");
}

#[test]
fn refs_validos_duplicados_insertan_limpio() {
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
        // el mismo par (kind, value) dos veces, como puede pasar tras extracción
        refs: vec![("ticket".into(), "PROJ-482".into()), ("ticket".into(), "PROJ-482".into())],
    };

    let id = ingest::insert(&conn, &m).unwrap();

    let n_refs: i64 = conn
        .query_row("select count(*) from memory_refs where memory_id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(n_refs, 1, "el duplicado válido no debe fallar ni duplicarse");
}
