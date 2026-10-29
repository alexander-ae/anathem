//! The MCP server lives for hours; the pack can change underneath it.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap()
}

#[test]
fn the_server_does_not_mix_vectors_when_the_pack_changes_underneath() {
    // `mem serve` opened the pack once on startup and never looked at the
    // registry again. A `mem model install` from another terminal — two
    // first-class commands — left the server writing the old pack's vectors
    // alongside the new one's. With different dimensions the short blob is
    // dead; with the same ones — the real case, since the format fixes 256
    // for every pack — nothing tells them apart and search computes cosines
    // against two mixed vector spaces.
    //
    // That is why both packs in this test have the SAME dimensions: the only
    // thing separating them is where each word points.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    support::write_pack_mini(&a, "mini-es"); // "▁mundo" -> eje 1
    support::write_pack_permuted(&b, "otro-es"); // "▁mundo" -> eje 2

    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "hola"]).status.success());
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());

    let mut hijo = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(&db)
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut entry = hijo.stdin.take().unwrap();
    let mut out_text = BufReader::new(hijo.stdout.take().unwrap());

    // Some message first, so the server is up and has already resolved pack
    // A.
    writeln!(entry, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).unwrap();
    let mut line_ = String::new();
    out_text.read_line(&mut line_).unwrap();
    assert!(line_.contains("protocolVersion"), "el servidor no arrancó: {line_}");

    // And now, from another terminal, someone switches the pack.
    let o = mem(&db, &["model", "install", b.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    // The agent saves a memory over MCP, none the wiser.
    writeln!(
        entry,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"save_memory","arguments":{{"text":"mundo"}}}}}}"#
    )
    .unwrap();
    line_.clear();
    out_text.read_line(&mut line_).unwrap();
    assert!(line_.contains("Guardada"), "el guardado por MCP falló: {line_}");

    drop(entry);
    hijo.wait().unwrap();

    let conn = rusqlite::Connection::open(&db).unwrap();
    let v = support::read_vector(
        &conn,
        "select v.vec from chunk_vectors v
           join chunks c on c.id = v.chunk_id
          where c.body = 'mundo'",
    );
    assert!(
        v[2] > 0.9,
        "el fragmento guardado por MCP se embebió con el paquete que el servidor tenía \
         abierto al arrancar, no con el que está instalado: {v:?}"
    );
}

#[test]
fn the_server_stops_using_the_pack_once_it_is_removed() {
    // The other side of the same hole: `mem model remove` from another
    // terminal deletes the vectors and the registry, and the server kept
    // writing vectors of a pack the database no longer recognizes.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    support::write_pack_mini(&a, "mini-es");
    assert!(mem(&db, &["init"]).status.success());
    assert!(mem(&db, &["save", "hola"]).status.success());
    assert!(mem(&db, &["model", "install", a.to_str().unwrap()]).status.success());

    let mut hijo = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(&db)
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut entry = hijo.stdin.take().unwrap();
    let mut out_text = BufReader::new(hijo.stdout.take().unwrap());
    writeln!(entry, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).unwrap();
    let mut line_ = String::new();
    out_text.read_line(&mut line_).unwrap();

    assert!(mem(&db, &["model", "remove"]).status.success());

    writeln!(
        entry,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"save_memory","arguments":{{"text":"mundo"}}}}}}"#
    )
    .unwrap();
    line_.clear();
    out_text.read_line(&mut line_).unwrap();
    assert!(line_.contains("Guardada"), "el guardado por MCP falló: {line_}");
    drop(entry);
    hijo.wait().unwrap();

    let conn = rusqlite::Connection::open(&db).unwrap();
    let n: i64 = conn.query_row("select count(*) from chunk_vectors", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "el servidor siguió escribiendo vectores de un paquete que ya no existe");
}
