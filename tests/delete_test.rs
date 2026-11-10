//! `mem delete`, which is a different promise from `mem forget`.
//!
//! `forget` sets a status. The memory leaves search and the body stays
//! readable by id, in `mem show` and in `mem export`. That is an audit
//! trail and it is the right default — and it is not an answer to somebody
//! who asks that something stop existing here. Until this command the only
//! answer was "delete the database".
//!
//! So the test that matters is the boring exhaustive one: after a delete,
//! the body is in none of the places that could show it, the index included,
//! and the bytes are not left legible in the file.

use std::path::Path;
use std::process::Command;

/// A word that appears nowhere else in this repository, so finding it in a
/// database file means finding this memory's body and nothing else.
const NEEDLE: &str = "zarpagliotti";

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

/// One memory holding the needle, and one that does not, so every assertion
/// below can tell "the delete worked" from "the database is empty".
fn one_to_delete() -> (tempfile::TempDir, i64) {
    let d = tempfile::tempdir().unwrap();
    let (ok, out) = mem(d.path(), &["init"]);
    assert!(ok, "{out}");
    let (ok, out) = mem(
        d.path(),
        &["save", &format!("the staging password was {NEEDLE} until we rotated it"),
          "--project", "secrets"],
    );
    assert!(ok, "{out}");
    let (ok, out) = mem(d.path(), &["save", "something else entirely", "--project", "secrets"]);
    assert!(ok, "{out}");
    (d, 1)
}

#[test]
fn the_body_is_gone_from_everywhere_that_could_show_it() {
    let (d, id) = one_to_delete();
    let i = id.to_string();

    // It is there first, or the rest of this test proves nothing.
    let (_, before) = mem(d.path(), &["search", NEEDLE]);
    assert!(before.contains(NEEDLE), "la memoria tenía que estar antes: {before}");

    let (ok, out) = mem(d.path(), &["delete", &i]);
    assert!(ok, "{out}");

    let (_, after) = mem(d.path(), &["search", NEEDLE]);
    assert!(!after.contains(NEEDLE), "sigue en search: {after}");

    let (ok, show) = mem(d.path(), &["show", &i]);
    assert!(!show.contains(NEEDLE), "sigue en `mem show`: {show}");
    assert!(!ok || show.contains("no memory") || show.contains("There is no"), "{show}");

    // The export is the other route the body used to leave by: `forget`
    // writes forgotten memories out with their bodies on purpose, and a
    // delete that left them exportable would be a forget with a louder name.
    let out_dir = d.path().join("exported");
    let (ok, out) = mem(d.path(), &["export", "--out", out_dir.to_str().unwrap()]);
    assert!(ok, "{out}");
    let mut found = Vec::new();
    for f in walk(&out_dir) {
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        if text.contains(NEEDLE) {
            found.push(f);
        }
    }
    assert!(found.is_empty(), "el export todavía lo trae: {found:?}");
    // And the export is not empty, so the assertion above is about the
    // delete and not about an export that wrote nothing.
    assert!(walk(&out_dir).len() >= 1, "el export no escribió nada");
}

