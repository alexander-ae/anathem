//! What someone installing anathem for the first time runs into.
//! These tests invoke the real binary: if the first-day experience breaks,
//! they fail even with the library in perfect shape.

use std::process::Command;

fn mem(db: &std::path::Path, args: &[&str]) -> (String, String, bool) {
    let out_text = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&out_text.stdout).to_string(),
        String::from_utf8_lossy(&out_text.stderr).to_string(),
        out_text.status.success(),
    )
}

/// Searching an empty database has to SAY SO. It used to print nothing,
/// which was indistinguishable from a broken binary.
#[test]
fn a_search_with_no_results_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    mem(&db, &["init"]);
    let (out, _, ok) = mem(&db, &["search", "cualquier cosa"]);
    assert!(ok, "search should exit 0 even with no results");
    assert!(!out.trim().is_empty(), "search printed nothing; the user cannot tell if it worked");
}

/// `eval` without an eval set has to explain what one is and how to build
/// it, not return a file-not-found error.
#[test]
fn eval_without_a_set_explains_what_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    mem(&db, &["init"]);
    let (out, err, _) = mem(&db, &["eval"]);
    let todo = format!("{out}{err}");
    assert!(todo.contains("evalset") || todo.contains("eval set"),
        "el mensaje no nombra el archivo que falta: {todo}");
    assert!(todo.to_lowercase().contains("consulta") || todo.contains("expect_docs"),
        "el mensaje no explica qué es un eval set: {todo}");
}

/// The minimal first-day cycle: save something and find it.
#[test]
fn the_minimal_cycle_works_with_nothing_configured() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    mem(&db, &["init"]);
    let (out, _, ok) = mem(&db, &["save", "Se decidió usar SQLite por un bug de concurrencia"]);
    assert!(ok && out.contains("saved"), "it did not save: {out}");
    assert!(out.contains('['), "it did not classify with the default rules: {out}");
    let (out, _, _) = mem(&db, &["search", "SQLite concurrencia"]);
    assert!(out.contains("SQLite"), "it did not find what it had just saved: {out}");
}

#[test]
fn the_search_budget_can_be_raised() {
    // Without this there is no way to inspect more candidates than fit in an
    // agent's context, which is exactly what is needed to measure whether
    // returning more and shorter ones pays off.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    let mem = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
            .args(["--db", db.to_str().unwrap()]).args(args).output().unwrap()
    };
    for i in 0..8 {
        let body = format!("memoria numero {i} sobre cacheo de propiedades y {}", "relleno ".repeat(60));
        mem(&["save", &body, "--project", "p"]);
    }
    let tight = mem(&["search", "cacheo propiedades", "--limit", "8"]);
    let roomy = mem(&["search", "cacheo propiedades", "--limit", "8", "--budget", "100000"]);
    let n = |o: &std::process::Output| String::from_utf8_lossy(&o.stdout)
        .lines().filter(|l| l.trim_start().starts_with(char::is_numeric) && l.contains('.')).count();
    assert!(n(&roomy) > n(&tight),
        "con más presupuesto tienen que entrar más resultados: {} vs {}", n(&roomy), n(&tight));
}
