//! Topic key: saving under the same key supersedes the previous active
//! memory of the same project.

use anathem::{config::Config, db, ingest, mcp, search};
use std::path::Path;
use std::process::Command;

/// Runs `mem` against the database in `dir`. Returns (success, stdout, stderr).
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

fn status_(dir: &Path, id: i64) -> String {
    let conn = rusqlite::Connection::open(dir.join("mem.db")).unwrap();
    conn.query_row("select status from memories where id = ?1", [id], |r| r.get(0)).unwrap()
}

const PAGOS_VIEJO: &str = "El proveedor de pagos es Stripe";
const PAGOS_NUEVO: &str = "El proveedor de pagos es Adyen desde octubre";

#[test]
fn saving_twice_under_one_key_leaves_the_first_superseded_and_the_second_active() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    assert_eq!(status_(d.path(), 1), "superseded");
    assert_eq!(status_(d.path(), 2), "active");
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let sup: i64 = conn
        .query_row("select supersedes_id from memories where id = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sup, 1, "la nueva tiene que apuntar a la que reemplazó");
}

#[test]
fn the_older_one_stops_showing_in_search_and_the_newer_one_does() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    let out = mem_ok(d.path(), &["search", "proveedor pagos"]);
    assert!(out.contains("Adyen"), "la nueva tiene que aparecer: {out}");
    assert!(!out.contains("Stripe"), "la vieja sigue apareciendo: {out}");
}

#[test]
fn history_on_the_newer_one_shows_the_chain_back_to_the_older() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", "El proveedor de pagos es Adyen", "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    let out = mem_ok(d.path(), &["history", "3"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "la cadena tiene tres eslabones: {out}");
    assert!(lines[0].contains("active") && lines[0].contains("desde octubre"), "{out}");
    assert!(lines[1].contains("superseded") && lines[1].contains("Adyen"), "{out}");
    assert!(lines[2].contains("superseded") && lines[2].contains("Stripe"), "{out}");
}

#[test]
fn the_same_key_in_two_projects_does_not_collide() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "La base de a es Postgres", "--project", "a", "--topic", "base-de-datos"]);
    mem_ok(d.path(), &["save", "La base de b es MySQL", "--project", "b", "--topic", "base-de-datos"]);

    assert_eq!(status_(d.path(), 1), "active");
    assert_eq!(status_(d.path(), 2), "active");
}

#[test]
fn without_a_topic_key_saving_twice_leaves_both_active() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p"]);

    assert_eq!(status_(d.path(), 1), "active");
    assert_eq!(status_(d.path(), 2), "active");
    // and with no key there is nothing left to list
    assert!(mem_ok(d.path(), &["topics"]).contains("No topic keys"));
}

#[test]
fn saving_under_a_new_key_simply_saves() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "Otro hecho", "--project", "p", "--topic", "otro"]);
    let out = mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);

    assert_eq!(status_(d.path(), 1), "active", "una clave distinta no toca a las demás");
    assert_eq!(status_(d.path(), 2), "active");
    assert!(out.contains("saved memory 2"), "{out}");
    assert!(out.contains("pagos") && out.contains("new one"), "tiene que decir que la clave era nueva: {out}");
}

#[test]
fn on_replacing_the_output_names_the_superseded_memory_by_id() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    let out = mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    assert!(out.contains("saved memory 2"), "{out}");
    assert!(out.contains("replaces memory 1"), "tiene que nombrar el id reemplazado: {out}");
    assert!(out.contains("Stripe"), "tiene que mostrar el título reemplazado: {out}");
}

#[test]
fn topics_lists_project_count_and_current() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", "La base es Postgres", "--project", "q", "--topic", "base"]);
    mem_ok(d.path(), &["save", "Sin clave, no figura", "--project", "p"]);

    let out = mem_ok(d.path(), &["topics"]);
    let row = |key: &str| -> Vec<String> {
        out.lines()
            .find(|l| l.split_whitespace().nth(1) == Some(key))
            .unwrap_or_else(|| panic!("falta la clave {key} en:\n{out}"))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    assert_eq!(row("pagos"), ["p", "pagos", "2", "2"], "{out}");
    assert_eq!(row("base"), ["q", "base", "1", "3"], "{out}");
    assert_eq!(out.lines().count(), 3, "cabecera y dos claves, nada más: {out}");
}

