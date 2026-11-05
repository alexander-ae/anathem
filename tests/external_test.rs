//! Memories received from another agent or another machine.
//!
//! The transport is SSH and the gate is the one already there: whoever can
//! run `mem save` on this host can run anything on it. What this adds is
//! **provenance**, and the thing provenance is for — a memory that arrived
//! from elsewhere is text another model wrote, possibly while summarising a
//! page it read, and it would otherwise be injected into an agent's context
//! on every matching search without anyone having seen it.
//!
//! So it arrives as a draft, which is invisible to search, and a person
//! promotes it. The lifecycle already had the room: `status` has allowed
//! `draft` since the first schema and nothing ever wrote it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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

fn mem_ok(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = mem(dir, args);
    assert!(ok, "mem {args:?} falló: {err}");
    out
}

fn mem_stdin(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

fn status_of(dir: &Path, id: i64) -> String {
    let conn = rusqlite::Connection::open(dir.join("mem.db")).unwrap();
    conn.query_row("select status from memories where id = ?1", [id], |r| r.get(0)).unwrap()
}

const NOTA: &str = "El servidor dell expone nginx en el 8443 y no en el 443";
const REMITENTE: &str = "claude-code@alexander-lenovo";

#[test]
fn an_external_memory_arrives_as_draft_and_search_does_not_see_it() {
    let d = tempfile::tempdir().unwrap();
    let out = mem_ok(
        d.path(),
        &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"],
    );
    assert!(out.contains("draft") || out.contains("accept"), "tiene que decir que no está activa: {out}");
    assert_eq!(status_of(d.path(), 1), "draft");

    let buscar = mem_ok(d.path(), &["search", "nginx", "--project", "dell"]);
    assert!(
        !buscar.contains("8443"),
        "una memoria que nadie aceptó no puede salir en una búsqueda: {buscar}"
    );
}

#[test]
fn external_without_a_project_is_refused() {
    // Deriving it from the receiver's working directory would file someone
    // else's note under whatever repository you happen to be standing in.
    let d = tempfile::tempdir().unwrap();
    let (ok, _out, err) = mem(d.path(), &["save", NOTA, "--external", "--from", REMITENTE]);
    assert!(!ok, "sin --project no se puede saber dónde va");
    assert!(err.contains("--project"), "{err}");
}

#[test]
fn external_without_a_sender_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _out, err) = mem(d.path(), &["save", NOTA, "--external", "--project", "dell"]);
    assert!(!ok, "una memoria externa sin remitente no tiene procedencia, que es todo el punto");
    assert!(err.contains("--from"), "{err}");
}

#[test]
fn external_with_a_topic_is_refused_and_explains_why() {
    // A topic key means "this replaces the current version of that subject".
    // Letting a memory from another machine do that to a local one needs a
    // rule about which of the two is current, and that rule costs the unique
    // index the whole topic feature rests on.
    let d = tempfile::tempdir().unwrap();
    let (ok, _out, err) = mem(
        d.path(),
        &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell", "--topic", "nginx"],
    );
    assert!(!ok);
    assert!(err.contains("--topic") && err.contains("--external"), "{err}");
}

#[test]
fn the_body_can_arrive_on_stdin_so_it_pipes_over_ssh() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _out, err) = mem_stdin(
        d.path(),
        &["save", "-", "--external", "--from", REMITENTE, "--project", "dell"],
        NOTA,
    );
    assert!(ok, "{err}");
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let body: String =
        conn.query_row("select body from memories where id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(body.trim(), NOTA);
}

#[test]
fn accepting_one_makes_it_searchable() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);

    let out = mem_ok(d.path(), &["accept", "1"]);
    assert!(out.contains('1'), "{out}");
    assert_eq!(status_of(d.path(), 1), "active");

    let buscar = mem_ok(d.path(), &["search", "nginx", "--project", "dell"]);
    assert!(buscar.contains("8443"), "aceptada, ya tiene que salir: {buscar}");
}

#[test]
fn accepting_something_that_is_not_waiting_fails_instead_of_saying_nothing() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "una nota local", "--project", "dell"]);
    let (ok, _out, err) = mem(d.path(), &["accept", "1"]);
    assert!(!ok, "aceptar algo que ya está activo no es una operación con sentido");
    assert!(err.contains("active") || err.contains("draft"), "{err}");

    let (ok, _out, err) = mem(d.path(), &["accept", "99"]);
    assert!(!ok);
    assert!(err.contains("99"), "{err}");
}

