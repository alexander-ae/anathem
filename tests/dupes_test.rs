//! Finding the same fact saved twice.
//!
//! The real case this was written against, found on the first run over a
//! 688-memory corpus: somebody saved a note, noticed the shell had eaten
//! part of it, saved it again — and never superseded the first. Both stayed
//! active and both kept turning up in searches. The second note even says
//! so in its own title.

use anathem::{db, dupes, model::NewMemory};

fn save(conn: &rusqlite::Connection, project: &str, ext: &str, body: &str) {
    anathem::ingest::insert(conn, &NewMemory {
        project: project.into(),
        title: body.chars().take(40).collect(),
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

fn conn() -> (tempfile::TempDir, rusqlite::Connection) {
    let d = tempfile::tempdir().unwrap();
    let c = db::open(&d.path().join("t.db")).unwrap();
    (d, c)
}

const LARGO: &str =
    "the quota middleware charged administrators for responses that never \
     reached them, because the after_action ran on every status including \
     the five hundreds";

#[test]
fn the_same_note_saved_twice_is_found() {
    let (_d, c) = conn();
    save(&c, "p", "a", LARGO);
    save(&c, "p", "b", LARGO);
    let pairs = dupes::find(&c).unwrap();
    assert_eq!(pairs.len(), 1, "expected exactly one pair: {pairs:?}");
    assert!(pairs[0].overlap > 0.9, "identical text should overlap almost fully: {pairs:?}");
}

/// What the check has to survive: a corpus is full of notes about the same
/// subject, and those are not duplicates.
#[test]
fn two_notes_on_one_subject_are_not_a_duplicate() {
    let (_d, c) = conn();
    save(&c, "p", "a", "the quota middleware charged for five hundreds, fixed in the after_action");
    save(&c, "p", "b", "the quota dashboard shows a monthly total per administrator and exports csv");
    assert!(dupes::find(&c).unwrap().is_empty(), "two different notes were called duplicates");
}

/// Two projects saying the same thing is two projects, not duplication.
/// Merging across them is what the scoping in `search_memory` exists to
/// prevent.
#[test]
fn the_same_text_in_two_projects_is_not_a_pair() {
    let (_d, c) = conn();
    save(&c, "uno", "a", LARGO);
    save(&c, "otro", "b", LARGO);
    assert!(dupes::find(&c).unwrap().is_empty(), "a pair was reported across projects");
}

/// A memory already retired is not a duplicate of anything: the whole point
/// of `forget` and `supersede` is that it stopped competing.
#[test]
fn a_retired_memory_is_not_counted() {
    let (_d, c) = conn();
    save(&c, "p", "a", LARGO);
    save(&c, "p", "b", LARGO);
    assert_eq!(dupes::find(&c).unwrap().len(), 1);
    c.execute("update memories set status = 'forgotten' where ext_id = 'b'", []).unwrap();
    assert!(dupes::find(&c).unwrap().is_empty(), "a forgotten memory still formed a pair");
}

/// Every pair appears once. Memories share many rare words, so the same two
/// are reached through several of them, and without the guard the report
/// would list one duplicate a dozen times.
#[test]
fn a_pair_is_reported_once_however_many_words_it_shares() {
    let (_d, c) = conn();
    save(&c, "p", "a", LARGO);
    save(&c, "p", "b", LARGO);
    let pairs = dupes::find(&c).unwrap();
    assert_eq!(pairs.len(), 1, "the same pair came back more than once: {pairs:?}");
}

/// An empty corpus is an answer, not an error.
#[test]
fn nothing_to_compare_is_not_a_failure() {
    let (_d, c) = conn();
    assert!(dupes::find(&c).unwrap().is_empty());
    save(&c, "p", "a", LARGO);
    assert!(dupes::find(&c).unwrap().is_empty());
}

// ------------------------------- what `mem doctor` does with the pairs

fn mem(dir: &std::path::Path, args: &[&str]) -> (bool, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("t.db"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr),
    )
}

/// The report says the line somebody can run, not the two ids and the
/// homework of working out which way round the command goes.
///
/// It is still only a report. Which of two overlapping notes is the keeper
/// needs judgement about what they are for, and the literature is explicit
/// that automating this is where these systems lose: an obsolete statement
/// left beside the current one is the dominant failure, and an automatic
/// merge is the other one.
#[test]
fn doctor_offers_the_command_instead_of_leaving_the_arithmetic() {
    let (d, c) = conn();
    save(&c, "p", "first", LARGO);
    save(&c, "p", "second", &format!("{LARGO} and it also skipped the retries"));
    // The second is a day newer, which is what decides the order of the
    // arguments — not the fact that it was inserted second.
    c.execute("update memories set created = '2026-11-07' where ext_id = 'second'", []).unwrap();
    drop(c);

    let (ok, out) = mem(d.path(), &["doctor"]);
    assert!(ok, "duplicating is not a failure, the exit code stays 0:\n{out}");
    assert!(out.contains("mem supersede 2 1"), "the ready-to-run line is missing:\n{out}");
    assert!(out.contains("if they are one fact"), "it must not sound like an order:\n{out}");
    assert!(out.contains("All good."), "{out}");
}

/// The order of the arguments follows the date the memory carries, not the
/// id. They usually agree; where they do not it is because the memories came
/// from somewhere else, and `mem import` assigns ids in whatever order the
/// file listed them — so an id comparison would confidently name the older
/// note as the correction.
#[test]
fn the_newer_one_goes_first_even_when_the_ids_disagree() {
    let (d, c) = conn();
    save(&c, "p", "imported-later", LARGO);
    save(&c, "p", "imported-first", &format!("{LARGO} and it also skipped the retries"));
    c.execute("update memories set created = '2026-10-02' where ext_id = 'imported-first'", [])
        .unwrap();
    c.execute("update memories set created = '2026-10-01' where ext_id = 'imported-later'", [])
        .unwrap();
    drop(c);

    let (ok, out) = mem(d.path(), &["doctor"]);
    assert!(ok, "{out}");
    assert!(out.contains("mem supersede 2 1"), "{out}");
}

/// Nothing is superseded by looking at the report.
#[test]
fn reporting_a_pair_changes_no_status() {
    let (d, c) = conn();
    save(&c, "p", "first", LARGO);
    save(&c, "p", "second", &format!("{LARGO} and it also skipped the retries"));
    drop(c);

    assert!(mem(d.path(), &["doctor"]).0);

    let c = db::open(&d.path().join("t.db")).unwrap();
    let active: i64 = c
        .query_row("select count(*) from memories where status = 'active'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(active, 2, "the report cannot have retired any of them");
}