#[test]
fn topics_says_there_is_no_current_one_when_the_only_one_was_forgotten() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["forget", "1"]);

    let out = mem_ok(d.path(), &["topics"]);
    assert!(out.contains("pagos") && out.contains("none"), "{out}");
}

#[test]
fn once_the_current_one_is_forgotten_the_key_saves_again_without_superseding() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["forget", "1"]);
    let out = mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    assert!(!out.contains("replaces memory"), "no había una activa que reemplazar: {out}");
    assert_eq!(status_(d.path(), 1), "forgotten", "no se revive lo olvidado");
    assert_eq!(status_(d.path(), 2), "active");
}

#[test]
fn an_empty_key_is_rejected_and_nothing_is_saved() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _, err) = mem(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "  "]);
    assert!(!ok, "una clave vacía tiene que fallar");
    assert!(err.contains("topic key"), "{err}");
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let n: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn the_key_is_trimmed_so_surrounding_spaces_mean_the_same_topic() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", " pagos "]);
    assert_eq!(status_(d.path(), 1), "superseded");
}

#[test]
fn case_and_inner_spaces_do_not_make_another_topic() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "proveedor de pagos"]);
    let out = mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "  Proveedor   DE\tPagos "]);

    assert_eq!(status_(d.path(), 1), "superseded", "{out}");
    assert!(out.contains("«proveedor de pagos»"), "muestra la clave normalizada: {out}");
    let topics = mem_ok(d.path(), &["topics"]);
    assert!(topics.contains("proveedor de pagos") && !topics.contains("Proveedor"), "{topics}");
}

#[test]
fn a_new_key_lists_the_ones_already_in_the_project() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "El stack es React", "--project", "p", "--topic", "stack-del-frontend"]);
    mem_ok(d.path(), &["save", "Otra cosa", "--project", "p", "--topic", "politica-de-ramas"]);
    mem_ok(d.path(), &["save", "De otro proyecto", "--project", "q", "--topic", "solo-en-q"]);
    // `stack-frontend` does not match `stack-del-frontend`: without the
    // list, the message would be identical to a legitimately new key's.
    let out = mem_ok(d.path(), &["save", "El stack es Svelte", "--project", "p", "--topic", "stack-frontend"]);

    assert!(out.contains("stack-del-frontend") && out.contains("politica-de-ramas"), "{out}");
    assert!(!out.contains("solo-en-q"), "solo las del mismo proyecto: {out}");
    assert_eq!(status_(d.path(), 1), "active");
}

#[test]
fn when_there_are_no_other_keys_the_message_says_so() {
    let d = tempfile::tempdir().unwrap();
    let out = mem_ok(d.path(), &["save", "Primera", "--project", "p", "--topic", "unica"]);
    assert!(out.contains("no other topic keys"), "{out}");
}

#[test]
fn on_superseding_no_other_keys_are_listed() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "Uno", "--project", "p", "--topic", "a"]);
    mem_ok(d.path(), &["save", "Dos", "--project", "p", "--topic", "b"]);
    let out = mem_ok(d.path(), &["save", "Dos bis", "--project", "p", "--topic", "b"]);
    assert!(out.contains("replaces memory 2"), "{out}");
    assert!(!out.contains("Keys that already exist"), "{out}");
}

// -- the vector layer -------------------------------------------------------