#[test]
fn show_by_id_prints_a_draft_so_it_can_be_read_before_accepting() {
    // Reviewing something you cannot read is not reviewing it. Until now the
    // CLI could print a topic but not a memory.
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);

    let out = mem_ok(d.path(), &["show", "1"]);
    assert!(out.contains(NOTA), "el cuerpo entero: {out}");
    assert!(out.contains(REMITENTE), "y de dónde vino: {out}");
    assert!(out.contains("draft"), "y que está esperando: {out}");
}

#[test]
fn show_by_id_on_a_memory_that_does_not_exist_says_so() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _out, err) = mem(d.path(), &["show", "42"]);
    assert!(!ok);
    assert!(err.contains("42"), "{err}");
}

#[test]
fn an_accepted_memory_still_says_where_it_came_from() {
    // The mark is the point. An agent reading a result has to be able to
    // tell what this memory was told from what it observed.
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);
    mem_ok(d.path(), &["accept", "1"]);

    let out = mem_ok(d.path(), &["search", "nginx", "--project", "dell"]);
    assert!(out.contains(REMITENTE), "la búsqueda tiene que marcarla: {out}");
}

#[test]
fn the_mcp_output_marks_a_received_memory_too() {
    use anathem::{config::Config, db, ingest, mcp, search};
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let g = ingest::save_received(&conn, NOTA, "dell", Vec::new(), REMITENTE).unwrap();
    anathem::maint::accept(&conn, g.id).unwrap();

    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"nginx"}}});
    let r = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains(REMITENTE), "el modelo tiene que saber que se lo contaron: {text}");
}

#[test]
fn export_and_import_keep_the_provenance() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);

    let out = d.path().join("export");
    mem_ok(d.path(), &["export", "--out", out.to_str().unwrap()]);
    // `export` writes one directory per project, so this walks one level in.
    let mut escrito = String::new();
    for e in std::fs::read_dir(&out).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            for f in std::fs::read_dir(&p).unwrap() {
                escrito.push_str(&std::fs::read_to_string(f.unwrap().path()).unwrap());
            }
        } else {
            escrito.push_str(&std::fs::read_to_string(&p).unwrap());
        }
    }
    assert!(escrito.contains(REMITENTE), "el frontmatter tiene que llevarlo: {escrito}");

    let otro = tempfile::tempdir().unwrap();
    mem_ok(otro.path(), &["import", "--from", out.to_str().unwrap()]);
    let conn = rusqlite::Connection::open(otro.path().join("mem.db")).unwrap();
    let (from, status): (Option<String>, String) = conn
        .query_row("select received_from, status from memories where id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(from.as_deref(), Some(REMITENTE), "la procedencia no puede perderse al migrar");
    assert_eq!(status, "draft", "y tampoco el hecho de que estaba esperando");
}

#[test]
fn recent_external_lists_only_what_arrived_including_drafts() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "una nota propia sobre nginx", "--project", "dell"]);
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);

    let out = mem_ok(d.path(), &["recent", "--external", "--project", "dell"]);
    assert!(out.contains(REMITENTE), "tiene que listar la recibida: {out}");
    assert!(
        !out.contains("una nota propia"),
        "y solo las recibidas, que es para lo que sirve la bandera: {out}"
    );
}

#[test]
fn accept_all_from_one_sender_promotes_that_batch_and_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);
    mem_ok(d.path(), &["save", "otra cosa", "--external", "--from", REMITENTE, "--project", "dell"]);
    mem_ok(d.path(), &["save", "de otro lado", "--external", "--from", "otro@host", "--project", "dell"]);

    let out = mem_ok(d.path(), &["accept", "--all", "--from", REMITENTE]);
    assert!(out.contains('2'), "tiene que decir cuántas aceptó: {out}");
    assert_eq!(status_of(d.path(), 1), "active");
    assert_eq!(status_of(d.path(), 2), "active");
    assert_eq!(status_of(d.path(), 3), "draft", "las de otro remitente no se tocan");
}

#[test]
fn accept_all_without_a_sender_is_refused() {
    // Accepting everything waiting, whatever its origin, is exactly the
    // decision this gate exists to make someone take on purpose.
    let d = tempfile::tempdir().unwrap();
    let (ok, _out, err) = mem(d.path(), &["accept", "--all"]);
    assert!(!ok);
    assert!(err.contains("--from"), "{err}");
}

#[test]
fn doctor_says_how_many_are_waiting_to_be_accepted() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", NOTA, "--external", "--from", REMITENTE, "--project", "dell"]);

    let (ok, out, _) = mem(d.path(), &["doctor"]);
    assert!(ok, "una memoria esperando no es un problema, es información");
    assert!(out.contains("accept"), "tiene que mencionar que hay algo esperando: {out}");
    assert!(out.contains('1'), "{out}");
}
