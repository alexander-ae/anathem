use anathem::{db, logsearch, secrets};
use std::path::Path;
use std::process::Command;

fn mem(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

#[test]
fn every_pattern_is_named_and_the_address_is_not_a_secret() {
    let hit = |t: &str| secrets::find(t).map(str::to_string);
    assert!(hit("-----BEGIN OPENSSH PRIVATE KEY-----").is_some());
    assert!(hit("AKIAIOSFODNN7EXAMPLE").is_some());
    assert!(hit("ghp_0123456789abcdefghijklmnopqrstuvwx").is_some());
    assert!(hit("glpat-0123456789abcdefghij").is_some());
    assert!(hit("xoxb-1234567890-abcdef").is_some());
    assert!(hit("sk-abcdefghijklmnopqrstuvwxyz0123").is_some());
    assert!(hit("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnop").is_some());
    assert!(hit("postgres://user:hunter2secret@host/db").is_some());
    assert!(hit("password: correcthorsebatterystaple").is_some());

    // The sentence the whole feature exists to keep savable.
    assert_eq!(hit("the deploy token is in ~/.config/deploy/env"), None);
    assert_eq!(hit("the token lives at src/config.rs"), None);
    assert_eq!(hit("a password is asked for at startup"), None);
}

#[test]
fn a_credential_is_refused_and_the_address_is_saved() {
    let d = tempfile::tempdir().unwrap();

    let (ok, _, err) = mem(d.path(), &["save", "glpat-0123456789abcdefghij", "--project", "p"]);
    assert!(!ok, "a token has to be refused");
    assert!(err.contains("GitLab token"), "the refusal names what it found: {err}");
    assert!(err.contains("Not saved"), "{err}");
    assert!(err.contains("where it lives"), "and what to do instead: {err}");

    let (ok, _, err) = mem(
        d.path(),
        &["save", "the deploy token is in ~/.config/deploy/env", "--project", "p"],
    );
    assert!(ok, "the address is the sentence we want: {err}");
}

#[test]
fn allow_secret_saves_anyway() {
    let d = tempfile::tempdir().unwrap();
    let (ok, out, err) = mem(
        d.path(),
        &["save", "glpat-0123456789abcdefghij", "--project", "p", "--allow-secret"],
    );
    assert!(ok, "{out} {err}");
    let (ok, out, _) = mem(d.path(), &["search", "glpat", "--project", "p"]);
    assert!(ok && out.contains("glpat"), "{out}");
}

#[test]
fn the_search_log_keeps_the_row_and_loses_the_value() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnop";
    logsearch::record(&conn, jwt, Some("p"), &[], 100, None, None).unwrap();
    logsearch::record(&conn, "cache breakdown", Some("p"), &[], 100, None, None).unwrap();

    let queries: Vec<String> = conn
        .prepare("select query from search_log order by id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(queries.len(), 2, "both rows survive: the counts are the point");
    assert!(queries[0].contains("[redacted"), "{}", queries[0]);
    assert!(!queries[0].contains("eyJhbGciOiJIUzI1NiJ9"), "{}", queries[0]);
    assert_eq!(queries[1], "cache breakdown", "an ordinary query is untouched");
}

#[test]
fn a_memory_that_arrived_from_elsewhere_cannot_be_pinned() {
    let d = tempfile::tempdir().unwrap();
    let (ok, out, err) = mem(
        d.path(),
        &["save", "the staging box is 10.0.0.4", "--project", "p",
          "--external", "--from", "claude@laptop"],
    );
    assert!(ok, "{out} {err}");
    let (ok, out, err) = mem(d.path(), &["accept", "1"]);
    assert!(ok, "{out} {err}");

    let (ok, _, err) = mem(d.path(), &["pin", "1"]);
    assert!(!ok, "pinning it would ride in every greeting");
    assert!(err.contains("claude@laptop"), "the refusal names the sender: {err}");

    let (ok, out, err) = mem(d.path(), &["pin", "1", "--force"]);
    assert!(ok, "the person can take responsibility: {out} {err}");
}

#[test]
fn taint_lists_what_arrived_and_doctor_names_the_pinned_one() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _, err) = mem(
        d.path(),
        &["save", "the staging box is 10.0.0.4", "--project", "p",
          "--external", "--from", "claude@laptop"],
    );
    assert!(ok, "{err}");

    let (ok, out, err) = mem(d.path(), &["taint"]);
    assert!(ok, "{err}");
    assert!(out.contains("claude@laptop") && out.contains("staging box"), "{out}");

    let (ok, _, err) = mem(d.path(), &["pin", "1", "--force"]);
    assert!(ok, "{err}");
    let (ok, out, err) = mem(d.path(), &["doctor"]);
    assert!(!ok, "a pinned memory from elsewhere is a problem");
    assert!(out.contains("pinned") || err.contains("pinned"), "{out}{err}");
}

#[test]
fn doctor_secrets_reports_ids_and_never_values() {
    let d = tempfile::tempdir().unwrap();
    let token = "glpat-0123456789abcdefghij";
    let (ok, _, err) = mem(
        d.path(),
        &["save", &format!("the token is {token}"), "--project", "p", "--allow-secret"],
    );
    assert!(ok, "{err}");

    let (ok, out, err) = mem(d.path(), &["doctor", "--secrets"]);
    assert!(!ok, "a credential in the database is a problem, so exit 1");
    let text = format!("{out}{err}");
    assert!(text.contains("carries a credential"), "{text}");
    assert!(text.contains("GitLab token"), "it names what it looks like: {text}");
    assert!(!text.contains(token), "and never prints the value: {text}");

    // Without the flag the scan does not run: doctor stays quick.
    let (ok, out, err) = mem(d.path(), &["doctor"]);
    assert!(ok || out.contains("pinned"), "{out}{err}");
    assert!(!format!("{out}{err}").contains("carry a credential"), "{out}{err}");
}
