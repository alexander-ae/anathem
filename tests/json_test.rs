//! `--json`: the same answers in a shape a program can read.
//!
//! What is checked here is the contract `docs/integrations.md` publishes, not
//! the prose next to it. A document that says `written: false` means a
//! delivery landed earlier; one whose id is a string where the document says
//! integer is a caller's parser broken by an upgrade nobody warned about.
//!
//! Two things are asserted for every command and not just for one: stdout is
//! **one parsable line**, and the prose is gone from it. The second is the
//! one that rots quietly — a `println!` added to a branch later turns the
//! document into two and nothing notices until somebody pipes it.

use std::path::Path;
use std::process::Command;

fn mem(dir: &Path, args: &[&str]) -> (String, String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
        o.status.code().unwrap_or(-1),
    )
}

/// Runs a command that has to succeed and returns its document, insisting on
/// the two properties every one of them has: a zero exit, and one line on
/// stdout that parses.
fn json(dir: &Path, args: &[&str]) -> serde_json::Value {
    let mut full = args.to_vec();
    full.push("--json");
    let (out, err, code) = mem(dir, &full);
    assert_eq!(code, 0, "`mem {}` exited {code}: {err}", args.join(" "));
    assert_eq!(
        out.lines().count(),
        1,
        "`mem {}` printed {} lines, not one document:\n{out}",
        args.join(" "),
        out.lines().count()
    );
    let v: serde_json::Value = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("`mem {}` is not JSON ({e}):\n{out}", args.join(" ")));
    assert_eq!(v["schema"], 1, "`mem {}` speaks another schema:\n{out}", args.join(" "));
    v
}

#[test]
fn a_saved_memory_comes_back_with_its_id_project_and_tags() {
    let d = tempfile::tempdir().unwrap();
    let v = json(
        d.path(),
        &["save", "The payment provider is Adyen", "--project", "api", "--tag", "decision", "--topic", "payment-provider"],
    );
    assert!(v["id"].is_i64());
    assert_eq!(v["project"], "api");
    assert_eq!(v["tags"][0], "decision");
    assert_eq!(v["topic"], "payment-provider");
    assert_eq!(v["status"], "active");
    assert_eq!(v["replaced"], serde_json::Value::Null);
    assert_eq!(v["key_is_new"], true);
}

#[test]
fn search_returns_the_signals_a_caller_cannot_recompute() {
    let d = tempfile::tempdir().unwrap();
    let saved = json(d.path(), &["save", "The payment provider is Adyen", "--project", "api"]);
    let v = json(d.path(), &["search", "payment provider"]);
    assert_eq!(v["total_active"], 1);
    assert_eq!(v["scope_empty"], false);
    let hit = &v["results"][0];
    assert_eq!(hit["id"], saved["id"]);
    assert_eq!(hit["words_matched"], 2);
    assert_eq!(hit["words_total"], 2);
    assert!(hit["snippet"].as_str().unwrap().contains("Adyen"));
    // A lexical-only installation has no cosine to report, and null says
    // "not this retriever" rather than a zero somebody would average.
    assert_eq!(hit["cosine"], serde_json::Value::Null);
    assert!(hit["bm25"].is_number());
}

#[test]
fn an_empty_search_says_which_kind_of_empty_it_is() {
    let d = tempfile::tempdir().unwrap();
    json(d.path(), &["save", "One note", "--project", "api"]);
    let v = json(d.path(), &["search", "nothing like this"]);
    assert_eq!(v["results"].as_array().unwrap().len(), 0);
    assert_eq!(v["total_active"], 1);
    assert_eq!(v["scope_empty"], false);

    // A scope nothing is filed under is a different answer, and the prose
    // says so in a sentence a caller would have to parse.
    let v = json(d.path(), &["search", "payment", "--scope", "tag:nothing-has-this"]);
    assert_eq!(v["scope_empty"], true);
}