#[test]
fn the_words_leave_the_index_and_the_file() {
    let (d, id) = one_to_delete();
    let (ok, out) = mem(d.path(), &["delete", &id.to_string()]);
    assert!(ok, "{out}");

    // The FTS index is its own store. `chunks` has `on delete cascade`, but
    // SQLite fires no triggers for a cascaded delete unless
    // `recursive_triggers` is on, and the trigger is what removes the terms:
    // getting this wrong leaves the body's words in the index while the row
    // is gone, which looks like a working delete from every other angle.
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let hits: i64 = conn
        .query_row(
            &format!("select count(*) from chunks_fts where chunks_fts match '\"{NEEDLE}\"'"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hits, 0, "la palabra sigue en el índice FTS");
    drop(conn);

    // And the bytes. SQLite does not promise to return the pages to the
    // filesystem, but `secure_delete` promises to overwrite them, and this
    // is the assertion somebody who asked for a delete actually cares
    // about.
    let raw = std::fs::read(d.path().join("mem.db")).unwrap();
    assert!(
        !contains(&raw, NEEDLE.as_bytes()),
        "el cuerpo sigue legible en el archivo de base de datos"
    );
}

#[test]
fn deleting_what_another_memory_supersedes_is_refused() {
    let d = tempfile::tempdir().unwrap();
    assert!(mem(d.path(), &["init"]).0);
    // Two versions of one topic: the second supersedes the first.
    assert!(mem(d.path(), &["save", "the payment provider is stripe",
                            "--project", "p", "--topic", "pay"]).0);
    assert!(mem(d.path(), &["save", "the payment provider is adyen",
                            "--project", "p", "--topic", "pay"]).0);

    let (ok, out) = mem(d.path(), &["delete", "1"]);
    assert!(!ok, "tenía que negarse: {out}");
    assert!(out.contains("superseded by 2"), "{out}");
    assert!(out.contains("--force"), "el error tiene que decir la salida: {out}");
    // And nothing was removed by the refusal.
    let (_, show) = mem(d.path(), &["show", "1"]);
    assert!(show.contains("stripe"), "la negativa no puede haber borrado nada: {show}");

    let (ok, out) = mem(d.path(), &["delete", "1", "--force"]);
    assert!(ok, "{out}");
    // `--force` prints the same warning rather than silencing it.
    assert!(out.contains("superseded by 2"), "{out}");
    // The memory that superseded it is still here: deleting more than was
    // asked is the other half of the failure this command exists to avoid.
    let (_, show) = mem(d.path(), &["show", "2"]);
    assert!(show.contains("adyen"), "{show}");
    let (ok, hist) = mem(d.path(), &["history", "2"]);
    assert!(ok, "{hist}");
    assert!(!hist.contains("stripe"), "{hist}");
}

#[test]
fn the_database_stays_healthy_after_a_delete() {
    let (d, id) = one_to_delete();
    assert!(mem(d.path(), &["delete", &id.to_string()]).0);

    let (ok, out) = mem(d.path(), &["doctor"]);
    assert!(ok, "doctor tenía que salir en 0:\n{out}");
    assert!(out.contains("All good."), "{out}");
    assert!(out.contains("no orphans"), "{out}");
    assert!(out.contains("FTS5 index in sync"), "{out}");
}

#[test]
fn it_says_what_it_cannot_reach_before_doing_it() {
    let (d, id) = one_to_delete();
    let (ok, out) = mem(d.path(), &["delete", &id.to_string()]);
    assert!(ok, "{out}");
    // The honest half: this deletes here, and a copy taken earlier is still
    // a copy.
    assert!(out.contains("export taken earlier"), "{out}");
    assert!(out.contains("nowhere else"), "{out}");
}

#[test]
fn forget_says_plainly_that_it_is_not_a_delete() {
    let (d, id) = one_to_delete();
    let i = id.to_string();
    let (ok, out) = mem(d.path(), &["forget", &i]);
    assert!(ok, "{out}");
    assert!(out.contains("not a delete"), "{out}");
    assert!(out.contains("mem delete"), "tiene que decir dónde está la otra: {out}");

    // And it keeps meaning what it meant: out of search, still readable.
    let (_, search) = mem(d.path(), &["search", NEEDLE]);
    assert!(!search.contains(NEEDLE), "{search}");
    let (_, show) = mem(d.path(), &["show", &i]);
    assert!(show.contains(NEEDLE), "`forget` no puede haber borrado el cuerpo: {show}");
}

#[test]
fn deleting_an_id_that_is_not_there_says_so() {
    let (d, _) = one_to_delete();
    let (ok, out) = mem(d.path(), &["delete", "999"]);
    assert!(!ok, "{out}");
    assert!(out.contains("999"), "{out}");
}

// --------------------------------------------------------------- helpers

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

