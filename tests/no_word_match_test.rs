//! A hit no word of the query matched says so.
//!
//! The vector ranking orders the whole corpus by distance and hands back the
//! nearest, so it answers every query — including one whose words appear
//! nowhere in the corpus. Those answers used to be shaped exactly like the
//! good ones: same excerpt, same position, same confidence. A model reading
//! them had nothing to tell them apart with.
//!
//! The label is not a filter, and these tests also pin that down. Reach is
//! what the vector layer measurably contributes, so a memory sharing no word
//! with the query is sometimes the right one; dropping those would cost the
//! only thing the pack buys.
//!
//! The pack has to be a real one: what is being tested is the behaviour of a
//! ranking that answers everything, and a stub would not answer everything.

use anathem::{config::Config, db, mcp, model::NewMemory, search};
use serde_json::json;

fn insert(conn: &rusqlite::Connection, ext: &str, body: &str) {
    anathem::ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-11-06".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: Some(ext.into()),
        tags: vec![],
        refs: vec![],
    })
    .unwrap();
}

fn installed_pack() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    // `packs` is the current name; `paquetes` is what installations made
    // before the rename still carry, and both are honoured in the code under
    // test, so both are looked for here.
    ["packs", "paquetes"]
        .iter()
        .map(|d| home.join(".anathem").join(d).join("en-es.anathem-pack"))
        .find(|p| p.is_file())
}

/// Returns `None` when this machine has no pack, which is not a failure:
/// there is nothing to measure without a ranking that answers everything.
fn answer(query: &str, all_projects: bool) -> Option<String> {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    insert(&conn, "a", "the deployment pipeline runs nightly");
    insert(&conn, "b", "notes about invoices and billing");

    let pack = installed_pack()?;
    anathem::packstore::install(&conn, d.path(), &pack, anathem::packstore::Sidecar::Optional)
        .unwrap();
    let live = anathem::packstore::resolve(&conn, d.path());
    let ctx = search::SearchContext::for_server(
        live.pack.as_ref().zip(live.identity.as_ref()),
        1.0,
    );

    let mut args = json!({"query": query});
    if all_projects {
        args["all_projects"] = json!(true);
    }
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search_memory", "arguments": args}
    });
    let res = mcp::handle(&conn, &ctx, &Config::default(), &req).unwrap().unwrap();
    Some(res["result"]["content"][0]["text"].as_str().unwrap().to_string())
}

#[test]
fn a_hit_a_word_matched_is_not_labelled() {
    let Some(out) = answer("deployment", true) else { return };
    let line = out
        .lines()
        .find(|l| l.contains("deployment pipeline") && l.starts_with("[id:"))
        .unwrap_or_else(|| panic!("the lexical hit is missing:\n{out}"));
    assert!(
        !line.contains("[no word match]"),
        "a hit bm25 matched was labelled as if it had not:\n{line}"
    );
}

#[test]
fn a_hit_no_word_matched_says_so() {
    let Some(out) = answer("deployment", true) else { return };
    let line = out
        .lines()
        .find(|l| l.contains("invoices") && l.starts_with("[id:"))
        .unwrap_or_else(|| panic!("the vector-only hit is missing:\n{out}"));
    assert!(
        line.contains("[no word match]"),
        "a hit no word matched came back looking like a real one:\n{line}"
    );
}

/// The case the label exists for: nothing in the corpus has anything to do
/// with the question, and the answer should make that visible rather than
/// present its nearest neighbours as findings.
#[test]
fn a_query_that_matches_nothing_has_every_hit_labelled() {
    let Some(out) = answer("kubernetes autoscaling", true) else { return };
    let hits: Vec<&str> = out.lines().filter(|l| l.starts_with("[id:")).collect();
    assert!(!hits.is_empty(), "expected the vector ranking to answer anyway:\n{out}");
    for h in &hits {
        assert!(
            h.contains("[no word match]"),
            "a hit went unlabelled in an answer where nothing matched:\n{h}"
        );
    }
}

/// Labelling and not filtering. Reach is what the pack buys, and a memory
/// sharing no word with the query is sometimes the right one.
#[test]
fn the_label_does_not_remove_the_hit() {
    let Some(out) = answer("kubernetes autoscaling", true) else { return };
    assert!(out.contains("invoices"), "a labelled hit was dropped:\n{out}");
    assert!(out.contains("deployment"), "a labelled hit was dropped:\n{out}");
}
