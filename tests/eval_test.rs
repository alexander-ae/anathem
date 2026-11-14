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
        assert!(j.get(k).is_some(), "key {k} is missing in {j}");
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

#[test]
fn an_expectation_can_name_a_memory_by_id_when_it_has_no_ext_id() {
    // `expect_docs` matched `memories.ext_id` and nothing else, and only
    // memories that came through `migrate` or `import` have one. Everything
    // `save_memory` wrote — which is everything anathem saved by itself —
    // could never be the answer to an eval case, so `mem eval` could measure
    // an imported corpus and not the corpus a person actually accumulated.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = ingest::insert(&conn, &NewMemory {
        project: "p".into(),
        title: "branching policy".into(),
        body: "Las ramas salen de main y vuelven por merge request".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap();

    let evalset = dir.path().join("evalset.json");
    let mut f = std::fs::File::create(&evalset).unwrap();
    write!(f, r#"{{"cases": [
        {{"id":"q1","query":"ramas main merge request","expect_docs":["id:{id}"],"kind":"conventional"}}
      ]}}"#).unwrap();

    let r = evalrun::run(&conn, &evalset, "bm25", &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(r.per_case[0].first_rank, Some(1), "per_case: {:?}", r.per_case);
}

#[test]
fn an_ext_id_that_looks_like_a_number_is_still_an_ext_id() {
    // The `id:` prefix is what makes the two namespaces distinguishable. A
    // bare "42" has to keep meaning the ext_id "42", or adding the feature
    // would silently change what existing eval sets measure.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "uno".into(),
        body: "El cache del breakdown no se justifica".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("42".into()),
        tags: vec![], refs: vec![],
    }).unwrap();

    let evalset = dir.path().join("evalset.json");
    let mut f = std::fs::File::create(&evalset).unwrap();
    write!(f, r#"{{"cases": [
        {{"id":"q1","query":"cache breakdown","expect_docs":["42"],"kind":"factual"}}
      ]}}"#).unwrap();

    let r = evalrun::run(&conn, &evalset, "bm25", &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(r.per_case[0].first_rank, Some(1), "per_case: {:?}", r.per_case);
}

#[test]
fn returned_names_every_hit_including_the_ones_without_an_ext_id() {
    // `per_case[].returned` is what `docs/benchmarks.md` tells you to compute
    // the strict metrics from, and it listed only the memories that have an
    // ext_id. A list that silently omits entries cannot be used to work out
    // what fraction of a case's evidence landed in the top k — the positions
    // do not line up with the ranks the same block reports.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let sin_ext = ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "gato".into(),
        body: "gato gato gato gato gato".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None,
        tags: vec![], refs: vec![],
    }).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "gato".into(),
        body: "gato relleno relleno relleno relleno relleno relleno relleno relleno relleno".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("x".into()),
        tags: vec![], refs: vec![],
    }).unwrap();

    let evalset = dir.path().join("evalset.json");
    std::fs::write(&evalset, r#"{"cases":[
        {"id":"q1","query":"gato","expect_docs":["x"],"kind":"factual"}
    ]}"#).unwrap();

    let r = evalrun::run(&conn, &evalset, "bm25", &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(r.per_case[0].returned, vec![format!("id:{sin_ext}"), "x".to_string()],
        "returned has to line up with the ranks: {:?}", r.per_case[0].returned);
}
