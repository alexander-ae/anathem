//! El servidor MCP vive horas; el paquete puede cambiar debajo.

mod apoyo;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap()
}

#[test]
fn el_servidor_no_mezcla_vectores_si_cambia_el_paquete_debajo() {
    // `mem serve` abría el paquete una vez al arrancar y no volvía a mirar
    // el registro nunca más. Un `mem model install` desde otra terminal
    // —dos comandos de primera clase— dejaba al servidor escribiendo
    // vectores del paquete viejo junto a los del nuevo. Con dimensiones
    // distintas el blob corto queda muerto; con las mismas —el caso real,
    // porque el formato fija 256 para todo paquete— nada lo distingue y la
    // búsqueda calcula cosenos contra dos espacios vectoriales mezclados.
    //
    // Por eso los dos paquetes de este test tienen las MISMAS dimensiones:
    // lo único que los separa es hacia dónde apunta cada palabra.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    let b = d.path().join("b.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es"); // "▁mundo" -> eje 1
    apoyo::escribir_pack_permutado(&b, "otro-es"); // "▁mundo" -> eje 2

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
    let mut entrada = hijo.stdin.take().unwrap();
    let mut salida = BufReader::new(hijo.stdout.take().unwrap());

    // Un mensaje cualquiera primero, para que el servidor esté arriba y ya
    // haya resuelto el paquete A.
    writeln!(entrada, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).unwrap();
    let mut linea = String::new();
    salida.read_line(&mut linea).unwrap();
    assert!(linea.contains("protocolVersion"), "el servidor no arrancó: {linea}");

    // Y ahora, desde otra terminal, alguien cambia el paquete.
    let o = mem(&db, &["model", "install", b.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    // El agente guarda una memoria por MCP, sin enterarse de nada.
    writeln!(
        entrada,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"save_memory","arguments":{{"text":"mundo"}}}}}}"#
    )
    .unwrap();
    linea.clear();
    salida.read_line(&mut linea).unwrap();
    assert!(linea.contains("Guardada"), "el guardado por MCP falló: {linea}");

    drop(entrada);
    hijo.wait().unwrap();

    let conn = rusqlite::Connection::open(&db).unwrap();
    let v = apoyo::leer_vector(
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
fn el_servidor_deja_de_usar_el_paquete_cuando_lo_quitan() {
    // El otro lado del mismo agujero: `mem model remove` desde otra
    // terminal borra los vectores y el registro, y el servidor seguía
    // escribiendo vectores de un paquete que la base ya no reconoce.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    let a = d.path().join("a.anathem-pack");
    apoyo::escribir_pack_mini(&a, "mini-es");
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
    let mut entrada = hijo.stdin.take().unwrap();
    let mut salida = BufReader::new(hijo.stdout.take().unwrap());
    writeln!(entrada, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).unwrap();
    let mut linea = String::new();
    salida.read_line(&mut linea).unwrap();

    assert!(mem(&db, &["model", "remove"]).status.success());

    writeln!(
        entrada,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"save_memory","arguments":{{"text":"mundo"}}}}}}"#
    )
    .unwrap();
    linea.clear();
    salida.read_line(&mut linea).unwrap();
    assert!(linea.contains("Guardada"), "el guardado por MCP falló: {linea}");
    drop(entrada);
    hijo.wait().unwrap();

    let conn = rusqlite::Connection::open(&db).unwrap();
    let n: i64 = conn.query_row("select count(*) from chunk_vectors", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "el servidor siguió escribiendo vectores de un paquete que ya no existe");
}
