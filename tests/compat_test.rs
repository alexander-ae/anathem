//! The four things a 1.0 would promise, each with the test that says it holds.
//!
//! Every other test here checks that a feature works. These check that
//! something written by an *earlier* version still works, which is a
//! different claim and the only one that matters to somebody deciding
//! whether to trust this with two years of notes.
//!
//! The fixtures are committed, and that is the whole point: from here on
//! they are not regenerated, so they keep being the shape 0.10.0 wrote while
//! the code moves on. A fixture is replaced only when a release deliberately
//! drops support for opening the old one, which is a major.
//!
//! **Today they prove less than they will.** They were written by the
//! version that is reading them, so right now the first test says "a
//! database opens" and not yet "a database from two schemas ago opens". Its
//! value starts at the next change to the schema or to the export format,
//! and the genuinely older shapes are already covered one migration at a
//! time by `topic_test::older_database` and by
//! `recent_test::a_database_predating_these_columns_keeps_working_and_migrates_itself`.
//! What is new here is that the check stops being per-feature and becomes a
//! standing promise.
//!
//! What these do NOT check is the other direction: that an older binary can
//! read what this one writes. Nothing promises that and nothing should.

use std::path::Path;
use std::process::Command;

fn mem(dir: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr),
    )
}

/// Builds a database from the committed SQL dump, the way an older version
/// left it. `sqlite3` is not assumed to be installed: the statements go in
/// through the same library the program uses.
fn database_from_fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let sql = std::fs::read_to_string("tests/fixtures/db-1.0.sql").unwrap();
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    conn.execute_batch(&sql).unwrap();
    drop(conn);
    d
}

// ----------------------------------------------- 1. the database opens

/// A database an earlier version wrote opens, migrates itself, and comes out
/// of `mem doctor` with nothing broken.
///
/// The assertion is "no `✗` and exit 0" rather than the exact closing line:
/// `doctor` ends in "All good." or in "No problems (N warnings)." depending
/// on things that are not about compatibility at all — here, the permissions
/// of a file in a temporary directory.
#[test]
fn a_database_from_an_earlier_version_opens_and_stays_healthy() {
    let d = database_from_fixture();

    let (ok, out) = mem(d.path(), &["doctor"]);
    assert!(ok, "doctor falló sobre una base anterior:\n{out}");
    assert!(!out.contains('✗'), "doctor encontró algo roto:\n{out}");

    // The memories are all there, with the statuses they had. A migration
    // that silently dropped the drafts would also pass a health check.
    assert!(out.contains("3 active, 1 superseded, 1 draft"), "{out}");

    // And they are findable, which is the part a health check does not
    // cover: the FTS index has to have survived too.
    let (_, found) = mem(d.path(), &["search", "payment provider"]);
    assert!(found.contains("Adyen"), "la memoria vigente no aparece:\n{found}");
    assert!(!found.contains("Stripe"), "la superseded no puede volver a buscar:\n{found}");

    // The supersession chain still walks backwards past the hole.
    let (ok, hist) = mem(d.path(), &["history", "2"]);
    assert!(ok, "{hist}");
    assert!(hist.contains("Stripe"), "la cadena se perdió:\n{hist}");

    // Accents survived whatever the migration did to the index.
    let (_, enie) = mem(d.path(), &["search", "acentos"]);
    assert!(enie.contains("unicode"), "{enie}");
}

// ------------------------------------------------- 2. the export imports

/// An export an earlier version wrote imports with nothing lost.
///
/// This is the escape hatch, and it is the reason a schema change is
/// survivable at all: whatever happens to the database format, an export
/// taken before it has to still come back.
#[test]
fn an_export_from_an_earlier_version_imports_with_nothing_lost() {
    let d = tempfile::tempdir().unwrap();
    let (ok, out) = mem(d.path(), &["init"]);
    assert!(ok, "{out}");

    let (ok, out) = mem(
        d.path(),
        &["import", "--from", "tests/fixtures/export-1.0"],
    );
    assert!(ok, "la importación falló:\n{out}");

    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };

    assert_eq!(count("select count(*) from memories"), 5, "falta alguna memoria");
    assert_eq!(
        count("select count(*) from memories where status = 'active'"),
        3,
        "los estados no sobrevivieron"
    );
    assert_eq!(
        count("select count(*) from memories where status = 'superseded'"),
        1,
        "la superseded volvió como activa, que es peor que perderla"
    );
    assert_eq!(
        count("select count(*) from memories where status = 'draft'"),
        1,
        "el borrador recibido volvió aceptado"
    );
    // The things that are easy to drop in a round trip because they live
    // outside `memories.body`.
    assert_eq!(
        count("select count(*) from memories where topic = 'payment-provider'"),
        2,
        "la clave de tema se perdió"
    );
    assert!(count("select count(*) from memory_tags") >= 3, "se perdieron etiquetas");
    assert_eq!(
        count("select count(*) from memories where received_from is not null"),
        1,
        "la procedencia se perdió: una memoria recibida volvió como propia"
    );
    // And the supersession pointer, which is an id and therefore has to be
    // remapped rather than copied.
    assert_eq!(
        count("select count(*) from memories where supersedes_id is not null"),
        1,
        "la cadena de supersesión no se remapeó"
    );
    let dangling: i64 = conn
        .query_row(
            "select count(*) from memories m where m.supersedes_id is not null
               and not exists (select 1 from memories o where o.id = m.supersedes_id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0, "quedó un supersedes_id apuntando a nada");
    drop(conn);

    // Searchable after the import, not merely stored: the chunks and the
    // index are rebuilt on the way in.
    let (_, found) = mem(d.path(), &["search", "migration before the code"]);
    assert!(found.contains("migration"), "lo importado no se busca:\n{found}");
}