#[test]
fn a_correction_shows_the_chain_and_what_it_replaced() {
    let d = tempfile::tempdir().unwrap();
    let first = json(d.path(), &["save", "Adyen since October", "--project", "api", "--topic", "payment-provider"]);
    let second = json(d.path(), &["save", "Adyen since November", "--project", "api", "--topic", "payment-provider"]);
    assert_eq!(second["replaced"], first["id"]);

    let v = json(d.path(), &["show", "2"]);
    assert_eq!(v["supersedes"], first["id"]);
    assert_eq!(v["body"], "Adyen since November");

    let v = json(d.path(), &["show", "--topic", "payment-provider", "--project", "api"]);
    assert_eq!(v["found"], true);
    assert_eq!(v["current"]["id"], second["id"]);
    assert_eq!(v["history"].as_array().unwrap().len(), 2);
    assert_eq!(v["history"][0]["status"], "active");
    assert_eq!(v["history"][1]["status"], "superseded");

    let v = json(d.path(), &["topics"]);
    assert_eq!(v["topics"][0]["memories"], 2);
    assert_eq!(v["topics"][0]["current"], second["id"]);
}

#[test]
fn a_missing_topic_is_an_answer_and_not_an_error() {
    let d = tempfile::tempdir().unwrap();
    json(d.path(), &["save", "A note", "--project", "api", "--topic", "known"]);
    let v = json(d.path(), &["show", "--topic", "unknown", "--project", "api"]);
    assert_eq!(v["found"], false);
    assert_eq!(v["keys"][0], "known");
}

#[test]
fn an_external_delivery_says_written_false_when_it_is_a_retry() {
    let d = tempfile::tempdir().unwrap();
    // The body travels as the argument: `-` would read it from stdin, and
    // this test has none to give it.
    let args = ["save", "a note from elsewhere", "--external", "--from", "laptop", "--project", "api", "--ext-id", "abc"];
    let first = json(d.path(), &args);
    assert_eq!(first["written"], true);
    assert_eq!(first["status"], "draft");
    assert_eq!(first["from"], "laptop");

    let second = json(d.path(), &args);
    assert_eq!(second["written"], false);
    assert_eq!(second["already_received"], true);
    assert_eq!(second["same_sender"], true);
    assert_eq!(second["id"], first["id"]);
}

#[test]
fn an_accepted_draft_is_announced_by_id() {
    let d = tempfile::tempdir().unwrap();
    let draft = json(
        d.path(),
        &["save", "a note from elsewhere", "--external", "--from", "laptop", "--project", "api"],
    );
    let v = json(d.path(), &["accept", &draft["id"].to_string()]);
    assert_eq!(v["accepted"][0], draft["id"]);
    assert_eq!(v["from"], "laptop");

    let batch = json(
        d.path(),
        &["save", "another note", "--external", "--from", "phone", "--project", "api"],
    );
    assert_eq!(batch["status"], "draft");
    let v = json(d.path(), &["accept", "--all", "--from", "phone"]);
    assert_eq!(v["accepted_count"], 1);
}

#[test]
fn recent_and_log_answer_with_arrays_even_when_they_are_empty() {
    let d = tempfile::tempdir().unwrap();
    json(d.path(), &["save", "A note", "--project", "api"]);
    let v = json(d.path(), &["recent", "--all"]);
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["items"][0]["project"], "api");

    // The CLI never logs a search; only the MCP server does.
    let v = json(d.path(), &["log"]);
    assert_eq!(v["entries"].as_array().unwrap().len(), 0);
}

#[test]
fn stats_speaks_json_before_it_has_anything_to_say() {
    let d = tempfile::tempdir().unwrap();
    let v = json(d.path(), &["stats"]);
    assert_eq!(v["searches"], 0);
    assert_eq!(v["answer_rate"], 0.0);
    assert_eq!(v["median_rank"], serde_json::Value::Null);
    assert_eq!(v["by_week"].as_array().unwrap().len(), 0);
}

#[test]
fn stale_and_why_are_lists_and_not_prose() {
    let d = tempfile::tempdir().unwrap();
    let saved = json(d.path(), &["save", "A note nobody opened", "--project", "api"]);
    let v = json(d.path(), &["stale"]);
    assert_eq!(v["items"][0]["id"], saved["id"]);
    assert_eq!(v["items"][0]["last_accessed"], serde_json::Value::Null);

    let v = json(d.path(), &["why", &saved["id"].to_string()]);
    assert_eq!(v["appearances"].as_array().unwrap().len(), 0);
}

