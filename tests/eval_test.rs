use anathem::{db, evalrun, ingest, model::NewMemory, search};
use std::io::Write;

#[test]
fn it_computes_metrics_over_a_known_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for (ext, body) in [
        ("a", "El cache del breakdown no se justifica por staleness"),
        ("b", "Convenciones de rspec para request specs"),
        ("c", "El design system interno define los colores"),
    ] {
        ingest::insert(&conn, &NewMemory {
            project: "p".into(), title: body.into(), body: body.into(),
            created: "2026-09-01".into(), event_start: None, event_end: None,
            source: "migrated".into(), ext_id: Some(ext.into()),
            tags: vec![], refs: vec![],
        }).unwrap();
    }

    let evalset = dir.path().join("evalset.json");
    let mut f = std::fs::File::create(&evalset).unwrap();
    write!(f, r#"{{
      "corpus": "x", "created": "2026-09-21",
      "cases": [
        {{"id":"q1","query":"cache staleness","expect_docs":["a"],"kind":"factual"}},
        {{"id":"q2","query":"rspec request specs","expect_docs":["b"],"kind":"conventional"}},
        {{"id":"q3","query":"algo que no existe en ningun documento","expect_docs":["c"],"kind":"factual"}}
      ]}}"#).unwrap();

    let r = evalrun::run(&conn, &evalset, "anathem bm25", &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(r.n_cases, 3);
    // q1 and q2 hit at rank 1; q3 does not hit
    assert!((r.recall_at_5 - 2.0 / 3.0).abs() < 1e-9, "recall@5 = {}", r.recall_at_5);
    assert!((r.mrr - 2.0 / 3.0).abs() < 1e-9, "MRR = {}", r.mrr);
    assert_eq!(r.per_case.len(), 3);
    assert_eq!(r.per_case[0].first_rank, Some(1));
    assert_eq!(r.per_case[2].first_rank, None);
}

#[test]
fn a_hit_without_an_ext_id_is_not_dropped_from_the_rank_count() {
    // Regression I1: a memory with no ext_id (created by MCP's save_memory,
    // say, and not by `migrate`) that ranks above the expected match must not
    // push the reported rank upward. `first_rank` has to reflect the real
    // position within `hits`, not the position within the list already
    // filtered by ext_id.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();

    // No ext_id, far shorter and with more repetitions of the query's term:
    // bm25 scores it better (more negative) than the target document.
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "gato".into(),
        body: "gato gato gato gato gato".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap();
    // The target document: the same term, but diluted in a longer body, so
    // bm25 scores it worse and it falls behind the previous one.
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "gato".into(),
        body: "gato relleno relleno relleno relleno relleno relleno relleno relleno relleno".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("x".into()),
        tags: vec![], refs: vec![],
    }).unwrap();

    let evalset = dir.path().join("evalset.json");
    std::fs::write(&evalset, r#"{"corpus":"x","created":"2026-09-21","cases":[
        {"id":"q1","query":"gato","expect_docs":["x"],"kind":"factual"}
    ]}"#).unwrap();

    let r = evalrun::run(&conn, &evalset, "anathem bm25", &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(r.per_case.len(), 1);
    // The hit with no ext_id takes rank 1 without ever matching; the target
    // shows up at rank 2, not rank 1.
    assert_eq!(
        r.per_case[0].first_rank,
        Some(2),
        "el rango debe contar la posición del hit sin ext_id, no saltarla: per_case={:?}",
        r.per_case[0]
    );
    assert!(!r.per_case[0].hit_at_5 || r.per_case[0].first_rank != Some(1));
}

#[test]
fn it_serializes_to_the_shared_format() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let evalset = dir.path().join("evalset.json");
    std::fs::write(&evalset, r#"{"corpus":"x","created":"2026-09-21","cases":[{"id":"q1","query":"nada","expect_docs":["z"],"kind":"factual"}]}"#).unwrap();
    let r = evalrun::run(&conn, &evalset, "anathem bm25", &search::SearchContext::bm25_only()).unwrap();
    let j: serde_json::Value = serde_json::to_value(&r).unwrap();
    for k in ["system", "n_cases", "recall_at_5", "recall_at_50", "mrr", "p50_ms", "per_case"] {
        assert!(j.get(k).is_some(), "falta la clave {k} en {j}");
    }
}

#[test]
fn a_case_scoped_to_a_project_does_not_see_the_other_projects() {
    // A benchmark like LongMemEval gives every question its own haystack:
    // the evidence has to be found among that user's sessions, not among
    // everyone's. Pooling them into one database and searching all of it
    // measures something harder than the benchmark defines, and the number
    // stops being comparable with anything published. So a case can name the
    // project it belongs to, and the search is confined to it.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for (project, ext) in [("q1", "q1-s3"), ("q2", "q2-s7")] {
        ingest::insert(&conn, &NewMemory {
            project: project.into(),
            title: "the deploy was rolled back after the cache went stale".into(),
            body: "the deploy was rolled back after the cache went stale".into(),
            created: "2026-09-01".into(), event_start: None, event_end: None,
            source: "migrated".into(), ext_id: Some(ext.into()),
            tags: vec![], refs: vec![],
        }).unwrap();
    }

    let evalset = dir.path().join("evalset.json");
    let mut f = std::fs::File::create(&evalset).unwrap();
    write!(f, r#"{{"cases": [
        {{"id":"q1","query":"cache went stale","project":"q1","expect_docs":["q1-s3"]}}
      ]}}"#).unwrap();

    let r = evalrun::run(&conn, &evalset, "anathem bm25", &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(r.per_case[0].returned, vec!["q1-s3".to_string()],
        "the case names project q1, so q2's session must not be in the results");
    assert_eq!(r.per_case[0].first_rank, Some(1));
}
