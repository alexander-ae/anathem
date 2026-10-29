//! The MCP server cannot pay for the tokenizer's heap (~54 MB) on a session
//! that never searches nor saves. This test measures the process's real
//! memory rather than inferring it: it starts `mem serve` with a genuinely
//! installed pack, reads `VmRSS` from `/proc/<pid>/status` at two moments,
//! and checks the pack is opened only on the first `search_memory`, not
//! before.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The real test pack (22.5 MB, en-es) the test bench leaves behind. It is
/// not part of this repo: when absent the test skips rather than fails,
/// because its absence says nothing about whether the code works.
const PAQUETE_REAL: &str = "/home/alguien/anathem-bench/exp/en-es.anathem-pack";

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap()
}

/// Reads `VmRSS` (real resident memory, in kB) from `/proc/<pid>/status`.
#[cfg(target_os = "linux")]
fn vmrss_kb(pid: u32) -> u64 {
    let content = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_else(|e| panic!("no se pudo leer /proc/{pid}/status: {e}"));
    for line_ in content.lines() {
        if let Some(resto) = line_.strip_prefix("VmRSS:") {
            return resto
                .split_whitespace()
                .next()
                .unwrap_or_else(|| panic!("línea VmRSS sin valor: {line_:?}"))
                .parse()
                .unwrap_or_else(|e| panic!("VmRSS no numérico ({line_:?}): {e}"));
        }
    }
    panic!("/proc/{pid}/status no tiene línea VmRSS");
}

#[cfg(target_os = "linux")]
fn send_(entry: &mut impl Write, json: &str) {
    writeln!(entry, "{json}").unwrap();
}

#[cfg(target_os = "linux")]
fn read_line(out_text: &mut impl BufRead) -> String {
    let mut line_ = String::new();
    out_text.read_line(&mut line_).unwrap();
    line_
}

#[cfg(target_os = "linux")]
fn kill_(mut hijo: Child) {
    let _ = hijo.kill();
    let _ = hijo.wait();
}

#[test]
#[cfg(target_os = "linux")]
fn the_server_opens_the_pack_only_on_the_first_search() {
    if !Path::new(PAQUETE_REAL).exists() {
        eprintln!(
            "se salta: no está {PAQUETE_REAL} (paquete real del banco de pruebas, \
             fuera del repo)"
        );
        return;
    }

    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mem.db");
    assert!(mem(&db, &["init"]).status.success());
    let o = mem(&db, &["model", "install", PAQUETE_REAL]);
    assert!(o.status.success(), "no se pudo instalar el paquete real: {}", String::from_utf8_lossy(&o.stderr));

    let mut hijo = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(&db)
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = hijo.id();
    let mut entry = hijo.stdin.take().unwrap();
    let mut out_text = BufReader::new(hijo.stdout.take().unwrap());

    send_(&mut entry, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let resp = read_line(&mut out_text);
    assert!(resp.contains("protocolVersion"), "el servidor no arrancó: {resp}");

    send_(&mut entry, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let resp = read_line(&mut out_text);
    assert!(resp.contains("search_memory"), "tools/list no respondió bien: {resp}");

    // With a pack installed, `initialize` and `tools/list` do not need it:
    // neither of the two tools they expose has been called yet. The
    // threshold is deliberately generous (the process with no pack open
    // measures ~7 MB; the tokenizer's heap alone, ~54 MB more) so as not to
    // be brittle against the normal noise in RSS.
    let rss_antes = vmrss_kb(pid);
    assert!(
        rss_antes < 25_000,
        "el servidor ya usa {rss_antes} kB tras el handshake, sin haber buscado ni \
         guardado nada: parece que abrió el paquete de vocabulario antes de que \
         hiciera falta"
    );

    send_(
        &mut entry,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_memory","arguments":{"query":"hola"}}}"#,
    );
    let resp = read_line(&mut out_text);
    assert!(resp.contains("\"content\""), "la búsqueda no respondió bien: {resp}");

    let rss_despues = vmrss_kb(pid);
    assert!(
        rss_despues > rss_antes + 15_000,
        "tras la primera búsqueda la memoria residente debería subir con el heap \
         del tokenizador (~54 MB); antes: {rss_antes} kB, después: {rss_despues} kB"
    );

    kill_(hijo);
}

/// Companion to the test above for platforms without `/proc`: it does not
/// vanish from the test count, it just says why it does not run.
#[test]
#[cfg(not(target_os = "linux"))]
fn the_server_opens_the_pack_only_on_the_first_search() {
    eprintln!(
        "se salta en esta plataforma: mide memoria real vía /proc/<pid>/status, \
         que solo existe en Linux."
    );
}

#[cfg(target_os = "linux")]
#[test]
fn it_warns_on_startup_when_the_registered_pack_is_gone() {
    // Lazy opening means a session that only lists tags never opens the
    // pack, and so never finds out it is missing. Checking that the file
    // exists costs one `stat`, so that case is caught on startup without
    // paying for the open.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    let conn = anathem::db::open(&db).unwrap();
    anathem::vecstore::adopt_pack(&conn, "fantasma", "abc", 256).unwrap();
    drop(conn);

    let mut s = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .args(["--db", db.to_str().unwrap(), "serve"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let e = s.stdin.as_mut().unwrap();
        writeln!(e, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"t","version":"1"}}}}}}"#).unwrap();
        e.flush().unwrap();
    }
    {
        use std::io::BufRead;
        let o = std::io::BufReader::new(s.stdout.as_mut().unwrap());
        let _ = o.lines().next();
    }
    let _ = s.kill();
    let out_text = s.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out_text.stderr);
    assert!(err.contains("fantasma") && err.contains("falta"),
        "el arranque tiene que avisar que el paquete registrado no está:\n{err}");
}