// --------------------------------------------------- 3. the five tools

/// The five tools keep their names and their required parameters.
///
/// Renaming one, or making an existing parameter required, breaks every
/// agent configuration that exists. Adding an *optional* parameter does not,
/// and stays allowed — which is what `since` and `until` did in 0.10.0.
#[test]
fn the_five_tools_keep_their_names_and_required_parameters() {
    let d = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&d.path().join("mem.db")).unwrap();
    let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let res = anathem::mcp::handle(
        &conn,
        &anathem::search::SearchContext::bm25_only(),
        &anathem::config::Config::default(),
        &req,
    )
    .unwrap()
    .unwrap();

    let mut got: Vec<(String, Vec<String>)> = res["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            let name = t["name"].as_str().unwrap().to_string();
            let mut required: Vec<String> = t["inputSchema"]["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            required.sort();
            (name, required)
        })
        .collect();
    got.sort();

    let expected: Vec<(String, Vec<String>)> = vec![
        ("get_memory", vec!["id"]),
        ("list_tags", vec![]),
        ("recent_memories", vec![]),
        ("save_memory", vec!["text"]),
        ("search_memory", vec!["query"]),
    ]
    .into_iter()
    .map(|(n, r)| (n.to_string(), r.into_iter().map(str::to_string).collect()))
    .collect();

    assert_eq!(
        got, expected,
        "la superficie MCP cambió: cualquier agente configurado contra la anterior se rompe"
    );
}

// -------------------------------------- 4. what the README documents runs

/// Every `mem <command>` the README shows exists, and every flag it shows
/// alongside one is still accepted by it.
///
/// `docs_english_test` guards the opposite direction — a document teaching
/// something that was removed — by keeping a list of what went away. This is
/// the same check without a list: it asks the binary.
#[test]
fn every_command_and_flag_the_readme_documents_still_runs() {
    let readme = std::fs::read_to_string("README.md").unwrap();
    let d = tempfile::tempdir().unwrap();

    let mut checked = 0;
    for span in readme.split('`').skip(1).step_by(2) {
        let span = span.trim();
        let Some(rest) = span.strip_prefix("mem ") else { continue };
        let mut words = rest.split_whitespace();
        // `mem --db ... init`: the subcommand is the first word that is not
        // a flag, and not the value of the flag before it.
        let mut command = None;
        let mut skip_next = false;
        let mut flags: Vec<&str> = Vec::new();
        for w in words.by_ref() {
            if skip_next {
                skip_next = false;
                continue;
            }
            if let Some(flag) = w.strip_prefix("--") {
                let flag = flag.split('=').next().unwrap();
                if command.is_none() {
                    // A global flag such as `--db`, which takes a value.
                    skip_next = true;
                } else {
                    flags.push(flag);
                }
                continue;
            }
            if w.starts_with('-') {
                continue;
            }
            if command.is_none() {
                command = Some(w);
            }
        }
        let Some(command) = command else { continue };
        // `mem --version` and friends leave no subcommand; prose inside a
        // code span would not survive `--help` either.
        if !command.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            continue;
        }

        let (ok, help) = mem(d.path(), &[command, "--help"]);
        assert!(ok, "el README documenta `mem {command}`, que ya no existe:\n{help}");
        checked += 1;

        for flag in flags {
            // Flags of a flag's value (`--scope project:api`) are already
            // filtered; what is left is a real flag of this subcommand.
            assert!(
                help.contains(&format!("--{flag}")),
                "el README muestra `mem {command} --{flag}`, que ya no acepta"
            );
        }
    }

    // A parser that quietly matched nothing would pass this test forever.
    assert!(checked >= 10, "solo se comprobaron {checked} comandos del README");
}