#[test]
fn a_body_full_of_control_characters_is_escaped_and_not_dropped() {
    let d = tempfile::tempdir().unwrap();
    let body = "before\u{1b}[2J after\r\nsecond line";
    let saved = json(d.path(), &["save", body, "--project", "api"]);
    let v = json(d.path(), &["show", &saved["id"].to_string()]);
    // The exact bytes travel: `--json` is machine output, and the sanitising
    // that protects a terminal has nothing to protect here. A newline inside
    // a string is escaped by JSON itself, which is what keeps the document
    // one line.
    assert_eq!(v["body"], body);
}

#[test]
fn a_failure_keeps_stdout_clean() {
    let d = tempfile::tempdir().unwrap();
    let (out, err, code) = mem(d.path(), &["show", "999", "--json"]);
    assert_ne!(code, 0);
    assert_eq!(out, "");
    assert!(err.contains("no memory with id:999"), "{err}");
}

#[test]
fn raw_changes_nothing_in_a_document() {
    // `--raw` exists to stop a terminal from interpreting control characters.
    // A JSON document is not a terminal, and the two flags together must not
    // produce either half of one.
    let d = tempfile::tempdir().unwrap();
    let saved = json(d.path(), &["save", "before\u{1b}[2J after", "--project", "api"]);
    let id = saved["id"].to_string();
    let (out, _, code) = mem(d.path(), &["show", &id, "--json", "--raw"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["body"], "before\u{1b}[2J after");
}

/// `mem doctor` keeps its exit code in JSON mode — 0 with no problems, even
/// with warnings — so a monitor can keep using the status it already watches
/// and read the detail from the document.
#[test]
fn doctor_reports_the_envelope_and_keeps_its_exit_code() {
    let d = tempfile::tempdir().unwrap();
    // No database yet: a warning, not a problem.
    let v = json(d.path(), &["doctor"]);
    assert_eq!(v["ok"], true);
    assert_eq!(v["problems"], 0);
    assert!(v["warnings"].as_u64().unwrap() >= 1);
    assert_eq!(v["database_exists"], false);
    assert!(v["version"].is_string());

    let titles: Vec<&str> =
        v["sections"].as_array().unwrap().iter().map(|s| s["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Binary"));
    assert!(titles.contains(&"Database"));

    // The counters are not decoration: they are the same numbers the prose
    // summary prints, and a caller that trusts them should be able to check
    // them against the lines.
    let lines: Vec<&serde_json::Value> = v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["lines"].as_array().unwrap())
        .collect();
    assert_eq!(v["levels"]["bad"], 0);
    assert_eq!(
        v["levels"]["good"].as_u64().unwrap()
            + v["levels"]["info"].as_u64().unwrap()
            + v["levels"]["warn"].as_u64().unwrap(),
        lines.len() as u64
    );
}

#[test]
fn doctor_exits_one_with_json_when_a_check_finds_a_problem() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("mem.db"), "this is not a database").unwrap();
    let (out, _, code) = mem(d.path(), &["doctor", "--json"]);
    assert_eq!(code, 1);
    assert_eq!(out.lines().count(), 1);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["ok"], false);
    assert!(v["problems"].as_u64().unwrap() >= 1);
    assert_eq!(v["levels"]["bad"], v["problems"]);
    let bad = v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["lines"].as_array().unwrap())
        .any(|l| l["level"] == "bad");
    assert!(bad, "a problem has to be a `bad` line:\n{out}");
}

/// A command without a document refuses the flag rather than printing prose
/// into a pipe that asked for JSON. Silence here is the expensive kind: the
/// caller gets a zero and nothing to parse, or parses somebody's next line.
#[test]
fn a_command_without_a_document_refuses_the_flag() {
    let d = tempfile::tempdir().unwrap();
    let (out, err, code) = mem(d.path(), &["init", "--json"]);
    assert_eq!(code, 1);
    assert_eq!(out, "");
    assert!(err.contains("no document for this command"), "{err}");
    assert!(err.contains("docs/integrations.md"), "{err}");
}
