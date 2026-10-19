use anathem::{db, evalrun, ingest, model::NewMemory, search};
use std::io::Write;

#[test]
fn calcula_metricas_sobre_fixture_conocido() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for (ext, body) in [
        ("a", "El cache del breakdown no se justifica por staleness"),
        ("b", "Convenciones de rspec para request specs"),
        ("c", "El design system CF Ruby define los colores"),
    ] {
        ingest::insert(&conn, &NewMemory {
            project: "p".into(), title: body.into(), body: body.into(),
            created: "2026-09-01".into(), event_start: None, event_end: None,
            source: "migrated".into(), ext_id: Some(ext.into()),
            tags: vec![], refs: vec![],
        }).unwrap();
    }

    let es = dir.path().join("evalset.json");
    let mut f = std::fs::File::create(&es).unwrap();
    write!(f, r#"{{
      "corpus": "x", "created": "2026-09-21",
      "cases": [
        {{"id":"q1","query":"cache staleness","expect_docs":["a"],"kind":"factual"}},
        {{"id":"q2","query":"rspec request specs","expect_docs":["b"],"kind":"conventional"}},
        {{"id":"q3","query":"algo que no existe en ningun documento","expect_docs":["c"],"kind":"factual"}}
      ]}}"#).unwrap();

    let r = evalrun::run(&conn, &es, "anathem bm25", &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(r.n_cases, 3);
    // q1 y q2 aciertan en rango 1; q3 no acierta
    assert!((r.recall_at_5 - 2.0 / 3.0).abs() < 1e-9, "recall@5 = {}", r.recall_at_5);
    assert!((r.mrr - 2.0 / 3.0).abs() < 1e-9, "MRR = {}", r.mrr);
    assert_eq!(r.per_case.len(), 3);
    assert_eq!(r.per_case[0].first_rank, Some(1));
    assert_eq!(r.per_case[2].first_rank, None);
}

#[test]
fn hit_sin_ext_id_no_se_descarta_del_conteo_de_rango() {
    // Regresión I1: un memory sin ext_id (p.ej. creado por save_memory del MCP,
    // no por `migrate`) que rankea mejor que el match esperado no debe correr el
    // rango reportado hacia arriba. `first_rank` debe reflejar la posición real
    // dentro de `hits`, no la posición dentro de la lista ya filtrada por ext_id.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();

    // Sin ext_id, mucho más corto y con más repeticiones del término de la
    // query: bm25 lo puntúa mejor (más negativo) que el documento objetivo.
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "gato".into(),
        body: "gato gato gato gato gato".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap();
    // El documento objetivo: mismo término, pero diluido en un cuerpo más
    // largo, así que bm25 lo puntúa peor y cae detrás del anterior.
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "gato".into(),
        body: "gato relleno relleno relleno relleno relleno relleno relleno relleno relleno".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("x".into()),
        tags: vec![], refs: vec![],
    }).unwrap();

    let es = dir.path().join("evalset.json");
    std::fs::write(&es, r#"{"corpus":"x","created":"2026-09-21","cases":[
        {"id":"q1","query":"gato","expect_docs":["x"],"kind":"factual"}
    ]}"#).unwrap();

    let r = evalrun::run(&conn, &es, "anathem bm25", &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(r.per_case.len(), 1);
    // El hit sin ext_id ocupa el rango 1 sin matchear nunca; el objetivo
    // aparece en rango 2, no en rango 1.
    assert_eq!(
        r.per_case[0].first_rank,
        Some(2),
        "el rango debe contar la posición del hit sin ext_id, no saltarla: per_case={:?}",
        r.per_case[0]
    );
    assert!(!r.per_case[0].hit_at_5 || r.per_case[0].first_rank != Some(1));
}

#[test]
fn serializa_al_formato_compartido() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let es = dir.path().join("evalset.json");
    std::fs::write(&es, r#"{"corpus":"x","created":"2026-09-21","cases":[{"id":"q1","query":"nada","expect_docs":["z"],"kind":"factual"}]}"#).unwrap();
    let r = evalrun::run(&conn, &es, "anathem bm25", &search::Contexto::solo_bm25()).unwrap();
    let j: serde_json::Value = serde_json::to_value(&r).unwrap();
    for k in ["system", "n_cases", "recall_at_5", "recall_at_50", "mrr", "p50_ms", "per_case"] {
        assert!(j.get(k).is_some(), "falta la clave {k} en {j}");
    }
}