#[test]
fn vector_search_does_not_return_the_superseded_memory_either() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let older = ingest::save_with_topic(&conn, PAGOS_VIEJO, "p", vec![], Some("pagos"), None).unwrap();
    let newer = ingest::save_with_topic(&conn, PAGOS_NUEVO, "p", vec![], Some("pagos"), None).unwrap();

    // The same vector for both memories' chunks: if one leaks through, it is
    // because of its status and not its closeness.
    let blob: Vec<u8> = [1.0f32, 0.0].iter().flat_map(|f| f.to_le_bytes()).collect();
    for id in [older.id, newer.id] {
        conn.execute(
            "insert into chunk_vectors (chunk_id, vec)
             select id, ?2 from chunks where memory_id = ?1",
            rusqlite::params![id, blob],
        )
        .unwrap();
    }

    let q = search::Query { text: "pagos".into(), ..Default::default() };
    let hits = anathem::vecstore::search_vectors(&conn, &[1.0, 0.0], &q, 10).unwrap();
    let ids: Vec<i64> = hits.iter().map(|h| h.memory_id).collect();
    assert_eq!(ids, vec![newer.id], "solo la vigente puede salir por vector: {ids:?}");
}

// -- the save happens in a single transaction --------------------------------

#[test]
fn when_the_link_fails_the_new_memory_is_not_left_half_written() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let primera = ingest::save_with_topic(&conn, PAGOS_VIEJO, "p", vec![], Some("pagos"), None).unwrap();
    // A trigger aborting the supersede stands in for any failure of the link.
    conn.execute_batch(
        "create trigger no_superseder before update of status on memories
         when new.status = 'superseded' begin select raise(abort, 'falla simulada'); end;",
    )
    .unwrap();

    let err = ingest::save_with_topic(&conn, PAGOS_NUEVO, "p", vec![], Some("pagos"), None);
    assert!(err.is_err(), "la falla del enlace tiene que propagarse");

    let n: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "el insert se tiene que deshacer junto con el enlace");
    let st: String = conn
        .query_row("select status from memories where id = ?1", [primera.id], |r| r.get(0))
        .unwrap();
    assert_eq!(st, "active");
}

// -- several processes at once ------------------------------------------------

#[test]
fn concurrent_saves_under_one_key_all_succeed_and_leave_a_single_active() {
    // With a deferred transaction, each process reads the current memory
    // under a read lock and then tries to upgrade it to a write one: SQLite
    // does not wait, it fails at once with `database is locked` (the ones
    // losing the race exit 1 and their memory is lost). With `begin
    // immediate` the write lock is taken before reading and the second one
    // waits its turn.
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["init"]);

    const N: usize = 12;
    let children: Vec<_> = (0..N)
        .map(|i| {
            Command::new(env!("CARGO_BIN_EXE_mem"))
                .arg("--db")
                .arg(d.path().join("mem.db"))
                .args(["save", &format!("El proveedor de pagos, versión {i}")])
                .args(["--project", "p", "--topic", "pagos"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for (i, h) in children.into_iter().enumerate() {
        let o = h.wait_with_output().unwrap();
        assert!(
            o.status.success(),
            "el guardado {i} falló: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let total: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(total, N as i64, "se perdió algún guardado");
    let active_ones: i64 = conn
        .query_row(
            "select count(*) from memories where project='p' and topic='pagos' and status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_ones, 1, "tiene que quedar exactamente una activa por clave");
}

// -- MCP ---------------------------------------------------------------------

fn call(conn: &rusqlite::Connection, tool: &str, args: serde_json::Value) -> String {
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":tool,"arguments":args}});
    let res = mcp::handle(conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    res["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn save_memory_with_a_topic_supersedes_and_says_which() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos"}));

    assert!(out.contains("replaces memory 1"), "{out}");
    let st: String = conn.query_row("select status from memories where id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(st, "superseded");
}

#[test]
fn save_memory_without_a_topic_or_with_an_empty_one_supersedes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    call(&conn, "save_memory", serde_json::json!({"text": PAGOS_NUEVO, "project": "p"}));
    call(&conn, "save_memory", serde_json::json!({
        "text": "Otro dato sobre pagos", "project": "p", "topic": ""}));

    let active_ones: i64 = conn
        .query_row("select count(*) from memories where status = 'active'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(active_ones, 3);
}

#[test]
fn the_save_memory_description_explains_when_to_use_topic() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let tools = res["result"]["tools"].as_array().unwrap();
    let save = tools.iter().find(|t| t["name"] == "save_memory").unwrap();

    assert!(save["inputSchema"]["properties"]["topic"].is_object(), "falta el parámetro topic");
    let desc = save["description"].as_str().unwrap().to_lowercase();
    for key in ["topic", "correction", "replace", "updated version"] {
        assert!(desc.contains(key), "la descripción no menciona {key:?}: {desc}");
    }
}

#[test]
fn search_and_get_memory_show_the_key_so_it_can_be_reused() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));

    let busqueda = call(&conn, "search_memory", serde_json::json!({"query": "proveedor pagos"}));
    assert!(busqueda.contains("[topic: pagos]"), "{busqueda}");
    let body = call(&conn, "get_memory", serde_json::json!({"id": 1}));
    assert!(body.contains("[topic: pagos]"), "{body}");
}

// -- supersedes: correcting one particular memory already saved ------------------

fn total(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap()
}

fn status(conn: &rusqlite::Connection, id: i64) -> String {
    conn.query_row("select status from memories where id = ?1", [id], |r| r.get(0)).unwrap()
}

#[test]
fn supersedes_corrects_a_memory_saved_without_a_key() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": 1}));

    assert!(out.contains("Replaces memory 1"), "{out}");
    assert_eq!(status(&conn, 1), "superseded");
    assert_eq!(status(&conn, 2), "active");
    let sup: i64 = conn.query_row("select supersedes_id from memories where id = 2", [], |r| r.get(0)).unwrap();
    assert_eq!(sup, 1);
    let hits = call(&conn, "search_memory", serde_json::json!({"query": "proveedor pagos"}));
    assert!(hits.contains("Adyen") && !hits.contains("Stripe"), "{hits}");
}

#[test]
fn supersedes_accepts_the_id_as_text() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": " 1 "}));
    assert_eq!(status(&conn, 1), "superseded");
}

