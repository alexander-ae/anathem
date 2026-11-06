//! `mem tune` builds its own eval set out of the topic keys, so that finding
//! a better `bm25_weight` costs nobody an afternoon of labelling.
//!
//! The measurement that justifies it: on LongMemEval the best weight is 1.0
//! and on a corpus of real technical notes it is 8.0, so no single default
//! serves both and the number has to be found per corpus.

use anathem::{db, ingest, model::NewMemory, tune};
use rusqlite::Connection;

fn save(conn: &Connection, title: &str, body: &str, topic: Option<&str>) -> i64 {
    let id = ingest::insert(conn, &NewMemory {
        project: "p".into(), title: title.into(), body: body.into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    if let Some(t) = topic {
        conn.execute("update memories set topic = ?2 where id = ?1", rusqlite::params![id, t]).unwrap();
    }
    id
}

#[test]
fn it_makes_one_case_per_topic_key() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let a = save(&conn, "pagos", "El proveedor de pagos es Adyen", Some("payment-provider"));
    let b = save(&conn, "ramas", "Las ramas salen de main", Some("branching-policy"));
    save(&conn, "suelta", "Una nota sin clave de tema", None);

    let cases = tune::cases_from_topics(&conn).unwrap();
    assert_eq!(cases.len(), 2, "only the memories carrying a key: {cases:?}");

    let by_query: Vec<&str> = cases.iter().map(|c| c.query.as_str()).collect();
    // The key is the query, with the hyphens opened out: it is written as a
    // slug and asked as words.
    assert!(by_query.contains(&"payment provider"), "{by_query:?}");
    assert!(by_query.contains(&"branching policy"), "{by_query:?}");

    // The expectation is by id, because save_memory assigns no ext_id and
    // that is exactly the gap `id:` was added to close.
    let expected: Vec<String> = cases.iter().flat_map(|c| c.expect_docs.clone()).collect();
    assert!(expected.contains(&format!("id:{a}")), "{expected:?}");
    assert!(expected.contains(&format!("id:{b}")), "{expected:?}");
}

#[test]
fn a_superseded_memory_does_not_become_a_case() {
    // The point of a topic key is that the new version replaces the old one.
    // Asking the corpus to return the old one would measure a failure that
    // is the design working.
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let old = save(&conn, "pagos", "El proveedor de pagos es Stripe", Some("payment-provider"));
    conn.execute("update memories set status = 'superseded' where id = ?1", [old]).unwrap();
    let new = save(&conn, "pagos", "El proveedor de pagos es Adyen", Some("payment-provider"));

    let cases = tune::cases_from_topics(&conn).unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].expect_docs, vec![format!("id:{new}")]);
}

#[test]
fn it_refuses_a_corpus_with_too_few_keys_and_says_how_many_there_are() {
    // Tuning on three questions would recommend noise with a straight face.
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    save(&conn, "uno", "cuerpo uno", Some("tema-uno"));
    save(&conn, "dos", "cuerpo dos", Some("tema-dos"));

    let e = tune::check_enough(2).unwrap_err().to_string();
    assert!(e.contains('2'), "it has to say how many there are: {e}");
    assert!(e.contains(&tune::MINIMUM_CASES.to_string()), "and how many it wants: {e}");
}

#[test]
fn it_recommends_the_weight_with_the_best_mrr() {
    // The shape measured on the real corpus: a plateau from 4 to 16 rather
    // than a spike, which is what makes it believable instead of overfitted.
    let scored = [(0.5, 0.388), (1.0, 0.409), (4.0, 0.454), (8.0, 0.466), (16.0, 0.455)];
    assert_eq!(tune::best_weight(&scored), 8.0);
}

#[test]
fn with_one_weight_it_recommends_that_one() {
    assert_eq!(tune::best_weight(&[(1.0, 0.5)]), 1.0);
}
