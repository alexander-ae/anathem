//! The flags moved to English (task 3). The Spanish forms stay as hidden
//! aliases: anything already written in a script keeps working, and the help
//! shows one name per flag.

use std::path::Path;

fn mem(db: &Path, args: &[&str]) -> (String, bool) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db").arg(db).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).to_string()
            + &String::from_utf8_lossy(&o.stderr),
        o.status.success(),
    )
}

fn base(d: &Path) -> std::path::PathBuf {
    let db = d.join("m.db");
    assert!(mem(&db, &["init"]).1);
    db
}

#[test]
fn the_new_english_flags_work() {
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    assert!(mem(&db, &["recent", "--since", "6h", "--limit", "3"]).1);
    assert!(mem(&db, &["recent", "--all"]).1);
}

#[test]
fn the_old_spanish_flags_still_work() {
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    assert!(mem(&db, &["recent", "--desde", "6h", "--limite", "3"]).1);
    assert!(mem(&db, &["recent", "--todos"]).1);
}

#[test]
fn import_takes_from_and_merge_in_english() {
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    let vacia = d.path().join("vacia");
    std::fs::create_dir(&vacia).unwrap();
    assert!(mem(&db, &["import", "--from", vacia.to_str().unwrap(), "--merge"]).1);
    // Y la forma vieja sigue andando.
    assert!(mem(&db, &["import", "--desde", vacia.to_str().unwrap(), "--fusionar"]).1);
}

#[test]
fn the_old_flags_are_hidden_from_help() {
    let d = tempfile::tempdir().unwrap();
    let (help, ok) = mem(&d.path().join("m.db"), &["recent", "--help"]);
    assert!(ok, "{help}");
    assert!(help.contains("--since"), "la nueva tiene que aparecer: {help}");
    assert!(help.contains("--limit"), "la nueva tiene que aparecer: {help}");
    assert!(!help.contains("--desde"), "la vieja no puede aparecer: {help}");
    assert!(!help.contains("--limite"), "la vieja no puede aparecer: {help}");
}

#[test]
fn the_help_is_english_and_well_formed() {
    let d = tempfile::tempdir().unwrap();
    let (help, ok) = mem(&d.path().join("m.db"), &["--help"]);
    assert!(ok, "{help}");
    // La primera línea de cada doc comment es el resumen del subcomando: si
    // alguno queda mal cortado al traducir, desaparece de la ayuda.
    for sub in ["save", "search", "recent", "doctor", "export", "import", "model", "serve"] {
        assert!(help.contains(sub), "falta el subcomando {sub}: {help}");
    }
    for palabra in ["memoria", "búsqueda", "proyecto", "Guarda", "archivo", "paquete"] {
        assert!(!help.contains(palabra), "queda español en la ayuda: «{palabra}»\n{help}");
    }
}

#[test]
fn the_unbounded_window_keyword_is_the_one_the_help_advertises() {
    // `--help` decía `--since all` y el código solo aceptaba `todo`: la
    // ayuda enseñaba un valor que el binario rechaza. Lo introduje al
    // traducir el doc comment sin traducir el centinela que compara.
    let d = tempfile::tempdir().unwrap();
    let db = base(d.path());
    assert!(mem(&db, &["recent", "--since", "all"]).1, "`all` es lo que anuncia la ayuda");
    // Y la forma vieja se sigue aceptando, como con las banderas.
    assert!(mem(&db, &["recent", "--since", "todo"]).1, "`todo` seguía andando y no se rompe");
}
