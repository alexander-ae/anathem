//! Lo que encuentra alguien que instala anathem por primera vez.
//! Estos tests invocan el binario real: si la experiencia del primer día se
//! rompe, fallan aunque la biblioteca esté perfecta.

use std::process::Command;

fn mem(db: &std::path::Path, args: &[&str]) -> (String, String, bool) {
    let salida = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&salida.stdout).to_string(),
        String::from_utf8_lossy(&salida.stderr).to_string(),
        salida.status.success(),
    )
}

/// Buscar en una base vacía debe DECIRLO. Antes no imprimía nada, así que
/// era indistinguible de un binario roto.
#[test]
fn buscar_sin_resultados_lo_dice() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    mem(&db, &["init"]);
    let (out, _, ok) = mem(&db, &["search", "cualquier cosa"]);
    assert!(ok, "search debería salir 0 aunque no haya resultados");
    assert!(!out.trim().is_empty(), "search no imprimió nada; el usuario no puede saber si funcionó");
}

/// `eval` sin eval set debe explicar qué es y cómo armarlo, no devolver un
/// error de archivo no encontrado.
#[test]
fn eval_sin_set_explica_que_hace_falta() {
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

/// El ciclo mínimo del primer día: guardar algo y encontrarlo.
#[test]
fn el_ciclo_minimo_funciona_sin_configurar_nada() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    mem(&db, &["init"]);
    let (out, _, ok) = mem(&db, &["save", "Se decidió usar SQLite por un bug de concurrencia"]);
    assert!(ok && out.contains("guardada"), "no guardó: {out}");
    assert!(out.contains('['), "no clasificó con las reglas por defecto: {out}");
    let (out, _, _) = mem(&db, &["search", "SQLite concurrencia"]);
    assert!(out.contains("SQLite"), "no encontró lo que acababa de guardar: {out}");
}

#[test]
fn el_presupuesto_de_la_busqueda_se_puede_subir() {
    // Sin esto no hay forma de inspeccionar más candidatos de los que entran
    // en el contexto de un agente, que es justo lo que hace falta para medir
    // si conviene devolver más y más cortos.
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("m.db");
    let mem = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
            .args(["--db", db.to_str().unwrap()]).args(args).output().unwrap()
    };
    for i in 0..8 {
        let cuerpo = format!("memoria numero {i} sobre cacheo de propiedades y {}", "relleno ".repeat(60));
        mem(&["save", &cuerpo, "--project", "p"]);
    }
    let apretado = mem(&["search", "cacheo propiedades", "--limit", "8"]);
    let holgado = mem(&["search", "cacheo propiedades", "--limit", "8", "--budget", "100000"]);
    let n = |o: &std::process::Output| String::from_utf8_lossy(&o.stdout)
        .lines().filter(|l| l.trim_start().starts_with(char::is_numeric) && l.contains('.')).count();
    assert!(n(&holgado) > n(&apretado),
        "con más presupuesto tienen que entrar más resultados: {} vs {}", n(&holgado), n(&apretado));
}