#[test]
fn a_supersedes_that_is_not_an_id_saves_nothing() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": "la anterior"}));
    assert!(out.starts_with("Not saved"), "{out}");
    assert_eq!(total(&conn), 0);
}

#[test]
fn supersedes_of_a_missing_memory_is_rejected_in_words_and_saves_nothing() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": 999999}));
    assert!(out.starts_with("Not saved") && out.contains("999999"), "{out}");
    assert_eq!(total(&conn), 0, "el rechazo no puede dejar la memoria nueva guardada");
}

#[test]
fn supersedes_of_a_memory_that_is_not_active_is_rejected() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    anathem::maint::forget(&conn, 1).unwrap();
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": 1}));

    assert!(out.starts_with("Not saved") && out.contains("forgotten"), "{out}");
    assert_eq!(total(&conn), 1);
    assert_eq!(status(&conn, 1), "forgotten", "no se revive lo olvidado");
}

#[test]
fn supersedes_and_topic_pointing_at_one_memory_supersede_it_once() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos", "supersedes": 1}));

    assert!(out.contains("Topic «pagos»: replaces memory 1"), "{out}");
    assert_eq!(status(&conn, 1), "superseded");
    assert_eq!(status(&conn, 2), "active");
}

#[test]
fn supersedes_and_topic_pointing_at_different_memories_are_rejected() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));
    call(&conn, "save_memory", serde_json::json!({"text": "Otra cosa", "project": "p"}));
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos", "supersedes": 2}));

    assert!(out.starts_with("Not saved"), "{out}");
    assert!(out.contains("id:1") && out.contains("id:2"), "nombra las dos: {out}");
    assert_eq!(total(&conn), 2);
    assert_eq!(status(&conn, 1), "active");
    assert_eq!(status(&conn, 2), "active");
}

