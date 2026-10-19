//! El servidor MCP no puede pagar el heap del tokenizador (~54 MB) por una
//! sesión que nunca busca ni guarda. Este test mide memoria real del
//! proceso, no la infiere: arranca `mem serve` con un paquete instalado de
//! verdad, lee `VmRSS` de `/proc/<pid>/status` en dos momentos, y comprueba
//! que el paquete se abre recién en la primera `search_memory`, no antes.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// El paquete real de prueba (22,5 MB, en-es) que deja el banco de pruebas.
/// No es parte de este repo: si no está, el test se salta en vez de fallar,
/// porque su ausencia no dice nada sobre si el código anda.
const PAQUETE_REAL: &str = "/home/alguien/anathem-bench/exp/en-es.anathem-pack";

fn mem(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mem")).arg("--db").arg(db).args(args).output().unwrap()
}

/// Lee `VmRSS` (memoria residente real, en kB) de `/proc/<pid>/status`.
#[cfg(target_os = "linux")]
fn vmrss_kb(pid: u32) -> u64 {
    let contenido = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_else(|e| panic!("no se pudo leer /proc/{pid}/status: {e}"));
    for linea in contenido.lines() {
        if let Some(resto) = linea.strip_prefix("VmRSS:") {
            return resto
                .split_whitespace()
                .next()
                .unwrap_or_else(|| panic!("línea VmRSS sin valor: {linea:?}"))
                .parse()
                .unwrap_or_else(|e| panic!("VmRSS no numérico ({linea:?}): {e}"));
        }
    }
    panic!("/proc/{pid}/status no tiene línea VmRSS");
}

#[cfg(target_os = "linux")]
fn enviar(entrada: &mut impl Write, json: &str) {
    writeln!(entrada, "{json}").unwrap();
}

#[cfg(target_os = "linux")]
fn leer_linea(salida: &mut impl BufRead) -> String {
    let mut linea = String::new();
    salida.read_line(&mut linea).unwrap();
    linea
}

#[cfg(target_os = "linux")]
fn matar(mut hijo: Child) {
    let _ = hijo.kill();
    let _ = hijo.wait();
}

#[test]
#[cfg(target_os = "linux")]
fn el_servidor_abre_el_paquete_recien_en_la_primera_busqueda() {
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
    let mut entrada = hijo.stdin.take().unwrap();
    let mut salida = BufReader::new(hijo.stdout.take().unwrap());

    enviar(&mut entrada, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let resp = leer_linea(&mut salida);
    assert!(resp.contains("protocolVersion"), "el servidor no arrancó: {resp}");

    enviar(&mut entrada, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let resp = leer_linea(&mut salida);
    assert!(resp.contains("search_memory"), "tools/list no respondió bien: {resp}");

    // Con un paquete instalado, `initialize` y `tools/list` no lo necesitan:
    // ninguna de las dos tools que exponen se llamó todavía. El umbral es
    // holgado a propósito (el proceso sin paquete abierto mide ~7 MB; el
    // heap del tokenizador solo, ~54 MB más) para no ser frágil frente al
    // ruido normal de RSS.
    let rss_antes = vmrss_kb(pid);
    assert!(
        rss_antes < 25_000,
        "el servidor ya usa {rss_antes} kB tras el handshake, sin haber buscado ni \
         guardado nada: parece que abrió el paquete de vocabulario antes de que \
         hiciera falta"
    );

    enviar(
        &mut entrada,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_memory","arguments":{"query":"hola"}}}"#,
    );
    let resp = leer_linea(&mut salida);
    assert!(resp.contains("\"content\""), "la búsqueda no respondió bien: {resp}");

    let rss_despues = vmrss_kb(pid);
    assert!(
        rss_despues > rss_antes + 15_000,
        "tras la primera búsqueda la memoria residente debería subir con el heap \
         del tokenizador (~54 MB); antes: {rss_antes} kB, después: {rss_despues} kB"
    );

    matar(hijo);
}

/// Compañero del test de arriba para plataformas sin `/proc`: no desaparece
/// del conteo de tests, solo dice por qué no corre.
#[test]
#[cfg(not(target_os = "linux"))]
fn el_servidor_abre_el_paquete_recien_en_la_primera_busqueda() {
    eprintln!(
        "se salta en esta plataforma: mide memoria real vía /proc/<pid>/status, \
         que solo existe en Linux."
    );
}

#[cfg(target_os = "linux")]
#[test]
fn avisa_al_arrancar_si_el_paquete_registrado_desaparecio() {
    // La apertura perezosa hace que una sesión que solo lista tags nunca
    // abra el paquete, y por lo tanto nunca descubra que falta. Comprobar
    // que el archivo exista cuesta un `stat`, así que ese caso se detecta
    // al arrancar sin pagar la apertura.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    let conn = anathem::db::open(&db).unwrap();
    anathem::vecstore::adoptar_pack(&conn, "fantasma", "abc", 256).unwrap();
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
    let salida = s.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&salida.stderr);
    assert!(err.contains("fantasma") && err.contains("falta"),
        "el arranque tiene que avisar que el paquete registrado no está:\n{err}");
}
