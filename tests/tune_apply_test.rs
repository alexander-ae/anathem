//! Writing the weight `mem tune` recommends, without taking anything else
//! with it.
//!
//! The risk here is not the number: it is the other seven keys. A naive
//! implementation serialises a `Config` and writes it out, which silently
//! turns every default into a setting somebody appears to have chosen — and
//! then a later change to a default never reaches that installation. These
//! tests exist to keep that from happening quietly.

use anathem::config::{load, write_bm25_weight, Wrote};

fn leer(dir: &std::path::Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(dir.join("config.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn with_no_file_it_creates_one_holding_only_that_key() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(write_bm25_weight(d.path(), 16.0).unwrap(), Wrote::Created);
    let v = leer(d.path());
    assert_eq!(v.as_object().unwrap().len(), 1, "it wrote more than it was asked: {v}");
    assert_eq!(v["bm25_weight"].as_f64(), Some(16.0));
}

/// The one that matters. Seven other keys, and they all have to be there
/// afterwards, with the values they had.
#[test]
fn every_other_key_survives() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.json"),
        r#"{ "candidates": 25, "excerpt": 150, "budget": 2000, "outline": true,
             "handoff_items": 3, "handoff_window": "6h", "release_pack_after": 0 }"#,
    )
    .unwrap();
    assert_eq!(write_bm25_weight(d.path(), 4.0).unwrap(), Wrote::KeyAdded);

    let v = leer(d.path());
    assert_eq!(v["candidates"].as_u64(), Some(25));
    assert_eq!(v["excerpt"].as_u64(), Some(150));
    assert_eq!(v["budget"].as_u64(), Some(2000));
    assert_eq!(v["outline"].as_bool(), Some(true));
    assert_eq!(v["handoff_items"].as_u64(), Some(3));
    assert_eq!(v["handoff_window"].as_str(), Some("6h"));
    assert_eq!(v["release_pack_after"].as_u64(), Some(0));
    assert_eq!(v["bm25_weight"].as_f64(), Some(4.0));

    // And the file still loads, which is the only thing the user cares about.
    let c = load(d.path()).unwrap();
    assert_eq!(c.bm25_weight, 4.0);
    assert_eq!(c.candidates, 25);
}

#[test]
fn replacing_a_value_reports_what_was_there() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{ "bm25_weight": 2.0 }"#).unwrap();
    assert_eq!(write_bm25_weight(d.path(), 8.0).unwrap(), Wrote::Replaced(2.0));
    assert_eq!(load(d.path()).unwrap().bm25_weight, 8.0);
}

/// A key this version does not know about is somebody's, or a newer
/// version's. Either way it is not ours to drop.
#[test]
fn a_key_this_version_does_not_know_is_kept() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.json"),
        r#"{ "something_from_the_future": [1, 2, 3] }"#,
    )
    .unwrap();
    write_bm25_weight(d.path(), 1.5).unwrap();
    let v = leer(d.path());
    assert_eq!(v["something_from_the_future"], serde_json::json!([1, 2, 3]));
}

/// Whoever has a broken `config.json` has something in it they meant.
/// Losing it to a tuning run would be a poor trade for saving one edit.
#[test]
fn a_file_that_does_not_parse_is_not_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let roto = r#"{ "candidates": 25, "#;
    std::fs::write(d.path().join("config.json"), roto).unwrap();

    let e = write_bm25_weight(d.path(), 16.0).unwrap_err();
    let msg = format!("{e:#}");
    assert!(msg.contains("config.json"), "the error has to name the file: {msg}");
    assert!(
        msg.contains("untouched") || msg.contains("left"),
        "the error has to say the file was not touched: {msg}"
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("config.json")).unwrap(),
        roto,
        "the broken file was overwritten"
    );
}

#[test]
fn a_json_that_is_not_an_object_is_refused() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), "[1, 2, 3]").unwrap();
    assert!(write_bm25_weight(d.path(), 16.0).is_err());
    assert_eq!(std::fs::read_to_string(d.path().join("config.json")).unwrap(), "[1, 2, 3]");
}

/// The grid extends past its ends, so the constants have to leave room to do
/// it: a ceiling below the last grid point would make the extension a no-op.
#[test]
fn the_extension_has_room_past_the_grid() {
    let last = *anathem::tune::WEIGHTS.last().unwrap();
    let first = anathem::tune::WEIGHTS[0];
    assert!(anathem::tune::MAX_WEIGHT > last, "nothing to extend into, upwards");
    assert!(anathem::tune::MIN_WEIGHT < first, "nothing to extend into, downwards");
}