#[test]
fn supersedes_with_a_new_key_leaves_the_key_ready_for_next_time() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    call(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    let out = call(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos", "supersedes": 1}));
    assert!(out.contains("Replaces memory 1") && out.contains("new key"), "{out}");

    // The next one under the key supersedes the one that corrected the previous.
    call(&conn, "save_memory", serde_json::json!({
        "text": "El proveedor de pagos es Mercado Pago", "project": "p", "topic": "pagos"}));
    assert_eq!(status(&conn, 2), "superseded");
    assert_eq!(status(&conn, 3), "active");
}

#[test]
fn the_description_tells_when_to_use_supersedes_and_when_topic() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let save = res["result"]["tools"].as_array().unwrap().iter()
        .find(|t| t["name"] == "save_memory").unwrap().clone();

    assert_eq!(save["inputSchema"]["properties"]["supersedes"]["type"], "integer");
    let desc = save["description"].as_str().unwrap();
    assert!(desc.contains("`supersedes`") && desc.contains("`topic`"), "{desc}");
    assert!(desc.contains("KEEP EVOLVING"), "cuándo va topic: {desc}");
    assert!(desc.contains("ONE SPECIFIC EXISTING MEMORY"), "cuándo va supersedes: {desc}");
}

// -- export ------------------------------------------------------------------

#[test]
fn export_keeps_the_topic_key() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", "Sin clave", "--project", "p"]);
    let out_text = d.path().join("out");
    mem_ok(d.path(), &["export", "--out", out_text.to_str().unwrap()]);

    let mut with_topic = 0;
    for f in std::fs::read_dir(out_text.join("p")).unwrap() {
        let txt = std::fs::read_to_string(f.unwrap().path()).unwrap();
        if txt.contains("topic: \"pagos\"") {
            with_topic += 1;
        } else {
            assert!(!txt.contains("topic:"), "sin clave no se emite el campo: {txt}");
        }
    }
    assert_eq!(with_topic, 1);
}

// -- a database predating this change -----------------------------------------

/// The schema exactly as it was before topic keys: with no `memories.topic`.
const ESQUEMA_ANTERIOR: &str = r#"
pragma journal_mode = wal;
pragma foreign_keys = on;

create table if not exists memories (
  id            integer primary key,
  project       text not null,
  title         text not null,
  body          text not null,
  created       text not null,
  updated       text not null,
  event_start   text,
  event_end     text,
  source        text not null check (source in ('explicit','suggested','migrated')),
  status        text not null default 'active'
                check (status in ('active','superseded','draft','forgotten')),
  pinned        integer not null default 0,
  last_accessed text,
  access_count  integer not null default 0,
  supersedes_id integer references memories(id),
  ext_id        text unique
);

create table if not exists memory_tags (
  memory_id integer not null references memories(id) on delete cascade,
  tag       text not null,
  primary key (memory_id, tag)
);

create table if not exists memory_refs (
  memory_id integer not null references memories(id) on delete cascade,
  kind      text not null check (kind in ('ticket','pr','sha')),
  value     text not null,
  primary key (memory_id, kind, value)
);

create table if not exists chunks (
  id        integer primary key,
  memory_id integer not null references memories(id) on delete cascade,
  ord       integer not null,
  body      text not null
);

create virtual table if not exists chunks_fts using fts5(
  body,
  content='chunks',
  content_rowid='id',
  tokenize="unicode61 remove_diacritics 2"
);

create table if not exists chunk_vectors (
  chunk_id integer primary key references chunks(id) on delete cascade,
  vec      blob not null   -- f32 little endian x dims, ya normalizado
);

create table if not exists meta (
  clave text primary key,
  valor text not null
);

create table if not exists search_log (
  id           integer primary key,
  ts           text not null,
  query        text not null,
  project      text,
  returned_ids text not null,
  budget_used  integer not null,
  followed_up  integer not null default 0
);

create index if not exists idx_mem_project_status on memories(project, status);
create index if not exists idx_chunks_memory      on chunks(memory_id);
create index if not exists idx_tags_tag           on memory_tags(tag);
create index if not exists idx_refs_value         on memory_refs(value);

create trigger if not exists chunks_ai after insert on chunks begin
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
create trigger if not exists chunks_ad after delete on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
end;
create trigger if not exists chunks_au after update on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
"#;

fn older_database(dir: &Path) {
    let conn = rusqlite::Connection::open(dir.join("mem.db")).unwrap();
    conn.execute_batch(ESQUEMA_ANTERIOR).unwrap();
    conn.execute(
        "insert into memories (project, title, body, created, updated, source)
         values ('p', 'La base es Postgres', 'La base es Postgres', '2026-01-01', '2026-01-01', 'explicit')",
        [],
    )
    .unwrap();
    conn.execute("insert into chunks (memory_id, ord, body) values (1, 0, 'La base es Postgres')", [])
        .unwrap();
    let tiene: i64 = conn
        .query_row("select count(*) from pragma_table_info('memories') where name='topic'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tiene, 0, "la base de partida no debería tener la columna topic");
}

#[test]
fn an_older_database_still_opens_and_keeps_its_memories() {
    let d = tempfile::tempdir().unwrap();
    older_database(d.path());

    let out = mem_ok(d.path(), &["search", "postgres"]);
    assert!(out.contains("La base es Postgres"), "{out}");
    // opening it twice cannot fail over the already-added column
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    db::open(&d.path().join("mem.db")).unwrap();
    let n: i64 = conn.query_row("select count(*) from memories where topic is null", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "la memoria vieja queda sin clave");
}

#[test]
fn an_older_database_accepts_saving_with_a_key_and_listing_topics() {
    let d = tempfile::tempdir().unwrap();
    older_database(d.path());

    mem_ok(d.path(), &["save", "La base es MySQL", "--project", "p", "--topic", "base"]);
    let out = mem_ok(d.path(), &["save", "La base es Oracle", "--project", "p", "--topic", "base"]);
    assert!(out.contains("replaces memory 2"), "{out}");
    assert_eq!(status_(d.path(), 1), "active", "la anterior sin clave no se toca");
    assert_eq!(status_(d.path(), 2), "superseded");
    assert!(mem_ok(d.path(), &["topics"]).contains("base"));
}

#[test]
fn an_index_over_project_and_key_exists() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let cols: Vec<String> = conn
        .prepare("select name from pragma_index_info('idx_mem_project_topic') order by seqno")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(cols, ["project", "topic"]);
}

// -- structural invariant: one active per (project, key) -------------------------

fn row_with_key(conn: &rusqlite::Connection, project: &str, topic: Option<&str>, status: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "insert into memories (project, title, body, created, updated, source, status, topic)
         values (?1, 't', 'b', '2026-01-01', '2026-01-01', 'explicit', ?3, ?2)",
        rusqlite::params![project, topic, status],
    )
}

#[test]
fn the_database_forbids_two_active_memories_under_one_key_in_a_project() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    row_with_key(&conn, "p", Some("pagos"), "active").unwrap();
    let err = row_with_key(&conn, "p", Some("pagos"), "active")
        .expect_err("una segunda activa con la misma clave tiene que rechazarse");
    assert!(err.to_string().contains("UNIQUE"), "{err}");
}

#[test]
fn the_unique_index_does_not_bother_anything_but_the_current_one() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    row_with_key(&conn, "p", Some("pagos"), "active").unwrap();
    // another project, the same key
    row_with_key(&conn, "q", Some("pagos"), "active").unwrap();
    // the key's history: there can be several superseded and forgotten ones
    for st in ["superseded", "superseded", "forgotten"] {
        row_with_key(&conn, "p", Some("pagos"), st).unwrap();
    }
    // with no key there is no cap
    for _ in 0..3 {
        row_with_key(&conn, "p", None, "active").unwrap();
    }
}

#[test]
fn reviving_a_forgotten_one_when_a_current_exists_fails_instead_of_leaving_two_active() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let older = ingest::save_with_topic(&conn, PAGOS_VIEJO, "p", vec![], Some("pagos"), None).unwrap();
    anathem::maint::forget(&conn, older.id).unwrap();
    ingest::save_with_topic(&conn, PAGOS_NUEVO, "p", vec![], Some("pagos"), None).unwrap();

    let r = conn.execute("update memories set status = 'active' where id = ?1", [older.id]);
    assert!(r.is_err(), "revivir la olvidada habría dejado dos activas para la clave");
}

#[test]
fn an_older_database_gets_the_unique_index_too() {
    let d = tempfile::tempdir().unwrap();
    older_database(d.path());
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    row_with_key(&conn, "p", Some("k"), "active").unwrap();
    assert!(row_with_key(&conn, "p", Some("k"), "active").is_err());
}
